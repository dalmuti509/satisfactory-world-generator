# syntax=docker/dockerfile:1

FROM rust:bookworm AS builder

RUN rustup target add wasm32-unknown-unknown \
    && cargo install trunk --locked

WORKDIR /app
COPY . .

# Ensure map outline exists for the WASM build (extract from upstream release if missing).
RUN OUTLINE='src/world-outline.json'; \
    if [ ! -s "$OUTLINE" ] || [ "$(tr -d '[:space:]' < "$OUTLINE")" = "[]" ]; then \
      curl -fsSL "https://konsl.github.io/satisfactory-world-generator/satisfactory-world-generator_bg.wasm" -o /tmp/outline.wasm; \
      python3 scripts/extract-outline-from-wasm.py /tmp/outline.wasm "$OUTLINE"; \
    fi

RUN trunk build --release --public-url /
RUN cargo build --release -p server

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/dist /var/www
COPY --from=builder /app/target/release/server /usr/local/bin/server

ENV STATIC_DIR=/var/www
ENV LISTEN_ADDR=0.0.0.0:8080
EXPOSE 8080

CMD ["server"]
