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
    - **HealthChecker**: background tasks that ping backends and update the LoadBalancer state atomically (using `ArcSwap` or `RwLock`).

### Concurrency Model

We avoid traditional OS threads per connection. Instead, we use tens of thousands of lightweight **Tokio Tasks**.

- **Memory Footprint**: Each task takes ~few KB of RAM.
- **Context Switching**: Handled in userspace by Tokio, extremely fast.
- **Scalability**: Can handle 100k+ concurrent connections on a single modern CPU core, limited mostly by OS file descriptors and bandwidth.
