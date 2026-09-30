FROM rust:1.98-slim as builder

RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    protobuf-compiler \
    curl \
    && rm -rf /var/lib/apt/lists/*

RUN rustup target add wasm32-unknown-unknown

RUN curl -L https://github.com/streamingfast/substreams/releases/download/v1.23.0/substreams_linux_x86_64.tar.gz | tar -xz -C /usr/local/bin/

WORKDIR /app

COPY proto ./proto
COPY substreams-erc20 ./substreams-erc20
COPY sink ./sink
COPY api ./api
COPY migrations ./migrations

RUN cargo build --release --target wasm32-unknown-unknown --manifest-path substreams-erc20/Cargo.toml

RUN substreams pack substreams-erc20/substreams.yaml -o /app/erc20.spkg

RUN cargo build --release --manifest-path sink/Cargo.toml
RUN cargo build --release --manifest-path api/Cargo.toml

FROM debian:trixie-slim
RUN apt-get update && apt-get install -y ca-certificates libssl3 curl && rm -rf /var/lib/apt/lists/*
WORKDIR /app

COPY --from=builder /app/sink/target/release/sink /usr/local/bin/sink
COPY --from=builder /app/api/target/release/api /usr/local/bin/api
COPY --from=builder /app/erc20.spkg /app/erc20.spkg
COPY migrations ./migrations
