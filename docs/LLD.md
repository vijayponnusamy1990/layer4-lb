# Low Level Design (LLD)

This document details the internal logic and data structures of the core modules.

## 1. Load Balancer (`balancer.rs`)

Responsible for selecting the backend for a new connection.

### Structs

- `LoadBalancer`:
  - `backends`: `Arc<ArcSwap<Vec<(String, usize)>>>` (List of healthy backend addresses).
  - `index`: `AtomicUsize` (Round-robin counter).

### Logic

- **`next_backend()`**:
    1. Loads the current list of healthy backends.
    2. If empty, returns `None`.
    3. Atomically increments `index` (`fetch_add`).
    4. Returns `backends[index % length]`.
  - *Optimization*: Uses `ArcSwap` allows wait-free reads while the Health Checker updates the list in the background.

## 2. Rate Limiting (`limiter.rs`)

Implements the **Token Bucket** algorithm to control request rate and bandwidth.

### RateLimiter (Connections/Sec)

- **Sharding**: Uses `DashMap<IpAddr, TokenBucket>` to reduce lock contention across threads.
- **TokenBucket**:
  - `tokens`: `f64` (Current available tokens).
  - `last_update`: `Instant` (Last refill time).
- **`check(ip)`**:
  - Calculates time elapsed since `last_update`.
  - Adds `time_elapsed * rate` to `tokens` (up to `burst` limit).
  - If `tokens >= 1.0`, consumes 1.0 and returns `true` (Allowed).
  - Else, returns `false` (Rejected).

### BandwidthManager (Bytes/Sec)

- Same Token Bucket logic, but tokens represent **Bytes**.
- Shared generic `RateLimiter<Key, Bucket>`.

## 3. Bandwidth Streams (`bandwidth.rs`)

To limit bandwidth without blocking the thread, we implement a custom Async Stream wrapper.

### Struct: `RateLimitedStream<S>`

Wraps an underlying stream `S` (e.g., `TcpStream`).

### Logic

- **`poll_read`**:
    1. Check buffer size requested (`buf.remaining()`).
    2. Ask `BandwidthManager` for tokens.
    3. If tokens available -> Proceed to `inner.poll_read()`.
    4. If not available -> Create a `Timer` future that sleeps until tokens refill, return `Poll::Pending`.
- **`poll_write`**:
    Similar logic. Before writing bytes to socket, we must "pay" tokens.

## 4. Connection Proxying (`proxy.rs`)

The core data plane function `proxy_connection`.

### Flow

1. **Connect**: Establish TCP connection to selected Backend.
2. **Wrappers**:
    - Wrap Client Stream in `RateLimitedStream`.
    - Wrap Backend Stream in `RateLimitedStream`.
3. **Split**:
    - Split streams into Read/Write halves (`tokio::io::split`).
4. **Copy**:
    - Spawn two tasks (or use `try_join`):
        - `copy(client_read, backend_write)`
        - `copy(backend_read, client_write)`
5. **Termination**:
    - When one side closes (FIN), the copy finishes.
    - Function returns, dropping all sockets and freeing resources.

## 5. TLS Integration (`tls.rs`, `main.rs`)

### Termination

- **`TlsAcceptor`**: Wraps the listener.
- Handshake happens *before* the Proxy Task starts.
- Decrypted stream is passed to `proxy_connection`.

### Re-Encryption (Backend TLS)

- Implemented inside `proxy.rs`.
- If configured, we use `tokio_rustls::TlsConnector` to handshake with the backend *after* TCP connect but *before* limiting wrappers.

## 6. Clustering (`cluster/mod.rs`)

Implements distributed state using the `foca` crate (SWIM protocol).

### Components

- **`Cluster` Actor**:
  - Owns the UDP socket and the `Foca` instance.
  - Runs a loop handling:
    - **Timer**: Triggers gossip rounds.
    - **UDP Recv**: Passes data to `Foca`.
    - **Command Channel**: Receives local updates to broadcast.
- **`SimpleBroadcastHandler`**:
  - Decodes incoming gossip messages (`UsageUpdate`).
  - Forwards valid updates to the main application via a `mpsc` channel.

### Message Flow

1. **Local Update**: `RateLimiter` -> `ClusterCommand::BroadcastUsage` -> `Cluster`.
2. **Encode**: `Cluster` wraps message in `BroadcastMessage` enum and encodes via `bincode`.
3. **Gossip**: `Foca` piggybacks the message on UDP heartbeats to random peers.
4. **Remote Receive**: Peer receives UDP -> `Foca` -> `BroadcastHandler` -> `rx_cluster_state`.
5. **Apply**: Main loop receives update -> Updates Global `RateLimiter` state.
