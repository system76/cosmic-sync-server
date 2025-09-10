# Multi-stage build for Rust gRPC server
FROM rust:slim AS builder

# Build arguments
ARG VCS_REF
ARG BUILD_DATE
ARG VERSION
ARG RUST_TARGET=x86_64-unknown-linux-musl

# Install build dependencies including protobuf compiler and musl toolchain
RUN apt-get update && apt-get install -y \
    pkg-config \
    protobuf-compiler \
    musl-tools \
    && rm -rf /var/lib/apt/lists/*

# Enable target
RUN rustup target add ${RUST_TARGET}

# Create app directory
WORKDIR /app

# Copy Cargo files first for better caching
COPY Cargo.toml Cargo.lock build.rs ./

# Copy proto files for gRPC compilation
COPY proto ./proto

RUN mkdir -p src && echo "fn main() {}" > src/main.rs && echo "pub fn dummy() {}" > src/lib.rs
RUN cargo build --release --features redis-cache --target ${RUST_TARGET}

RUN rm -f src/main.rs src/lib.rs
COPY src ./src
RUN cargo clean

RUN cargo build --release --bin cosmic-sync-server --features redis-cache --target ${RUST_TARGET}
# Runtime stage
FROM gcr.io/distroless/static:nonroot

WORKDIR /app

# Copy the binary from builder stage (musl static)
ARG RUST_TARGET=x86_64-unknown-linux-musl
COPY --from=builder /app/target/${RUST_TARGET}/release/cosmic-sync-server /app/cosmic-sync-server
COPY config ./config

USER nonroot:nonroot

EXPOSE 50051 8080

# Distroless lacks curl; rely on container orchestrator health checks
ENTRYPOINT ["/app/cosmic-sync-server"]