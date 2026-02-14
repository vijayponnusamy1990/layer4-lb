# Build Stage
FROM rust:bookworm as builder

WORKDIR /usr/src/app
COPY . .

# Build for release
RUN cargo build --release

# Runtime Stage
FROM debian:bookworm-slim

# Install OpenSSL/CA certs if needed for TLS
RUN apt-get update && apt-get install -y libssl-dev ca-certificates && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy binary from builder
COPY --from=builder /usr/src/app/target/release/layer4-lb /app/layer4-lb

# Copy config (default, can be overridden by volume)
COPY lb.yaml /app/lb.yaml

# Run as non-root user for security
RUN useradd -m appuser
USER appuser

# Expose port range (matches docker-compose)
EXPOSE 8000-20000

CMD ["./layer4-lb", "--config", "lb.yaml"]
