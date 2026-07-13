---
name: mnemosyne
description: Store memories in the Mnemosyne palace. Use when the user asks to remember, save, or file information for later sessions, or says "remember this".
---

# Saving memories

Use the `mnemosyne` CLI (or the `mnemosyne_save` MCP tool when connected).

- Store the user's exact words — never summarize or paraphrase on save.
- Pick a wing per person/project and a room per topic:
  `mnemosyne remember "<verbatim text>" --wing <project> --room <topic>`
- For facts with a time dimension ("X works at Y since March"), also add a
  knowledge-graph triple: `mnemosyne kg add <subject> <predicate> <object> --from <date>`
- Confirm to the user what was filed and where (wing/room + drawer id).
