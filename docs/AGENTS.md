# Agents implementation guide

**Audience: an AI agent (or the human pairing with one) that needs to give
itself — or a product it is building — a hardened, local-first memory.**
This document is scenario-driven: find the scenario that matches your
situation, follow its steps verbatim, then verify with the checklist at the
end. Everything here is the real surface of the current release — tool
names, routes, and environment variables are copied from the code, not
paraphrased.

Links are absolute so this page reads correctly anywhere:
repository <https://github.com/compufreq/mnemosyne>, rendered docs
<https://compufreq.github.io/mnemosyne/docs/>.

---

## 0. Ground rules (invariants you must not violate)

Mnemosyne stores memories **verbatim** in *drawers*, filed into
*wings/rooms*, inside isolated *vaults* (own SQLite database, own
HKDF-derived keys). When you build on it:

1. **Never summarize, paraphrase, or compress content on the write path.**
   Store the exact words; retrieval returns the exact words. Summarize at
   read time in your own context if you must.
2. **Local-first, zero external calls by default.** The default embedder is
   deterministic and offline. Never add a phone-home. Telemetry exists but
   is opt-in at build time and metadata-only.
3. **Sealed vaults keep nothing plaintext-derived on disk.** Do not write
   sidecar files, caches, or logs containing drawer content next to a
   sealed vault.
4. **Drawer ids are deterministic** over (wing, room, source, chunk_index).
   Re-ingesting the same source is idempotent — rely on that instead of
   inventing your own dedup on top.
