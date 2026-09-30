FROM rust:1.98-slim as builder

RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY migrations ./migrations
COPY proto ./proto
COPY sink ./sink
COPY api ./api

RUN cargo build --release --manifest-path sink/Cargo.toml
RUN cargo build --release --manifest-path api/Cargo.toml

FROM debian:trixie-slim
RUN apt-get update && apt-get install -y ca-certificates libssl3 curl && rm -rf /var/lib/apt/lists/*
WORKDIR /app

COPY --from=builder /app/sink/target/release/sink /usr/local/bin/sink
COPY --from=builder /app/api/target/release/api /usr/local/bin/api
COPY migrations ./migrations