# HLS Streaming Server - Production Dockerfile
# Multi-stage build for minimal image size

# Build stage
FROM rust:1.75-slim-bookworm AS builder

WORKDIR /app

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libavcodec-dev \
    libavformat-dev \
    libavutil-dev \
    libavfilter-dev \
    libswscale-dev \
    libswresample-dev \
    libssl-dev \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Copy source
COPY Cargo.toml Cargo.lock ./
COPY src/ ./src/

# Build in release mode
RUN cargo build --release

# Runtime stage
FROM debian:bookworm-slim

WORKDIR /app

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    libavcodec60 \
    libavformat60 \
    libavutil58 \
    libavfilter9 \
    libswscale7 \
    libswresample4 \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && adduser --system --group --no-create-home hls \
    && mkdir -p /app/config /app/media /app/logs \
    && chown -R hls:hls /app

# Copy binary from builder
COPY --from=builder /app/target/release/hls-server /app/hls-server
COPY --chown=hls:hls config.example.toml /app/config/config.toml

# Switch to non-root user
USER hls

# Expose ports
EXPOSE 3000

# Health check
HEALTHCHECK --interval=30s --timeout=5s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:3000/health || exit 1

# Set environment variables
ENV RUST_LOG=info
ENV HLS_CONFIG=/app/config/config.toml

# Run server
CMD ["/app/hls-server"]
