FROM lukemathwalker/cargo-chef:latest-rust-bookworm AS chef
WORKDIR /app

# --- Plan dependencies ---
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# --- Build ---
FROM chef AS builder

RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    cmake \
    && rm -rf /var/lib/apt/lists/*

COPY --from=planner /app/recipe.json recipe.json

ARG FEATURES=""

# Cache dependency build (rebuilds only when Cargo.toml/lock change)
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    if [ -n "$FEATURES" ]; then \
      cargo chef cook --release --recipe-path recipe.json --features "$FEATURES"; \
    else \
      cargo chef cook --release --recipe-path recipe.json; \
    fi

COPY . .

ARG BINARY

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    if [ -n "$FEATURES" ]; then \
      cargo build --release --bin "$BINARY" --features "$FEATURES"; \
    else \
      cargo build --release --bin "$BINARY"; \
    fi \
    && cp /app/target/release/"$BINARY" /usr/local/bin/app

# --- Runtime ---
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    ca-certificates \
    curl \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /usr/local/bin/app /usr/local/bin/app

ENTRYPOINT ["/usr/local/bin/app"]
