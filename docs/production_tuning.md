# Production Tuning Guide

To achieve maximum performance (targeted 500k+ OPS) with the Layer 4 Load Balancer, you must tune the underlying operating system and hardware. This guide covers essential kernel parameters, file descriptor limits, and NIC settings.

## 1. Build optimizations

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

Add the following to `/etc/sysctl.conf` and run `sysctl -p` to apply.

### TCP Stack Tuning

```ini
# Max open files (system-wide)
fs.file-max = 2097152

# TCP Backlog Queue (prevent dropped SYNs during bursts)
net.core.somaxconn = 65535
net.ipv4.tcp_max_syn_backlog = 65535

# TCP Memory Buffers (autotuning)
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

Expand the local port range to allow more outgoing connections to backends.

```ini
net.ipv4.ip_local_port_range = 1024 65535
```

### Time Wait Reuse

Allow reuse of sockets in `TIME_WAIT` state for new connections.

```ini
# Valid for Linux kernel < 4.12. For newer kernels, use tcp_tw_reuse only if needed, 
# but generally modern kernels handle this well.
net.ipv4.tcp_tw_reuse = 1
```

### Congestion Control

Use BBR for better throughput and lower latency.

```ini
net.core.default_qdisc = fq
net.ipv4.tcp_congestion_control = bbr
```

## 4. NIC Tuning (ethtool)

Offload packet processing to the Network Interface Card (NIC) hardware where possible.

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

By default, the load balancer spawns one acceptor thread per available CPU core. In widely non-uniform memory access (NUMA) systems or when running alongside other services, you might want to manually control this.

```bash
# Set specific number of acceptor threads
export NUM_ACCEPTORS=8
./layer4-lb --config lb.yaml
```

## 6. Deployment Checklist

- [ ] Built with `--release`?
- [ ] `ulimit -n` > 100k?
- [ ] `net.core.somaxconn` > 10k?
- [ ] Running separately from other heavy workloads?
