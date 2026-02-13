# Technical Overview: Layer 4 Load Balancer

This document provides a high-level architectural overview of the Layer 4 Load Balancer.

## Architecture Guidelines

The application is designed as a **Non-Blocking, Event-Driven** server using Rust's async ecosystem.

### Core Technologies

- **Language**: Rust (Memory safety, Zero-cost abstractions).
- **Runtime**: `tokio` (Async I/O, Task scheduling, Green threads).
- **Networking**: `socket2` (Raw socket manipulation for `SO_REUSEPORT`).
- **TLS**: `rustls` / `tokio-rustls` (Modern, safe TLS implementation).

### High-Level Design

```mermaid
graph TD
    Client[Client] -->|TCP SYN| Interface[Network Interface]
    Interface -->|Rule 1 (Port 8080)| Acceptor1[Acceptor Task 1]
    Interface -->|Rule 1 (Port 8080)| Acceptor2[Acceptor Task 2]
    
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

### Distributed Deployment (Multi-Instance)

The Load Balancer uses a **Shared-Nothing Architecture**. State is not synchronized between instances.

1. **Traffic Distribution**:
    - Deploy multiple instances behind a Cloud LB (AWS NLB, GCP LB) or use DNS Round Robin.
    - Each instance handles a subset of the traffic independently.

2. **Impact on Limits**:
    - **Rate Limits**: Configured **Per Instance**.
        - *Example*: If you set `100 RPS` and run **3 instances**, the total cluster capacity is `300 RPS`.
    - **Bandwidth Limits**: Configured **Per Instance**.
        - *Example*: If you set `10 MB/s` and run **3 instances**, the total cluster bandwidth is `30 MB/s`.
    - **Connection Limits**: configured **Per Instance**.

3. **Recommendation**:
    - Divide your total desired cluster limit by the number of instances ($Limit_{Instance} = Limit_{Total} / N_{Instances}$).
