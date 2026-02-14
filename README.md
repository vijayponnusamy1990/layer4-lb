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
- **Clustering**: P2P state synchronization (Gossip protocol) for distributed rate limiting.
- **TLS Support**:
  - **Termination**: Decrypts incoming TLS (HTTPS) traffic.
  - **Re-Encryption**: Encrypts traffic to secure backends.
  - **Passthrough**: Forwards encrypted traffic without decryption.
- **Microservice Architecture**: Modular design (`core`, `networking`, `traffic`, `cluster`).
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

    > **Note:** For optimal performance, please refer to the [Production Tuning Guide](docs/production_tuning.md).

## Quick Start Scenarios

### 1. Simple Load Balancer

Forward traffic from 8080 to two backends:

```yaml
rules:
  - name: "SimpleHTTP"
    listen: "0.0.0.0:8080"
    backends: ["127.0.0.1:8081", "127.0.0.1:8082"]
```

### 2. TLS Termination (HTTPS)

Decrypt traffic at LB, forward plain text to backend:

```yaml
rules:
  - name: "SecureWeb"
    listen: "0.0.0.0:443"
    backends: ["127.0.0.1:8080"]
    tls:
      enabled: true
      cert: "./certs/server.crt"
      key: "./certs/server.key"
```

### 3. Rate Limiting Protection

Limit each client IP to 100 req/s:

```yaml
rules:
  - name: "ProtectedAPI"
    listen: "0.0.0.0:9090"
    backends: ["127.0.0.1:9091"]
    rate_limit:
      enabled: true
      requests_per_second: 100
      burst: 20
```

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

# Optional: P2P Cluster Configuration
cluster:
  enabled: true
  bind_addr: "0.0.0.0:9090" # UDP Gossip port
  peers:
    - "10.0.0.2:9090"

```

## Running Locally

1. **Generate Certificates** (if testing TLS):

    ```bash
    chmod +x utilities/generate_certs.sh
    ./utilities/generate_certs.sh
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

## Utilities

Helper scripts are located in the `utilities/` directory:

- **`generate_certs.sh`**: Generates self-signed certificates for testing.
- **`throughput.rs`**: High-performance benchmark tool (Source/Sink).
  - Source code moved to `utilities/`.
  - Run via Cargo: `cargo run --release --bin throughput -- server 9001`
- **Python Scripts**: Various test clients (e.g., `echo_server.py`, `failover_test.py`, `benchmark.py`).
  - Run them from the project root: `python3 utilities/echo_server.py 8081`

See `docs/` for detailed architecture documentation.
