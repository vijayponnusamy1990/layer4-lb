# Clustering & Distributed State Proposal

To enable a "Memory Grid" across instances, we need to introduce a **Shared State Layer**.

## Option 1: Redis-Based (Recommended)

The standard pattern for high-performance Load Balancers.

### 1. Distributed Rate Limiting

Instead of checking a local `DashMap`, every request checks a central Redis.

- **Pros**: Exact global limits.
- **Cons**: Adds network latency (1-2ms) to every request.
- **Optimization**: Use **slide window** or **batching** (async reservation of tokens).

### 2. Config Synchronization

- **Mechanism**: All instances subscribe to a Redis Channel (`LB_CONFIG_UPDATES`).
- **Flow**: Admin pushes new YAML to Redis -> All instances receive event -> Hot Reload.

### 3. Session Stickiness

- Store `ClientIP -> BackendID` in Redis.
- Any L4 node can route the client to the correct backend.

## Option 2: Peer-to-Peer (Gossip/CRDTs)

Use a library like `foca` (SWIM protocol) or `zenoh` to form a mesh.

- **Pros**: No external dependency (like Redis).
- **Cons**: Extremely complex to implement. Eventual consistency makes strict rate limiting hard.

## Recommendation: Redis + Local Caching

1. **Config**: Use Redis Pub/Sub for instant updates.
2. **Rate Limits**: Keep limits **local** but synchronized loosely.
    - Or use **Redis** only for "Global High Limits" (DDoS protection) and local for fine-grained.
