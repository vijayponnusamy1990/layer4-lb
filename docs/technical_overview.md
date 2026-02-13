# Technical Overview: Layer 4 Load Balancer

This document provides a high-level architectural overview of the Layer 4 Load Balancer.

## Design Philosophy

The application is designed as a **Non-Blocking, Event-Driven** server using Rust's async ecosystem.

## Supported Protocols

As a Layer 4 (TCP) Load Balancer, it implicitly supports **any** application protocol that runs over TCP. Common use cases include:

- **Databases**: PostgreSQL, MySQL, Redis, MongoDB.
- **Message Queues**: Kafka, RabbitMQ, NATS.
- **Web**: HTTP/1.1, HTTP/2 (Passthrough).
- **Other**: SMTP, LDAP, Game Servers.

> **Note for Kafka**: When load balancing Kafka, ensure your brokers are configured with `advertised.listeners` that match the Load Balancer's public address if you are not using transparent proxying.

## Feature Specifications

### 1. Load Balancing

**Description**: Distributes incoming TCP connections across a pool of backend servers using a **Weighted Round Robin** strategy.
**Configuration**:

```yaml
rules:
  - name: "AppService"
    listen: "0.0.0.0:8080"
    backends:
      - "10.0.0.1:8080"
      - "10.0.0.2:8080"
    backend_connection_limit: 1000 # Max conns per backend
```

**Limitations**:

- **Sticky Sessions**: Not supported (this is L4, no cookies/headers).
- **Weights**: Currently implicit (equal weight). Future versions will support explicit weights.

### 2. Rate Limiting

**Description**: Limits the number of new connections/requests per second from a single client IP using a **Token Bucket** algorithm.
**Configuration**:

```yaml
rate_limit:
  enabled: true
  requests_per_second: 100
  burst: 50
```

**Limitations**:

- **Granularity**: IP-based only. Cannot limit by API key or Header (L7 features).
- **NAT**: Clients behind a single NAT (e.g., corporate office) will share the same limit.

### 3. Bandwidth Limiting

**Description**: Throttles upload and download speeds for clients or backends.
**Configuration**:

```yaml
bandwidth_limit:
  enabled: true
  client:
    upload_per_sec: 10485760   # 10 MB/s
    download_per_sec: 20971520 # 20 MB/s
```

**Limitations**:

- **CPU Overhead**: High bandwidth limits (1Gbps+) may incur CPU cost due to frequent token checks.
- **Bursts**: Short bursts are allowed by design; strictly constant bit rate (CBR) is not enforced.

### 4. TLS Termination & Re-Encryption

**Description**: Decrypts incoming TLS (HTTPS) and optionally re-encrypts to the backend (Zero Trust).
**Configuration**:

```yaml
tls:
  enabled: true
  cert: "/path/to/cert.pem"
  key: "/path/to/key.pem"
backend_tls:
  enabled: true
  ignore_verify: false # Set true for self-signed backend certs
```

**Limitations**:

- **SNI**: Currently serving a single cert per rule. Multi-cert SNI selection is planned.
- **Client Auth (mTLS)**: Not currently enforced (planned).

### 5. Clustering (Distributed State)

**Description**: Synchronizes rate limit usage across multiple LB instances using P2P Gossip (SWIM protocol).
**Configuration**:

```yaml
cluster:
  enabled: true
  bind_addr: "0.0.0.0:9090"
  peers: ["10.0.0.2:9090"]
```

**Limitations**:

- **Consistency**: Eventual consistency. There is a slight delay (gossip interval) in syncing global counters.
- **Traffic**: Uses UDP. Packet loss may cause temporary divergence in rate limits.

### 6. Health Checks

**Description**: Actively probes backends to ensure they are reachable.
**Configuration**:

```yaml
health_check:
  enabled: true
  interval_ms: 5000
  timeout_ms: 1000
  protocol: "http" # or "tcp"
  path: "/health"
```

**Limitations**:

- **Protocol**: HTTP check expects 200 OK. TCP check ensures syn/ack.
- **Failover Time**: Depends on `interval_ms`. Fast failure detection requires low intervals (higher traffic).

## Internal Architecture

- **Language**: Rust (Memory safety, Zero-cost abstractions).
- **Runtime**: `tokio` (Async I/O, Task scheduling, Green threads).
- **Networking**: `socket2` (Raw socket manipulation for `SO_REUSEPORT`).
- **TLS**: `rustls` / `tokio-rustls` (Modern, safe TLS implementation).

### High-Level Design

```mermaid
graph TD
    Client[Client] -->|TCP SYN| Interface[Network Interface]
    Interface -->|"Rule 1 (Port 8080)"| Acceptor1[Acceptor Task 1]
    Interface -->|"Rule 1 (Port 8080)"| Acceptor2[Acceptor Task 2]
    
    subgraph "Load Balancer Process"
        Acceptor1 -->|Spawn| ProxyTask[Proxy Task]
        
        ProxyTask -->|Check| RateLimiter[Rate Limiter (Token Bucket)]
        ProxyTask -->|Select| LoadBalancer[Load Balancer (Round Robin)]
        
        LoadBalancer -->|Return IP| ProxyTask
        
        ProxyTask -->|Connect| Backend[Backend Server]
        
        subgraph "Data Plane"
            ProxyTask -- Copy Bidirectional --> Backend
        end
    end
```

### Key Components

1. **Main Loop (`main.rs`)**:
    - Parses configuration.
    - Initializes shared state (`Arc<LoadBalancer>`, `Arc<RateLimiter>`).
    - Spawns multiple **Acceptor Tasks** per listening port (using `SO_REUSEPORT`).
    - Spawns a **Config Watcher** task for hot-reloading.

2. **Acceptor Tasks**:
    - Listen for incoming TCP connections.
    - Perform initial Rate Limiting (IP-based).
    - Wrap connections in TLS (if termination is enabled).
    - Spawn a dedicated **Proxy Task** for each connection.

3. **Proxy Task**:
    - Determines the backend server using the Load Balancer strategy.
    - Establishes a connection to the backend (Plain TCP or TLS).
    - Wraps sockets in **Bandwidth Limiters**.
    - Pumps bytes bidirectionally between Client and Backend (`tokio::io::copy`).

4. **Shared State**:
    - **LoadBalancer**: Stores the list of healthy backends. Updated by Health Checks.
    - **HealthChecker**: background tasks that ping backends and update the LoadBalancer state atomically.

### Module Structure

The codebase is organized into domain-specific modules:

- **`core`**: Business logic (Load Balancer, Health Checks).
- **`networking`**: Protocol handling (Proxying, TLS).
- **`traffic`**: Rate and Bandwidth limiting logic.
- **`cluster`**: Distributed state management (Gossip).
- **`config`**: Configuration parsing.

### Distributed Deployment (Clustering)

The Load Balancer supports a **P2P Gossip Architecture** (SWIM protocol) for state synchronization.

- **Membership**: Nodes discover each other via seed peers or dynamic discovery.
- **State Sharing**: Usage metrics (e.g., global request counts) are gossiped across the cluster.
- **Convergence**: Allows "approximate" global rate limiting without a central bottleneck like Redis.

We avoid traditional OS threads per connection. Instead, we use tens of thousands of lightweight **Tokio Tasks**.

- **Memory Footprint**: Each task takes ~few KB of RAM.
- **Context Switching**: Handled in userspace by Tokio, extremely fast.
- **Scalability**: Can handle 100k+ concurrent connections on a single modern CPU core.
