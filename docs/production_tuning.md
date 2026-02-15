# Production Tuning Guide

To achieve maximum performance (targeted 500k+ OPS) with the Layer 4 Load Balancer, you must tune the underlying operating system and hardware. This guide covers essential kernel parameters, file descriptor limits, and NIC settings.

## 1. Build optimizations

**Why:** Rust's default `debug` build includes heavy runtime checks, overflow protection, and no optimizations. It is often 10-50x slower than release builds.
**What it does:** Compiles with full optimizations, vectorization, and removes debug symbols to produce a lean, fast binary.

Ensure you are running the binary built with the release profile:

```bash
cargo build --release
```

The `release` profile is configured in `Cargo.toml` with:

- `opt-level = 3`: Maximum optimizations.
- `lto = "fat"`: Link Time Optimization for better cross-crate optimization.
- `codegen-units = 1`: Slower build, but faster code.
- `panic = "abort"`: Removes stack unwinding overhead.

## 2. System Limits (ulimit)

**Why:** Every TCP connection in Linux requires a file descriptor (FD). Use `ulimit -n` to check.
**What it does:** Prevents "Too many open files" errors when handling thousands of concurrent connections.

Increase the maximum number of open file descriptors. The default (often 1024) is insufficient for high-concurrency loads.

**Temporary (current session):**

```bash
ulimit -n 1000000
```

**Permanent (`/etc/security/limits.conf`):**

```conf
* soft nofile 1000000
* hard nofile 1000000
root soft nofile 1000000
root hard nofile 1000000
```

## 3. Kernel Tuning (sysctl)

**Why:** Default Linux kernels are tuned for general-purpose use (browsing, light serving), not high-throughput load balancing.
**What it does:** Optimizes the TCP stack to handle connection bursts, large buffers, and rapid recycling of sockets.

Add the following to `/etc/sysctl.conf` and run `sysctl -p` to apply.

### TCP Stack Tuning

```ini
# Max open files (system-wide)
fs.file-max = 2097152

# TCP Backlog Queue (prevent dropped SYNs during bursts)
# Why: If the queue fills up during a traffic spike, the kernel drops new connections.
net.core.somaxconn = 65535
net.ipv4.tcp_max_syn_backlog = 65535

# TCP Memory Buffers (autotuning)
# Why: Larger buffers allow higher throughput on high-latency links (BDP).
# 16MB - 128MB - 256MB
net.ipv4.tcp_rmem = 16777216 134217728 268435456
net.ipv4.tcp_wmem = 16777216 134217728 268435456

# Core buffer limits
net.core.rmem_max = 268435456
net.core.wmem_max = 268435456
net.core.rmem_default = 65536
net.core.wmem_default = 65536
```

### Port Availability

**Why:** Every outgoing connection to a backend consumes a local source port. The default range is often small (e.g., 28k ports).
**What it does:** Doubles the available source ports, preventing `EADDRNOTAVAIL` errors under load.

```ini
net.ipv4.ip_local_port_range = 1024 65535
```

### Time Wait Reuse

**Why:** Closed TCP connections stay in `TIME_WAIT` for 60s. High churning connections can exhaust all ports.
**What it does:** Allows the kernel to safely reuse these sockets for new outgoing connections immediately.

```ini
# Valid for Linux kernel < 4.12. For newer kernels, use tcp_tw_reuse only if needed, 
# but generally modern kernels handle this well.
net.ipv4.tcp_tw_reuse = 1
```

### Congestion Control

**Why:** Traditional algorithms (CUBIC) interpret packet loss as congestion, slowing down unnecessarily on modern networks.
**What it does:** BBR models the network pipe and paces packets, resulting in significantly higher throughput and lower latency.

```ini
net.core.default_qdisc = fq
net.ipv4.tcp_congestion_control = bbr
```

## 4. NIC Tuning (ethtool)

**Why:** High packet rates (PPS) can overwhelm a single CPU core handling interrupts.
**What it does:** Spreads interrupt handling across multiple cores (RPS/RFS) and increases the hardware buffer (Ring Buffer) to prevent packet drops at the NIC level.

```bash
# Enable Receive Packet Steering (RPS) and Receive Flow Steering (RFS)
# Distribute IRQs across CPUs
```

Increase the ring buffer size:

```bash
ethtool -G eth0 rx 4096 tx 4096
```

## 5. Application Tuning

### Threading Model (`NUM_ACCEPTORS`)

**Why:** Relying on `available_parallelism()` might spawn too many threads on hyper-threaded cores, increasing context switching.
**What it does:** Allows you to pin the number of acceptor threads to physical cores for deterministic latency.

```bash
# Set specific number of acceptor threads
export NUM_ACCEPTORS=8
./layer4-lb --config lb.yaml
```

## 6. Bandwidth Tuning

If you are using the Bandwidth Limiter features:

### Burst Size & Latency

**Why:** Tokens are generated continuously, but checks happen discretely. If the check frequency is lower than token generation, valid packets might get rejected.
**What it does:** A **64KB burst** allows tokens to accumulate for ~6ms (at 10MB/s), absorbing system jitter and scheduling delays without dropping throughput.

### Chunking

**Why:** A single large `write()` call (e.g., 10MB) would lock the limiter for seconds, blocking other connections.
**What it does:** Splitting IO into **16KB chunks** ensures the Lock is held for microseconds, allowing thousands of connections to share the bandwidth limiter fairly.

## 7. Deployment Checklist

- [ ] Built with `--release`?
- [ ] `ulimit -n` > 100k?
- [ ] `net.core.somaxconn` > 10k?
- [ ] Running separately from other heavy workloads?
