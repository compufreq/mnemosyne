# Tamper runbook (operator quick-reference)

Full version, published: **https://compufreq.github.io/mnemosyne/docs/runbook.html**
(this is what the `PalaceTamperDetected` alert's `runbook_url` links to).

`PalaceTamperDetected` fired (or `mnemosyne verify` shows `hmac failures > 0`, or
the Palace Monitor beacon lit) → a stored record failed its HMAC on read. Treat
as on-disk tampering until proven otherwise.

**1. Where** — the alert's `surface` label (`drawer`/`kg`/`tunnel`/`manifest`)
and `vault` label localize it. Grafana “Tamper by surface” + Logs panels show
the same.

**2. Confirm** — name the exact record:
```bash
mnemosyne verify <vault>      # -> "TAMPERED: <id>", "audit chain: BROKEN"
```

**3. Mitigate** — freeze writes and preserve evidence before touching anything:
```bash
mnemosyne serve-http --read-only …
cp -a "$MNEMOSYNE_HOME/vaults/<vault>" "/tmp/<vault>.evidence.$(date +%s)"
```

**4. Fix** — verbatim restore from a known-good backup, then re-verify:
```bash
mnemosyne backup list
mnemosyne backup restore <vault> <backup-id>
mnemosyne verify <vault>      # must be 0 failures, chain ok
mnemosyne repair <vault>      # backfill fingerprints, vacuum, re-verify
```

**5. Prevent** — scheduled `backup`s, `0600` on the vault dir + `master.key`,
OS file-integrity monitoring on the vault dir, keep alerting on, per-vault
assertions for multi-tenant.

The alarm only ever fires on a real HMAC-verify failure — there are no synthetic
tamper alarms anywhere in the system.
