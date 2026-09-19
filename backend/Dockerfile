# Stage 1: Build binary & libheif 1.23.1
FROM rust:1.98.1-bookworm AS builder
WORKDIR /app

RUN apt-get update && apt-get install -y \
    pkg-config \
    build-essential \
    cmake \
    git \
    libssl-dev \
    libsqlite3-dev \
    libde265-dev \
    libx265-dev \
    && rm -rf /var/lib/apt/lists/*

# Build and install libheif 1.23.1
RUN git clone --depth 1 --branch v1.23.1 https://github.com/strukturag/libheif.git /tmp/libheif \
    && cd /tmp/libheif \
    && mkdir build && cd build \
    && cmake --preset=release-noplugins .. \
    && cmake --build . --target install \
    && ldconfig \
    && rm -rf /tmp/libheif

# Set PKG_CONFIG_PATH so pkg-config locates libheif.pc in /usr/local
ENV PKG_CONFIG_PATH=/usr/local/lib/pkgconfig:/usr/local/lib64/pkgconfig:/usr/local/lib/x86_64-linux-gnu/pkgconfig:$PKG_CONFIG_PATH
ENV LD_LIBRARY_PATH=/usr/local/lib:/usr/local/lib64:$LD_LIBRARY_PATH

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
    libde265-0 \
    ffmpeg \
    && rm -rf /var/lib/apt/lists/*

# Copy compiled libheif shared libraries from builder
COPY --from=builder /usr/local/lib /usr/local/lib
RUN ldconfig

# Copy backend binary
COPY --from=builder /app/target/release/backend /app/backend

ENV SERVER_HOST=0.0.0.0
ENV SERVER_PORT=3000
ENV STORAGE_ROOT=/data

EXPOSE 3000
CMD ["/app/backend"]