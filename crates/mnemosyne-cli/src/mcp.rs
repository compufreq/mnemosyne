//! MCP (Model Context Protocol) stdio server.
//!
//! Speaks JSON-RPC 2.0 over newline-delimited stdio, the transport MCP
//! clients (Claude Code, Cursor, etc.) use for local servers. Covers the
//! palace surface the original mempalace MCP server exposed: drawer
//! reads/writes, search, wake-up, knowledge-graph operations, cross-wing
//! tunnels, hallways, agent diaries, stats, dedup, and integrity
//! verification — all on top of the vault security layer.

use anyhow::Result;
use serde_json::{json, Value};
use std::io::{BufRead, Write};

use mnemosyne_core::{normalize_content, Drawer};
use mnemosyne_store::{PalaceStore, SearchOptions};

const PROTOCOL_VERSION: &str = "2024-11-05";

/// Tools that mutate the palace — rejected when the server runs read-only
/// (the team-server deployment exposes recall without write access).
const WRITE_TOOLS: &[&str] = &[
    "mnemosyne_save",
    "mnemosyne_add_drawer",
    "mnemosyne_update_drawer",
    "mnemosyne_delete_drawer",
    "mnemosyne_delete_by_source",
    "mnemosyne_create_tunnel",
    "mnemosyne_delete_tunnel",
    "mnemosyne_kg_add",
    "mnemosyne_kg_invalidate",
    "mnemosyne_kg_supersede",
    "mnemosyne_diary_write",
    "mnemosyne_dedup",
];

/// Transport-independent MCP message handler, shared by the stdio and HTTP
/// servers.
pub struct McpHandler {
    store: PalaceStore,
    read_only: bool,
}

impl McpHandler {
    pub fn new(store: PalaceStore, read_only: bool) -> Self {
        Self { store, read_only }
    }

    /// Handle one JSON-RPC message. Returns `None` for notifications.
    pub fn handle(&mut self, msg: &Value) -> Option<Value> {
        let id = msg.get("id").cloned().unwrap_or(Value::Null);
        let method = msg
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default();

        // Notifications (no id) get no response.
        msg.get("id")?;

        let _span = mnemosyne_obs::scope_request(method, None);
        Some(match method {
            "initialize" => json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "protocolVersion": PROTOCOL_VERSION,
                    "capabilities": { "tools": {} },
                    "serverInfo": { "name": "mnemosyne", "version": env!("CARGO_PKG_VERSION") }
                }
            }),
            "ping" => json!({ "jsonrpc": "2.0", "id": id, "result": {} }),
            "tools/list" => json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": { "tools": tool_definitions() }
            }),
            "tools/call" => {
                let name = msg
                    .pointer("/params/name")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let args = msg
                    .pointer("/params/arguments")
                    .cloned()
                    .unwrap_or(json!({}));
                let result = if self.read_only && WRITE_TOOLS.contains(&name.as_str()) {
                    Err(anyhow::anyhow!(
                        "server is read-only: {name} is not allowed"
                    ))
                } else {
                    call_tool(&mut self.store, &name, &args)
                };
                match result {
                    Ok(text) => json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "content": [ { "type": "text", "text": text } ],
                            "isError": false
                        }
                    }),
                    Err(e) => json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "content": [ { "type": "text", "text": format!("error: {e}") } ],
                            "isError": true
                        }
                    }),
                }
            }
            _ => error_response(id, -32601, &format!("method not found: {method}")),
        })
    }
}

pub fn serve(store: PalaceStore) -> Result<()> {
    let mut handler = McpHandler::new(store, false);
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                write_msg(
                    &mut out,
                    &error_response(Value::Null, -32700, &format!("parse error: {e}")),
                )?;
                continue;
            }
        };
        if let Some(response) = handler.handle(&msg) {
            write_msg(&mut out, &response)?;
        }
    }
    Ok(())
}

fn write_msg(out: &mut impl Write, msg: &Value) -> Result<()> {
    serde_json::to_writer(&mut *out, msg)?;
    out.write_all(b"\n")?;
    out.flush()?;
    Ok(())
}

