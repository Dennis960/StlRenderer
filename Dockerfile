# Build stage
FROM rust:latest AS builder

WORKDIR /app
COPY stl-renderer/Cargo.toml stl-renderer/Cargo.lock* ./
# Create dummy main for dependency caching
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release 2>/dev/null || true
# Now copy real source and build
COPY stl-renderer/src ./src
COPY stl-renderer/static ./static
RUN touch src/main.rs && cargo build --release

# Runtime stage
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/stl-renderer /usr/local/bin/stl-renderer

ENV BIND_ADDR=0.0.0.0:8080
EXPOSE 8080

CMD ["stl-renderer"]
