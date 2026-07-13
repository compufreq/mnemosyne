# Architecture

## The palace

```
Palace (data dir, one master key)
└── Vaults (isolation boundary: own DB file, own derived keys)
    ├── Wings   (people / projects)          ── connected by Tunnels
    │   └── Rooms (topics)
    │       └── Drawers (verbatim chunks, ~800 chars)
    ├── Knowledge graph (temporal triples with validity windows)
    ├── Audit chain (append-only, HMAC-chained writes)
    └── Hallways (entity co-occurrence, computed on demand — never persisted)
```

## Crates

| Crate | Responsibility |
|---|---|
| `mnemosyne-core` | Domain types, chunking, deterministic ids, normalization, hash embedder, transcript parsing, entity detection |
| `mnemosyne-vault` | Master key (file or Argon2id), HKDF per-vault keys, XChaCha20-Poly1305 sealing, HMAC tags, audit chain, MAC'd manifests |
| `mnemosyne-store` | Per-vault SQLite (system of record), hybrid search, knowledge graph, management surface, remote-index integration |
| `mnemosyne-index` | Qdrant / Chroma / pgvector clients — untrusted accelerators |
| `mnemosyne-embed-onnx` | Feature-gated ONNX sentence embedder (tract) |
| `mnemosyne-cli` | `mnemosyne` binary: CLI + MCP stdio + MCP HTTP server |
| `mnemosyne-bench` | LongMemEval harness + synthetic regression benchmark |

## Write path

remember/mine/sweep → normalize (verbatim-preserving) → chunk → deterministic
id → embed → seal content+embedding (sealed vaults) → HMAC tag over
id/meta/content → SQLite row + audit append → manifest chain head advances.

## Read path

search → embed query → candidates (local scan, FTS-free by design in sealed
vaults; or remote ANN index) → HMAC verify every candidate → decrypt →
hybrid re-rank (semantic + lexical + recency) → relevance gate.
