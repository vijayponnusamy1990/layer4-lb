# Layer 4 Load Balancer (Rust)

A high-performance, multi-threaded Layer 4 (TCP) Load Balancer written in Rust. It supports weighted round-robin load balancing, active health checks, rate limiting, bandwidth throttling, and TLS termination/re-encryption.

## Features

- **Layer 4 TCP Proxy**: Agnostic to upper-layer protocols (HTTP, MySQL, Redis, etc.).
- **High Performance**: Built on `tokio` (async I/O) and `socket2` (SO_REUSEPORT) for massive concurrency (target: 500k OPS).
- **Load Balancing**: Weighted Round Robin strategy.
- **Health Checks**: Active TCP/HTTP probing to remove unhealthy backends.
- **Traffic Control**:
  - **Rate Limiting**: Token bucket (requests/sec + burst) per client IP.
  - **Bandwidth Limiting**: Token bucket byte counting (upload/download) per client or backend.
- **TLS Support**:
  - **Termination**: Decrypts incoming TLS (HTTPS) traffic.
  - **Re-Encryption**: Encrypts traffic to secure backends.
  - **Passthrough**: Forwards encrypted traffic without decryption.
- **Dynamic Configuration**: Hot-reload support for `lb.yaml`.
- **Docker Ready**: Multi-stage Dockerfile and optimized `docker-compose`.

## Prerequisites

- **Rust**: Install via [rustup](https://rustup.rs/).
- **Docker** (Optional, for containerized deployment).
- **OpenSSL** (For generating test certificates).

## Installation

1. Clone the repository:

    ```bash
    git clone <repository_url>
    cd layer4-lb/layer4-lb
    ```

2. Build the project in release mode:

    ```bash
    cargo build --release
    ```

    The binary will be located at `target/release/layer4-lb`.

## Configuration

Control the load balancer using a YAML configuration file (default: `lb.yaml`).

```yaml
rules:
  - name: "MyWebService"
    listen: "0.0.0.0:8080"
    backends:
      - "127.0.0.1:8081"
      - "127.0.0.1:8082"
    backend_connection_limit: 100
    
    # Optional: Rate Limiting
    rate_limit:
        enabled: true
        requests_per_second: 1000
        burst: 2000

    # Optional: Bandwidth Limiting
    bandwidth_limit:
        enabled: true
        client:
             upload_per_sec: 10485760 # 10 MB/s
             download_per_sec: 10485760

    # Optional: TLS Termination
    # tls:
    #   enabled: true
    #   cert: "./certs/server.crt"
    #   key: "./certs/server.key"
```

## Running Locally

1. **Generate Certificates** (if testing TLS):

    ```bash
    chmod +x generate_certs.sh
    ./generate_certs.sh
    ```

2. **Run the Load Balancer**:

    ```bash
    cargo run --release -- --config lb.yaml
    ```

    Or run the binary directly:

    ```bash
    ./target/release/layer4-lb --config lb.yaml
    ```

## Running with Docker

1. **Build the Image**:

    ```bash
    docker build -t layer4-lb .
    ```

2. **Run with Compose**:

    ```bash
    docker-compose up -d
    ```

    *Note*: The default `docker-compose.yml` mounts `lb.yaml` and exposes ports 8000-20000.

## Testing

For testing different TLS modes, use the provided test config:

```bash
cargo run --release -- --config lb_tls_test.yaml
```

See `docs/` for detailed architecture documentation.
