# Observability

Mnemosyne ships an **opt-in** observability layer: structured logs, a
Prometheus `/metrics` endpoint, and OpenTelemetry (OTLP) trace/metric
export. It is built to preserve the project's stance:

- **Off by default.** A standard build carries none of the telemetry
  dependencies and no runtime overhead — the layer only exists when you
  compile with `--features telemetry`.
- **Local-first / no phone-home.** Nothing leaves the process unless you
  explicitly point it somewhere: `/metrics` is served only when you ask,
  and OTLP export happens only when `MNEMOSYNE_OTLP_ENDPOINT` is set.
- **Metadata only.** Every signal is a count, a rate, a latency, or an
  aggregate gauge. Drawer content, drawer names beyond what `stats`
  already exposes, and key material are **never** emitted. Sealed vaults
  expose only aggregate counts.

## Building with telemetry

```bash
cargo build -p mnemosyne-cli --release --features telemetry
```

Without the feature the same binary runs identically, and hitting
`/metrics` (if enabled) returns `503` with a hint to rebuild.

## Structured logs

With the feature on, diagnostics become `tracing` events.

| Variable | Default | Meaning |
|---|---|---|
| `MNEMOSYNE_LOG` | `warn,mnemosyne=info` | `EnvFilter` directives |
| `MNEMOSYNE_LOG_FORMAT` | `text` | `json` for machine-readable logs |

## Prometheus metrics

```bash
MNEMOSYNE_METRICS=1 mnemosyne serve-http --host 127.0.0.1 --port 8765
curl -H "Authorization: Bearer $MNEMOSYNE_MCP_HTTP_TOKEN" \
     http://127.0.0.1:8765/metrics
```

`/metrics` is **opt-in** (`MNEMOSYNE_METRICS=1`), served on the bind
address (loopback unless you deliberately expose the server), and sits
**behind the same bearer token** as the rest of the server. It is absent
(`404`) when the flag is unset.

Exposed series (all `mnemosyne_*`):

- **Counters** — `search_total{fusion}`, `search_prefiltered_total`,
  `drawer_writes_total{outcome}`, `drawer_deletes_total`,
  `kg_writes_total{kind}`, `chain_commits_total`,
  `hmac_verify_failures_total{surface}`, `vault_opens_total`,
  `http_requests_total{route,status}`, `auth_rejections_total{kind}`.
- **Histograms** — `search_duration_seconds`, `search_hits`,
  `http_request_duration_seconds{route}`.
- **Gauges** (per vault) — `drawers`, `audit_chain_height`, plus
  `kg_triples` / `kg_entities` / `store_bytes` where sampled.

`hmac_verify_failures_total` is the headline signal: any non-zero value
means a record, KG triple, tunnel, or vault manifest failed HMAC
verification — i.e. tamper was detected on read.

## OpenTelemetry (OTLP)

Set an endpoint to export traces and metrics over OTLP/HTTP:

```bash
MNEMOSYNE_OTLP_ENDPOINT=http://localhost:4318 \
MNEMOSYNE_SERVICE_NAME=mnemosyne \
mnemosyne serve-http
```

| Variable | Meaning |
|---|---|
| `MNEMOSYNE_OTLP_ENDPOINT` | OTLP/HTTP collector base URL. **Unset ⇒ no network egress.** |
| `MNEMOSYNE_SERVICE_NAME` | `service.name` resource attribute (default `mnemosyne`). |
| `MNEMOSYNE_OTLP_HEADERS` | Optional headers for the exporter. |

Spans cover the hot paths (search, save/dedup, KG writes, vault
seal/commit). Export is synchronous and thread-based — the server itself
stays fully synchronous, with no async runtime introduced.

## Grafana dashboard

A ready-to-run stack lives in `deploy/observability/` — a telemetry-built
Mnemosyne server, Prometheus scraping its `/metrics`, and Grafana with a
pre-provisioned dashboard:

```bash
cd deploy/observability
docker compose -f docker-compose.observability.yml up --build
# Grafana → http://localhost:3000  (dashboard: "Mnemosyne — Palace")
```

The dashboard surfaces request rate by route, search rate and p95/p50
latency, drawer writes (created vs deduped), audit-chain commit rate, and
an **HMAC-verify-failures** stat panel that turns red the instant tamper
is detected. See `deploy/observability/README.md`.

## Live stream (SSE)

Prometheus is pull-based; for a **live** view the multi-tenant server also
pushes an [SSE](https://developer.mozilla.org/docs/Web/API/Server-sent_events)
stream per vault — a periodic sample of aggregate counts plus discrete
event pings as they happen. This is what the forthcoming Palace Monitor
UI consumes. Telemetry build + bearer required; sealed vaults stream only
aggregates (wing/room names suppressed).

```bash
# live event stream (Ctrl-C to stop)
curl -N -H "Authorization: Bearer $TOKEN" \
     http://127.0.0.1:8765/v1/vaults/<id>/stream

# recent samples for backfill
curl -H "Authorization: Bearer $TOKEN" \
     "http://127.0.0.1:8765/v1/vaults/<id>/stats/history?window=100"
```

Frames:

- `event: sample` — `{ts, drawers, rooms, wings, kg_triples, kg_entities,
  kg_active, tunnels, chain_height, db_bytes, sealed}`. Emitted on the
  sampler tick (default 2s, `MNEMOSYNE_SAMPLE_INTERVAL_MS`), and only for
  vaults with an active subscriber.
- `event: drawer-saved` / `drawer-deleted` / `search` / `kg-triple` /
  `chain-commit` — discrete pings carrying vault + (for hmac-only vaults)
  wing/room. A comment heartbeat (`: ping`) every 15s keeps the
  connection detectably alive.

Each connection is served on its own thread (the request is handed off so
the single-threaded server keeps serving), reading only from an in-process
broker — never a vault store — so streaming can never touch content.
