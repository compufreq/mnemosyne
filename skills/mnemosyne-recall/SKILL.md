---
name: mnemosyne-recall
description: Recall memories from the Mnemosyne palace. Use at session start (wake-up) or when the user references past conversations, decisions, or vague "remember when..." questions.
---

# Recalling memories

- Session start: run `mnemosyne wake-up` and load L0 identity + L1 recent
  essential memories as context.
- Vague or semantic questions: `mnemosyne search "<question>"` — hybrid
  semantic + lexical + recency retrieval; scope with `--wing`/`--room` when
  the project is known.
- Time-anchored facts ("where did X work in 2024?"):
  `mnemosyne kg query <entity> --as-of <date>` or `mnemosyne kg timeline --entity <entity>`.
- Quote retrieved memories verbatim; they are the user's exact words.
- If a search errors with an integrity failure, STOP and tell the user to
  run `mnemosyne verify` — never present tampered data.