fn error_response(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

/// Terse helper for tool definitions: (name, description, properties, required).
fn tool(name: &str, desc: &str, props: Value, required: &[&str]) -> Value {
    json!({
        "name": name,
        "description": desc,
        "inputSchema": { "type": "object", "properties": props, "required": required }
    })
}

fn tool_definitions() -> Value {
    let s = |d: &str| json!({ "type": "string", "description": d });
    let i = |d: &str| json!({ "type": "integer", "description": d });
    let n = |d: &str| json!({ "type": "number", "description": d });
    json!([
        // --- palace core ---
        tool("mnemosyne_save", "Save one memory verbatim (encrypted + integrity-tagged at rest).",
            json!({ "content": s("verbatim text"), "wing": s("person/project partition"), "room": s("topic") }),
            &["content"]),
        tool("mnemosyne_search", "Hybrid semantic + lexical search over stored memories.",
            json!({ "query": s("search query"), "wing": s("scope to wing"), "room": s("scope to room"), "limit": i("max results") }),
            &["query"]),
        tool("mnemosyne_wake_up", "Load session context: recent essential memories.",
            json!({ "wing": s("scope to wing") }), &[]),
        tool("mnemosyne_verify", "Verify every record's HMAC and the tamper-evident audit chain.",
            json!({}), &[]),
        tool("mnemosyne_status", "Palace statistics: records, wings, rooms, KG, size, security level.",
            json!({}), &[]),
        // --- drawers ---
        tool("mnemosyne_get_drawer", "Fetch one drawer verbatim by id.",
            json!({ "id": s("drawer id") }), &["id"]),
        tool("mnemosyne_add_drawer", "File a drawer with explicit wing/room/source.",
            json!({ "content": s("verbatim text"), "wing": s("wing"), "room": s("room"), "source_file": s("origin") }),
            &["content"]),
        tool("mnemosyne_update_drawer", "Replace a drawer's content in place (re-sealed, re-tagged).",
            json!({ "id": s("drawer id"), "content": s("new content") }), &["id", "content"]),
        tool("mnemosyne_delete_drawer", "Delete a drawer (logs a tamper-evident tombstone).",
            json!({ "id": s("drawer id") }), &["id"]),
        tool("mnemosyne_list_drawers", "Page through drawer summaries.",
            json!({ "wing": s("scope"), "room": s("scope"), "limit": i("page size"), "offset": i("page start") }), &[]),
        tool("mnemosyne_delete_by_source", "Delete every drawer mined from a source file.",
            json!({ "source_file": s("source path") }), &["source_file"]),
        tool("mnemosyne_check_duplicate", "Check whether exact content is already filed.",
            json!({ "content": s("content to check") }), &["content"]),
        // --- navigation ---
        tool("mnemosyne_list_wings", "Wings with drawer counts.", json!({}), &[]),
        tool("mnemosyne_list_rooms", "Rooms and counts within a wing.",
            json!({ "wing": s("wing") }), &["wing"]),
        tool("mnemosyne_get_taxonomy", "Full wing → room tree.", json!({}), &[]),
        tool("mnemosyne_create_tunnel", "Connect two wings.",
            json!({ "from_wing": s("origin"), "to_wing": s("destination"), "label": s("why related") }),
            &["from_wing", "to_wing"]),
        tool("mnemosyne_list_tunnels", "List tunnels, optionally touching one wing.",
            json!({ "wing": s("filter") }), &[]),
        tool("mnemosyne_follow_tunnel", "Recent drawers from a tunnel's destination wing.",
            json!({ "id": s("tunnel id"), "limit": i("max drawers") }), &["id"]),
        tool("mnemosyne_delete_tunnel", "Remove a tunnel.",
            json!({ "id": s("tunnel id") }), &["id"]),
        tool("mnemosyne_traverse", "Wings reachable from a start wing over tunnels (BFS).",
            json!({ "start": s("start wing"), "depth": i("max hops") }), &["start"]),
        tool("mnemosyne_list_hallways", "Entity pairs co-occurring across a wing's drawers.",
            json!({ "wing": s("wing"), "top": i("max pairs") }), &["wing"]),
        tool("mnemosyne_get_closet_index", "Compact scannable index: one line per room with counts, date span, key entities, and drawer ids — decide WHERE to look, then get_drawer.",
            json!({ "wing": s("scope to wing") }), &[]),
        // --- knowledge graph ---
        tool("mnemosyne_kg_add", "Add a temporal fact (subject, predicate, object).",
            json!({ "subject": s("entity"), "predicate": s("relation"), "object": s("value"),
                    "valid_from": s("ISO date fact starts"), "valid_to": s("ISO date fact ends"),
                    "confidence": n("0..1") }),
            &["subject", "predicate", "object"]),
        tool("mnemosyne_kg_query", "Facts about an entity, optionally as of an instant.",
            json!({ "entity": s("entity"), "as_of": s("ISO instant"), "direction": s("outgoing|incoming|both") }),
            &["entity"]),
        tool("mnemosyne_kg_invalidate", "Close the validity window of matching active facts.",
            json!({ "subject": s("entity"), "predicate": s("relation"), "object": s("only this value"), "ended": s("ISO end") }),
            &["subject", "predicate"]),
        tool("mnemosyne_kg_supersede", "Replace the current value of (subject, predicate).",
            json!({ "subject": s("entity"), "predicate": s("relation"), "new_object": s("new value"), "changed_at": s("ISO instant") }),
            &["subject", "predicate", "new_object"]),
        tool("mnemosyne_kg_timeline", "Fact history, optionally for one entity.",
            json!({ "entity": s("entity") }), &[]),
        tool("mnemosyne_kg_stats", "Knowledge-graph counts.", json!({}), &[]),
        // --- agent diaries ---
        tool("mnemosyne_diary_write", "Append a diary entry for an agent.",
            json!({ "agent": s("agent name"), "entry": s("diary text") }), &["agent", "entry"]),
        tool("mnemosyne_diary_read", "Read an agent's recent diary entries.",
            json!({ "agent": s("agent name"), "limit": i("max entries") }), &["agent"]),
        tool("mnemosyne_list_agents", "Agents that have diaries.", json!({}), &[]),
        // --- maintenance ---
        tool("mnemosyne_dedup", "Report (or remove) exact-duplicate drawers.",
            json!({ "apply": { "type": "boolean", "description": "actually delete duplicates" } }), &[]),
    ])
}

fn call_tool(store: &mut PalaceStore, name: &str, args: &Value) -> Result<String> {
    match name {
        "mnemosyne_save" => {
            let content = args
                .get("content")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("missing required argument: content"))?;
            let wing = args
                .get("wing")
                .and_then(Value::as_str)
                .unwrap_or("general");
            let room = args.get("room").and_then(Value::as_str).unwrap_or("inbox");
            mnemosyne_core::validate_name(wing, "wing")?;
            mnemosyne_core::validate_name(room, "room")?;
            let normalized = normalize_content(content);
            if normalized.is_empty() {
                anyhow::bail!("content is empty after normalization");
            }
            let idx = store.count()? as u32;
            let drawer = Drawer::new(wing, room, normalized, None, idx, "mcp");
            store.upsert(&drawer)?;
            Ok(format!("saved drawer {} in {}/{}", drawer.id, wing, room))
        }
        "mnemosyne_search" => {
            let query = args
                .get("query")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("missing required argument: query"))?;
            let wing = opt_str(args, "wing").map(str::to_string);
            let room = opt_str(args, "room").map(str::to_string);
            let limit = opt_u64(args, "limit").unwrap_or(5) as usize;
            let hits = store.search(query, &SearchOptions { wing, room, limit })?;
            if hits.is_empty() {
                return Ok("no memories matched".into());
            }
            let mut out = String::new();
            for (i, h) in hits.iter().enumerate() {
                out.push_str(&format!(
                    "{}. [score {:.3}] ({}/{}, filed {})\n{}\n\n",
                    i + 1,
                    h.score,
                    h.drawer.meta.wing,
                    h.drawer.meta.room,
                    h.drawer.meta.filed_at,
                    h.drawer.content
                ));
            }
            Ok(out.trim_end().to_string())
        }
        "mnemosyne_wake_up" => {
            let wing = args.get("wing").and_then(Value::as_str);
            let recent = store.recent(wing, 15)?;
            if recent.is_empty() {
                return Ok("palace is empty".into());
            }
            let mut out = String::from("recent essential memories:\n");
            for d in recent {
                let line = d.content.lines().next().unwrap_or("");
                out.push_str(&format!("- [{}/{}] {}\n", d.meta.wing, d.meta.room, line));
            }
            Ok(out.trim_end().to_string())
        }
        "mnemosyne_verify" => {
            let report = store.verify()?;
            Ok(format!(
                "records checked: {}\nhmac failures: {}\naudit chain: {}\nresult: {}",
                report.records_checked,
                report.bad_records.len(),
                if report.chain_ok { "ok" } else { "BROKEN" },
                if report.ok() {
                    "VERIFY OK"
                } else {
                    "VERIFY FAILED"
                }
            ))
        }
        "mnemosyne_status" => {
            let st = store.stats()?;
            Ok(serde_json::to_string_pretty(&st)?)
        }
        "mnemosyne_get_drawer" => {
            let id = req_str(args, "id")?;
            match store.get(id)? {
                Some(d) => Ok(serde_json::to_string_pretty(&d)?),
                None => anyhow::bail!("no drawer with id {id}"),
            }
        }
        "mnemosyne_add_drawer" => {
            let content = req_str(args, "content")?;
            let wing = opt_str(args, "wing").unwrap_or("general");
            let room = opt_str(args, "room").unwrap_or("inbox");
            mnemosyne_core::validate_name(wing, "wing")?;
            mnemosyne_core::validate_name(room, "room")?;
            let normalized = normalize_content(content);
            if normalized.is_empty() {
                anyhow::bail!("content is empty after normalization");
            }
            let idx = store.count()? as u32;
            let drawer = Drawer::new(
                wing,
                room,
                normalized,
                opt_str(args, "source_file").map(str::to_string),
                idx,
                "mcp",
            );
            store.upsert(&drawer)?;
            Ok(format!("added drawer {} in {}/{}", drawer.id, wing, room))
        }
        "mnemosyne_update_drawer" => {
            let id = req_str(args, "id")?;
            if store.update_drawer(id, req_str(args, "content")?)? {
                Ok(format!("updated drawer {id}"))
            } else {
                anyhow::bail!("no drawer with id {id}")
            }
        }
        "mnemosyne_delete_drawer" => {
            let id = req_str(args, "id")?;
            if store.delete_drawer(id)? {
                Ok(format!("deleted drawer {id}"))
            } else {
                anyhow::bail!("no drawer with id {id}")
            }
        }
        "mnemosyne_list_drawers" => {
            let rows = store.list_drawers(
                opt_str(args, "wing"),
                opt_str(args, "room"),
                opt_u64(args, "limit").unwrap_or(20) as usize,
                opt_u64(args, "offset").unwrap_or(0) as usize,
            )?;
            Ok(serde_json::to_string_pretty(&rows)?)
        }
        "mnemosyne_delete_by_source" => {
            let n = store.delete_by_source(req_str(args, "source_file")?)?;
            Ok(format!("deleted {n} drawer(s)"))
        }
        "mnemosyne_check_duplicate" => {
            match store.check_duplicate(&normalize_content(req_str(args, "content")?))? {
                Some(id) => Ok(format!("duplicate of {id}")),
                None => Ok("not filed".into()),
            }
        }
        "mnemosyne_list_wings" => {
            let wings = store.wings()?;
            Ok(serde_json::to_string_pretty(&wings)?)
        }
        "mnemosyne_list_rooms" => {
            let rooms = store.rooms(req_str(args, "wing")?)?;
            Ok(serde_json::to_string_pretty(&rooms)?)
        }
        "mnemosyne_get_taxonomy" => {
            let tax = store.taxonomy()?;
            Ok(serde_json::to_string_pretty(&tax)?)
        }
        "mnemosyne_create_tunnel" => {
            let id = store.create_tunnel(
                req_str(args, "from_wing")?,
                req_str(args, "to_wing")?,
                opt_str(args, "label").unwrap_or("related"),
            )?;
            Ok(format!("tunnel {id} created"))
        }
        "mnemosyne_list_tunnels" => {
            let t = store.list_tunnels(opt_str(args, "wing"))?;
            Ok(serde_json::to_string_pretty(&t)?)
        }
        "mnemosyne_follow_tunnel" => {
            let drawers = store.follow_tunnel(
                req_str(args, "id")?,
                opt_u64(args, "limit").unwrap_or(5) as usize,
            )?;
            Ok(serde_json::to_string_pretty(&drawers)?)
        }
        "mnemosyne_delete_tunnel" => {
            let id = req_str(args, "id")?;
            if store.delete_tunnel(id)? {
                Ok(format!("deleted tunnel {id}"))
            } else {
                anyhow::bail!("no tunnel with id {id}")
            }
        }
        "mnemosyne_traverse" => {
            let reach = store.traverse(
                req_str(args, "start")?,
                opt_u64(args, "depth").unwrap_or(3) as usize,
            )?;
            Ok(serde_json::to_string_pretty(&reach)?)
        }
        "mnemosyne_get_closet_index" => {
            let lines = store.closet_index(opt_str(args, "wing"))?;
            if lines.is_empty() {
                return Ok("palace is empty".into());
            }
            Ok(lines.join("\n"))
        }
        "mnemosyne_list_hallways" => {
            let halls = store.hallways(
                req_str(args, "wing")?,
                opt_u64(args, "top").unwrap_or(20) as usize,
            )?;
            Ok(serde_json::to_string_pretty(&halls)?)
        }
        "mnemosyne_kg_add" => {
            let id = store.kg_add(
                req_str(args, "subject")?,
                req_str(args, "predicate")?,
                req_str(args, "object")?,
                opt_str(args, "valid_from"),
                opt_str(args, "valid_to"),
                args.get("confidence")
                    .and_then(Value::as_f64)
                    .unwrap_or(1.0),
                None,
            )?;
            Ok(format!("fact {id} added"))
        }
        "mnemosyne_kg_query" => {
            let facts = store.kg_query_entity(
                req_str(args, "entity")?,
                opt_str(args, "as_of"),
                opt_str(args, "direction").unwrap_or("outgoing"),
            )?;
            Ok(serde_json::to_string_pretty(&facts)?)
        }
        "mnemosyne_kg_invalidate" => {
            let n = store.kg_invalidate(
                req_str(args, "subject")?,
                req_str(args, "predicate")?,
                opt_str(args, "object"),
                opt_str(args, "ended"),
            )?;
            Ok(format!("invalidated {n} fact(s)"))
        }
        "mnemosyne_kg_supersede" => {
            let id = store.kg_supersede(
                req_str(args, "subject")?,
                req_str(args, "predicate")?,
                req_str(args, "new_object")?,
                opt_str(args, "changed_at"),
            )?;
            Ok(format!("superseded; new fact {id}"))
        }
        "mnemosyne_kg_timeline" => {
            let tl = store.kg_timeline(opt_str(args, "entity"))?;
            Ok(serde_json::to_string_pretty(&tl)?)
        }
        "mnemosyne_kg_stats" => {
            let st = store.kg_stats()?;
            Ok(serde_json::to_string_pretty(&st)?)
        }
        "mnemosyne_diary_write" => {
            let id = store.diary_write(req_str(args, "agent")?, req_str(args, "entry")?)?;
            Ok(format!("diary entry {id} written"))
        }
        "mnemosyne_diary_read" => {
            let entries = store.diary_read(
                req_str(args, "agent")?,
                opt_u64(args, "limit").unwrap_or(10) as usize,
            )?;
            Ok(serde_json::to_string_pretty(&entries)?)
        }
        "mnemosyne_list_agents" => {
            let agents = store.list_agents()?;
            Ok(serde_json::to_string_pretty(&agents)?)
        }
        "mnemosyne_dedup" => {
            let apply = args.get("apply").and_then(Value::as_bool).unwrap_or(false);
            let report = store.dedup(apply)?;
            Ok(serde_json::to_string_pretty(&report)?)
        }
        other => anyhow::bail!("unknown tool: {other}"),
    }
}

fn req_str<'a>(args: &'a Value, key: &str) -> anyhow::Result<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("missing required argument: {key}"))
}

fn opt_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

fn opt_u64(args: &Value, key: &str) -> Option<u64> {
    args.get(key).and_then(Value::as_u64)
}
