# MCP setup

## Claude Code

```bash
claude mcp add mnemosyne -- mnemosyne serve-mcp
```

Docker variant:

```bash
claude mcp add mnemosyne -- docker run -i --rm -v mnemosyne-data:/data mnemosyne serve-mcp
```

## Any stdio MCP client

```json
{ "mcpServers": { "mnemosyne": { "command": "mnemosyne", "args": ["serve-mcp"] } } }
```

## Remote (HTTP) server

See [docs/remote-server.md](../docs/remote-server.md).

32 tools are exposed; ask the client to list them, or see the README table.
