# Deploying Mnemosyne

- `docker-compose.server.yml` — shared team memory server: MCP over HTTP
  (bearer-token auth) backed by Qdrant. Content is sealed client-side inside
  the mnemosyne container before it ever reaches Qdrant.
- `mnemosyne-server.service` — the same server as a hardened systemd unit.
- `mnemosyne-daemon.service` — per-user auto-save daemon (periodic
  `mnemosyne daemon run` sweep of `~/.claude/projects`).
- `server.env.example` — environment template; copy to `.env` / 
  `/etc/mnemosyne/server.env` and set the bearer token.

The server refuses a non-loopback bind without `MNEMOSYNE_MCP_HTTP_TOKEN`.
Use `--read-only` to expose recall without write access. Always terminate
TLS in front of it for anything beyond a trusted private network.