5. **Integrity is enforced, not assumed.** Every read verifies an HMAC;
   every write advances a tamper-evident audit chain in the same
   transaction. If `verify` fails, treat it as an incident (see the
   [tamper runbook](https://compufreq.github.io/mnemosyne/docs/runbook.html)),
   not as noise.
6. **Names are validated.** Vault/wing/room names go through a
   path-traversal guard — expect errors on `../`-style input rather than
   trying to sanitize yourself.

---

## 1. Choose your scenario

| Your situation | Scenario | Deployment shape |
|---|---|---|
| One agent, one machine, persistent memory across sessions | **A** | CLI + MCP stdio server |
| Several agents / teammates sharing one memory | **B** | `serve-http` with a bearer token |
| Your product needs per-customer isolated memory | **C** | Multi-tenant `/v1` REST engine |
| Fleets of engines, tenants placed/migrated between them | **D** | The `mnemosyne-orchestrator` control plane |
| You need better recall or lower latency than defaults | **E** | Retrieval/model tier selection |
| You operate any of the above | **F** | Security operations (verify/rotate/backup/bundles) |
| You need dashboards/alerts | **G** | Opt-in telemetry build |

All scenarios start the same way:

```bash
git clone https://github.com/compufreq/mnemosyne && cd mnemosyne
docker build -t mnemosyne .        # or: cargo build --release
mnemosyne init                     # palace at ~/.mnemosyne (override: MNEMOSYNE_HOME)
```

`init` creates the master key (`master.key`, 0600 — or derive it from
`MNEMOSYNE_PASSPHRASE` instead) and a `default` vault at the `sealed`
level. Use `--level hmac-only` only when you explicitly want a
plaintext-inspectable database with integrity tags.

---

## 2. Scenario A — a single agent that remembers

The shape: your agent runs the MCP stdio server as a subprocess and uses
its tools; hooks auto-save the session transcript so nothing is lost even
when the agent forgets to save.

**A1. Register the MCP server** (Claude Code `.mcp.json`, Claude Desktop
`claude_desktop_config.json`, or any MCP client):

```json
{ "mcpServers": { "mnemosyne": { "command": "mnemosyne", "args": ["serve-mcp"] } } }
```

Add `"--vault", "work"` to scope the server to a non-default vault, and
set `MNEMOSYNE_HOME` in the server's env if the palace lives elsewhere.

**A2. Install the auto-save hook** (Claude Code):

```bash
mnemosyne hooks claude-code
```

This prints a `settings.json` fragment wiring **Stop** and **PreCompact**
events to `mnemosyne sweep ~/.claude/projects --wing claude-code` — one
verbatim drawer per prose message, idempotent, so re-sweeps are no-ops.

**A3. Use the tools.** Session start: call `mnemosyne_wake_up` (recent
essential memories; the CLI `wake-up` additionally prints an L0 identity
section from `<data-dir>/identity.txt` — create that file to give the
agent a durable self-description). During work: `mnemosyne_save` for
decisions worth keeping, `mnemosyne_search` before re-deriving anything,
`mnemosyne_kg_add`/`mnemosyne_kg_query` for temporal facts ("alice
works_at acme *since* 2024-01"). The full 32-tool surface is in §8.

**A4. Bulk history**: `mnemosyne mine <dir>` chunks documents;
`mnemosyne mine <dir> --mode convos` and `mnemosyne sweep <dir>` ingest
agent transcripts; `mnemosyne daemon run --watch <dir>` keeps sweeping in
the background. Ingest is batched — hundreds of drawers commit as single
transactions.

---

## 3. Scenario B — a shared team memory

One `serve-http` process serves both MCP-over-HTTP (`POST /mcp`) and the
REST surface. Auth is layered:

```bash
export MNEMOSYNE_MCP_HTTP_TOKEN=$(openssl rand -hex 24)   # palace bearer
mnemosyne serve-http --host 0.0.0.0 --port 8800
```

- The server **refuses to start** on a non-loopback bind without the
  bearer. Every request (MCP and `/v1`) must send
  `Authorization: Bearer <token>`.
- `--read-only` strips all 12 mutating MCP tools and returns 403 on
  mutating `/v1` routes — run a second read-only instance for consumers
  that should never write.
- `GET /healthz` is the only unauthenticated route.
- Put TLS in front with a reverse proxy; the server itself speaks HTTP.

Point every teammate's MCP client at it, or use the REST routes in §9
directly.

---

## 4. Scenario C — a multi-tenant memory engine inside your product

Give each customer their own vault, and require a **per-vault assertion**
on every request so holding the palace bearer alone is not enough:

```bash
export MNEMOSYNE_MCP_HTTP_TOKEN=...        # reaching the server
export MNEMOSYNE_ASSERTION_SECRET=...      # addressing a tenant
mnemosyne serve-http --host 0.0.0.0 --port 8800
```

Every `/v1` request must then carry
`X-Vault-Assertion: <unix-ts>:<hex HMAC-SHA256(secret, "<ts>|<vault_id>")>`
for the exact vault it addresses (±120 s window; the vault id is inside
the MAC, so an assertion for tenant A can never address tenant B). Mint
one for testing with `mnemosyne assert-header <vault>`.

Per-tenant flow (full route table in §9):

```text
POST   /v1/vaults                      {"id":"acme","level":"sealed"}       # create
POST   /v1/vaults/acme/drawers         {"text":"...","wing":"notes"}        # save
POST   /v1/vaults/acme/search          {"query":"...","limit":8}            # search
GET    /v1/vaults/acme/export                                              # lossless NDJSON
POST   /v1/vaults/acme/import                                              # count-verified restore
```

Two options worth knowing:

- **External embeddings**: create the vault with
  `"embedder":"external:<name>@<dim>"` and supply a `vector` with every
  save and search — your product's embedding model, mnemosyne's sealing
  and integrity. Dimension is enforced exactly.
- **Dedup-refresh**: pass `"dedup_threshold":0.9` on save to refresh a
  near-duplicate in place (audited update) instead of piling up copies.

Export lines carry vectors and ColBERT token artifacts, so
export→import is a **lossless migration primitive** — restore is a copy,
not a re-embed.

---

## 5. Scenario D — a fleet with the orchestrator

When one engine is not enough, `mnemosyne-orchestrator` (separate binary,
same repo) is the control plane: instance registry, tenant→vault mapping,
token minting, routing, and live migration. It is a pure client of `/v1` —
engines never know it exists. Full docs:
[MULTI_TENANCY.md](https://github.com/compufreq/mnemosyne/blob/main/docs/MULTI_TENANCY.md).

```bash
export MNEMOSYNE_ORCH_KEY=$(mnemosyne-orchestrator keygen)   # seals engine creds
export MNEMOSYNE_ORCH_ADMIN_TOKEN=...                        # /admin bearer (≥16 chars)
mnemosyne-orchestrator serve                                 # 127.0.0.1:8900 (MNEMOSYNE_ORCH_ADDR)

# register engines, create tenants (token shown ONCE), migrate:
mnemosyne-orchestrator instance-add engine-a http://a:8800 <bearer> <assertion-secret>
mnemosyne-orchestrator tenant-create acme
mnemosyne-orchestrator migrate acme engine-b     # export→import→count-verify→flip→delete
```

Tenants call `/t/<subpath>` with their own bearer; the orchestrator
resolves the token (stored only as an HMAC), forwards to
`/v1/vaults/{their-vault}/<subpath>` with the engine bearer + a fresh
assertion. The subpath allowlist is `drawers | search | stats | export |
import` — vault lifecycle is deliberately unreachable with a tenant token.
Optional per-tenant rate limiting: `MNEMOSYNE_ORCH_RATE_LIMIT=<req/min>`.
Rotate a tenant token with `tenant-rotate` (the old one dies in the same
statement). Deploy TLS on both hops; back up the orchestrator's SQLite.

---

## 6. Scenario E — choosing retrieval quality and latency

Everything composes through environment variables; identity is recorded
per vault on first write, and a model swap is refused unless you set
`MNEMOSYNE_FORCE_EMBEDDER=1` and re-embed with `mnemosyne repair`.

**Embedder tiers** (`MNEMOSYNE_EMBEDDER`):

| Value | What | When |
|---|---|---|
| `hash` (default) | deterministic hashed n-grams, offline, zero deps | correct default; measured LoCoMo R@10 92.7% with hybrid search |
| `onnx` | user-supplied MiniLM-class ONNX via tract (pure Rust); needs `MNEMOSYNE_ONNX_MODEL`/`_TOKENIZER`, build `--features onnx` | best recall, pure-Rust constraint |
| `ort` | same models via ONNX Runtime (C++ dep, build `--features ort`); ~2.5× faster/forward, int8 support, ~4–5× faster ingest | throughput matters; same env vars, switching is one env change |

**Second stage** (`MNEMOSYNE_RERANKER`): `onnx`/`ort` = cross-encoder
re-scoring of the top `MNEMOSYNE_RERANK_TOP_N` (default 50) — measured
LoCoMo R@10 94.6→97.7%; `colbert`/`colbert-ort` = late interaction: encode
once at ingest, **one** query forward + MaxSim at search — ~96.5–96.8% at
a flat ~93 ms/q (tract) or ~70 ms/q (ort), independent of core count.
Model paths via `MNEMOSYNE_RERANK_*` / `MNEMOSYNE_COLBERT_*`. BERT-family
models only (tract cannot run DeBERTa rerankers).

**Candidate generation** (`MNEMOSYNE_RETRIEVAL`): unset = full scan with
FTS prefilter (fine to ~10⁴ drawers); `pq` = bounded-RAM PQ/IVF prefilter
(recall flat in corpus size, works on sealed vaults via a decrypt-once RAM
cache); `fde` = MUVERA fixed-dimensional encodings for the ColBERT stage —
measured recall identical to fusion at −25% latency, rows PQ-compress 32×.
Export recipes and all measured tables:
[RETRIEVAL_SCALING.md](https://github.com/compufreq/mnemosyne/blob/main/docs/RETRIEVAL_SCALING.md).

**Remote vector DBs** (Qdrant/Chroma/pgvector/Milvus/Weaviate via
`mnemosyne index push` + `search --backend`) are **untrusted
accelerators**: they hold sealed bytes, every candidate is re-verified and
decrypted locally. They pay off only at very large corpora — measure
before adopting. After a key rotation, re-run `index push`.

---

## 7. Scenario F — operating it securely

Daily/CI:

```bash
mnemosyne verify           # HMAC every record + replay the audit chain; exit 2 on failure
mnemosyne backup create    # verified snapshot, keeps last 10
```

- A **crash is never a tamper alarm** (open-time reconciliation
  fast-forwards a lagging manifest anchor); a **rollback or forged record
  always is**. On `VERIFY FAILED`, follow the
  [runbook](https://compufreq.github.io/mnemosyne/docs/runbook.html).
- **Key rotation** — `mnemosyne vault rotate <name>`: fresh derived keys,
  every sealed blob re-encrypted and every tag re-keyed in one
  transaction, crash-safe at any instant. Do it on key-exposure suspicion
  or on schedule. Not while another process serves the vault.
- **Encrypted backups** — a backup file should never exist in plaintext:

```bash
mnemosyne bundle keygen --out ops.key            # prints the shareable recipient once
mnemosyne export --to <recipient> --out palace.bundle
mnemosyne import palace.bundle --identity ops.key
```

- Durability is real: SQLite runs WAL + `synchronous=FULL`, the manifest
  anchor and key files are fsynced — an acknowledged write is on disk.

---

## 8. Reference — MCP tools (32)

Write tools (marked **W**) are refused when the server runs `--read-only`.

| Tool | W | Does |
|---|---|---|
| `mnemosyne_save` | W | save one memory verbatim |
| `mnemosyne_search` | | hybrid semantic+lexical search |
| `mnemosyne_wake_up` | | recent essential memories for session start |
| `mnemosyne_verify` | | verify HMACs + audit chain |
| `mnemosyne_status` | | palace statistics |
| `mnemosyne_get_drawer` | | fetch one drawer verbatim |
| `mnemosyne_add_drawer` | W | file a drawer with explicit wing/room |
| `mnemosyne_update_drawer` | W | replace content in place (re-sealed, audited) |
| `mnemosyne_delete_drawer` | W | delete + tamper-evident tombstone |
| `mnemosyne_list_drawers` | | page drawer summaries |
| `mnemosyne_delete_by_source` | W | delete everything mined from a source |
| `mnemosyne_check_duplicate` | | is this exact content already filed? |
| `mnemosyne_list_wings` / `_list_rooms` / `_get_taxonomy` | | palace shape |
| `mnemosyne_create_tunnel` / `_delete_tunnel` | W | connect/disconnect wings |
| `mnemosyne_list_tunnels` / `_follow_tunnel` / `_traverse` | | navigate tunnels |
| `mnemosyne_list_hallways` | | entity co-occurrence within a wing |
| `mnemosyne_get_closet_index` | | compact LLM-scannable index |
| `mnemosyne_kg_add` / `_kg_invalidate` / `_kg_supersede` | W | temporal facts: assert/close/replace |
| `mnemosyne_kg_query` / `_kg_timeline` / `_kg_stats` | | query facts (incl. `--as-of`) |
| `mnemosyne_diary_write` | W | per-agent diary entry |
| `mnemosyne_diary_read` / `_list_agents` | | read diaries |
| `mnemosyne_dedup` | W | report/remove exact duplicates |

## 9. Reference — HTTP surface

Engine (`serve-http`; bearer always; `X-Vault-Assertion` when
`MNEMOSYNE_ASSERTION_SECRET` is set; mutating routes 403 in read-only):

| Method | Path | Purpose |
|---|---|---|
| GET | `/healthz` | liveness (no auth) |
| POST | `/mcp` | MCP over HTTP |
| POST | `/v1/vaults` | create vault (`level`, optional `embedder`) |
| GET | `/v1/vaults` | list vaults (403 when assertions are enabled) |
| DELETE | `/v1/vaults/{id}` | delete vault |
| GET | `/v1/vaults/{id}/stats` | stats: records, level, writes, chain head |
| POST | `/v1/vaults/{id}/drawers` | save (`text`, `wing`, `room`, opt `vector`, `dedup_threshold`) |
| POST | `/v1/vaults/{id}/search` | search (`query`, `limit`, opt `vector`) |
| DELETE | `/v1/vaults/{id}/drawers/{drawer_id}` | delete drawer |
| GET | `/v1/vaults/{id}/export` | lossless NDJSON (vectors + token artifacts) |
| POST | `/v1/vaults/{id}/import` | parse-before-write import |
| GET | `/metrics`, `/monitor`, `/v1/…/stream` | telemetry builds only |

Orchestrator: tenant data plane `/t/<drawers|search|stats|export|import>`
with the tenant bearer; admin plane `/admin/instances[…]`,
`/admin/tenants[…]` (+ `/rotate`, `/migrate`) with
`MNEMOSYNE_ORCH_ADMIN_TOKEN`.

## 10. Reference — environment variables

Core: `MNEMOSYNE_HOME` (palace dir, default `~/.mnemosyne`) ·
`MNEMOSYNE_PASSPHRASE` (Argon2id master key instead of key file) ·
`MNEMOSYNE_LANG` (CLI language, 9 supported).

Models: `MNEMOSYNE_EMBEDDER` (`hash`|`onnx`|`ort`) ·
`MNEMOSYNE_ONNX_MODEL`/`_TOKENIZER`/`_NAME` ·
`MNEMOSYNE_RERANKER` (`onnx`|`ort`|`colbert`|`colbert-ort`) ·
`MNEMOSYNE_RERANK_MODEL`/`_TOKENIZER`/`_NAME`/`_TOP_N` (50) ·
`MNEMOSYNE_COLBERT_MODEL`/`_QUERY_MODEL`/`_TOKENIZER`/`_NAME` ·
`MNEMOSYNE_ORT_POOL` (session pool, default = cores) ·
`MNEMOSYNE_FORCE_EMBEDDER` (allow identity swap, then `repair`).

Retrieval: `MNEMOSYNE_RETRIEVAL` (`pq`|`fde`|`hnsw`) · `MNEMOSYNE_FUSION`
(`bm25` default |`rrf`|`legacy`) · `MNEMOSYNE_FTS_PREFILTER_MIN` (2048) ·
`MNEMOSYNE_IVF_MIN` (8192) · `MNEMOSYNE_IVF_NPROBE` ·
`MNEMOSYNE_TOK_PQ_MIN` (256) · `MNEMOSYNE_FDE_PQ_MIN` (256) ·
`MNEMOSYNE_FDE_REPS`/`_KSIM`/`_DPROJ`/`_SEED` (first build only, then
persisted per vault) · remote backends:
`MNEMOSYNE_QDRANT_URL`/`_CHROMA_URL`/`_PGVECTOR_DSN`/`_MILVUS_URL`/`_WEAVIATE_URL`.

Server: `MNEMOSYNE_MCP_HTTP_TOKEN` (bearer; mandatory non-loopback) ·
`MNEMOSYNE_ASSERTION_SECRET` (enables per-vault assertions) ·
`MNEMOSYNE_METRICS=1` (+ bearer) · `MNEMOSYNE_SAMPLE_INTERVAL_MS` (2000).

LLM (optional, for `refine`): `MNEMOSYNE_LLM_URL` · `MNEMOSYNE_LLM_MODEL`
(`llama3.2`) · `MNEMOSYNE_LLM_API` (`ollama`|`openai`).

Telemetry builds: `MNEMOSYNE_LOG` · `MNEMOSYNE_LOG_FORMAT` (`json`) ·
`MNEMOSYNE_OTLP_ENDPOINT` (unset ⇒ nothing leaves the process) ·
`MNEMOSYNE_SERVICE_NAME`.

Orchestrator: `MNEMOSYNE_ORCH_DB` · `MNEMOSYNE_ORCH_KEY` (required) ·
`MNEMOSYNE_ORCH_ADMIN_TOKEN` (required, ≥16 chars) ·
`MNEMOSYNE_ORCH_ADDR` (127.0.0.1:8900) · `MNEMOSYNE_ORCH_RATE_LIMIT`
(req/min, 0 = off).

## 11. Verify your implementation

Whatever scenario you built, prove it before calling it done:

```bash
mnemosyne verify                          # exit 0, "VERIFY OK", chain ok
mnemosyne stats                           # records/wings match what you ingested
mnemosyne search "<something you stored>" # returns the exact words
mnemosyne backup create && mnemosyne backup list
```

Server scenarios: `curl -fsS http://host:port/healthz`; a request
**without** the bearer must 401; with assertions enabled, a request signed
for vault A against vault B must 401; `--read-only` must refuse a save.
Orchestrator: a tenant token must reach only its own vault, and
`/t/<anything-not-allowlisted>` must 404. If any of these checks
surprises you, stop and read the matching scenario again — the system is
designed so that the insecure configuration is the one that takes extra
work.
