# Gemini CLI setup

Add to `~/.gemini/settings.json`:

```json
{
  "mcpServers": {
    "mnemosyne": { "command": "mnemosyne", "args": ["serve-mcp"] }
  }
}
```

Gemini CLI will discover the `mnemosyne_*` tools (save, search, wake_up,
kg_*, diary_*, …). Start sessions by calling `mnemosyne_wake_up`, and store
decisions verbatim with `mnemosyne_save`.

For automatic transcript capture, run the sweep daemon against Gemini CLI's
session directory:

```bash
mnemosyne daemon run --watch ~/.gemini/tmp --interval 300 --wing gemini
```
