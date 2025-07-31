# Multi-stage build for Rust gRPC server
FROM rust:1.75-slim as builder

# Install build dependencies including protobuf compiler
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    libmysqlclient-dev \
    protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*

# Create app directory
WORKDIR /app

# Copy Cargo files first for better caching
COPY Cargo.toml Cargo.lock build.rs ./

# Copy proto files for gRPC compilation
COPY proto ./proto

# Create a dummy main.rs to build dependencies
RUN mkdir -p src && echo "fn main() {}" > src/main.rs

# Build dependencies (this layer will be cached unless Cargo.toml changes)
RUN cargo build --release
RUN rm src/main.rs

# Copy source code and crates
COPY src ./src
COPY crates ./crates

# Build the application
RUN cargo build --release

# Runtime stage
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    libmysqlclient21 \
    && rm -rf /var/lib/apt/lists/*

# Create app user
RUN groupadd -r appuser && useradd -r -g appuser appuser

WORKDIR /app

# Copy the binary from builder stage
COPY --from=builder /app/target/release/cosmic-sync-server /app/cosmic-sync-server

# Copy configuration files if needed
COPY config ./config

# Create data directory
RUN mkdir -p /app/data && chown -R appuser:appuser /app

# Switch to non-root user
USER appuser

# Expose gRPC port (adjust if different)
EXPOSE 50051

# Health check
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
  CMD ./cosmic-sync-server --health-check || exit 1

# Run the server
CMD ["./cosmic-sync-server"]