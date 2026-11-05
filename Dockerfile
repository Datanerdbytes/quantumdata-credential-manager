# Stage 1: Build
FROM rust:1-bookworm AS builder

WORKDIR /app

# Install build dependencies for rusqlite (bundled SQLite) and crossterm
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy dependency manifests first for layer caching
COPY Cargo.toml Cargo.lock ./

# Build dependencies only (cache-friendly)
RUN cargo build --release --locked 2>/dev/null || true

# Copy source code
COPY src ./src

# Build the binary
RUN cargo build --release --locked

# Stage 2: Runtime
FROM debian:bookworm-slim

WORKDIR /app

# Install ca-certificates for TLS (Notion API over HTTPS)
RUN apt-get update && apt-get install -y \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Copy the built binary
COPY --from=builder /app/target/release/notion-api-credential-manager /usr/local/bin/notion-api-credential-manager

# Create a non-root user and give ownership of /app
RUN useradd -m -s /bin/bash appuser && chown -R appuser:appuser /app
USER appuser

# Default environment variables (override at runtime)
ENV NOTION_DATA_SOURCE_ID=""
ENV DB_PATH="/app/credentials.db"

# The app needs a TTY for the TUI — run with `docker run -it`
ENTRYPOINT ["notion-api-credential-manager"]
