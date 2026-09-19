# Stage 1: Build binary
FROM rust:1.80-slim-bookworm AS builder
WORKDIR /app

RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    libsqlite3-dev \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY backend ./backend

RUN cargo build --release -p backend

# Stage 2: Minimal runtime
FROM debian:bookworm-slim
WORKDIR /app

RUN apt-get update && apt-get install -y \
    ca-certificates \
    libsqlite3-0 \
    ffmpeg \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/backend /app/backend

ENV SERVER_HOST=0.0.0.0
ENV SERVER_PORT=3000
ENV STORAGE_ROOT=/data

EXPOSE 3000
CMD ["/app/backend"]