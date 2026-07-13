# Mnemosyne — hardened local-first AI memory (Rust).
#
# Multi-stage build:
#   * builder — compiles the workspace with the full test toolchain
#   * test    — runs unit + integration tests (docker build --target test)
#   * runtime — minimal image with just the `mnemosyne` binary
#
# Everything persists under /data (palace: vaults, keys, identity), so
# mount a volume there:
#
#   docker build -t mnemosyne .
#   docker run --rm -v mnemosyne-data:/data mnemosyne init
#   docker run --rm -v mnemosyne-data:/data mnemosyne remember "hello"
#   docker run -i  --rm -v mnemosyne-data:/data mnemosyne serve-mcp   # MCP stdio

FROM rust:1.90-slim-bookworm AS builder
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
# Default members only — the onnx embedder crate is built by the
# dedicated `onnx-build` compose service.
RUN cargo build --release && cargo test --release --no-run

FROM builder AS test
CMD ["cargo", "test", "--release"]

FROM debian:bookworm-slim AS runtime
RUN useradd --create-home --uid 10001 mnemosyne \
    && mkdir -p /data && chown mnemosyne:mnemosyne /data
COPY --from=builder /src/target/release/mnemosyne /usr/local/bin/mnemosyne
USER mnemosyne
ENV MNEMOSYNE_HOME=/data
VOLUME ["/data"]
ENTRYPOINT ["mnemosyne"]
CMD ["--help"]
