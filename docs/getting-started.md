# Getting started

> Implementing with (or as) an AI agent? The
> [agents implementation guide](https://compufreq.github.io/mnemosyne/docs/agents.html)
> is the scenario-driven version of this page: pick a deployment shape
> (single agent, team server, multi-tenant engine, fleet), follow its
> steps, and verify with the checklist.

## Install

Docker (recommended — nothing touches the host):

```bash
docker build -t mnemosyne .
alias mnemosyne='docker run --rm -v mnemosyne-data:/data mnemosyne'
```

Or native: `cargo build --release` → `target/release/mnemosyne`.

## First palace

```bash
mnemosyne init                                   # master key + sealed 'default' vault
mnemosyne remember "We chose GraphQL for the mobile API" --wing backend --room decisions
mnemosyne mine ~/notes --wing personal           # documents
mnemosyne mine ~/.claude/projects --mode convos  # Claude Code sessions
mnemosyne search "why graphql"
mnemosyne wake-up                                # session-start context
mnemosyne verify                                 # HMAC + audit chain check
```

Palace location: `$MNEMOSYNE_HOME` (default `~/.mnemosyne`). Passphrase
mode: export `MNEMOSYNE_PASSPHRASE` before `init` and every command.

## Wire into Claude Code

```bash
claude mcp add mnemosyne -- mnemosyne serve-mcp
mnemosyne hooks claude-code   # auto-save hook settings to paste
```

Continue with [integrations](integrations.md), [architecture](architecture.md),
[security model](security.md), and [remote team server](remote-server.md).
