# Production Tuning Guide: Achieving 500k OPS

The Layer 4 Load Balancer is architected for high performance, but achieving **500,000 Operations Per Second (OPS)** requires significant tuning of the Operating System and Container Runtime.

## 1. Application Tuning

- **Acceptor Threads**: The application automatically detects the number of CPU cores and spawns one acceptor thread per core per listener.
  - You can override this with the `NUM_ACCEPTORS` environment variable.
  - For 500k OPS, ensure your container has access to **multiple cores** (e.g., 8+ cores) and set `NUM_ACCEPTORS` accordingly if auto-detection fails.

    ```bash
    export NUM_ACCEPTORS=16
    ```

- **Logging**: **CRITICAL**. At 500k OPS, `info!` logs will kill performance.
  - Set `RUST_LOG=error` or `warn`.

## 2. Docker Tuning

Docker containers inherit limits from the host but imposed limits can restrict performance.

### Running with `docker run`

You must explicitly pass ulimits and sysctls:

```bash
docker run -d \
  --name layer4-lb \
  --net host \  # High performance networking (bypass bridge)
  --ulimit nofile=1048576:1048576 \
  --sysctl net.ipv4.tcp_tw_reuse=1 \
  --sysctl net.core.somaxconn=65535 \
  -e NUM_ACCEPTORS=8 \
  -e RUST_LOG=error \
  layer4-lb
```

### Docker Compose

See `docker-compose.yml` for a production-ready example:

```yaml
services:
  lb:
    image: layer4-lb
    network_mode: "host" # Recommended for max throughput
    ulimits:
      nofile:
        soft: 1048576
        hard: 1048576
    sysctls:
      - net.ipv4.tcp_tw_reuse=1
      - net.core.somaxconn=65535
```

## 3. Kubernetes Tuning

In Kubernetes, you must configure the **Pod Security Context** and ensuring the underlying node has the `sysctl` settings applied (or allow safe sysctls).

### Pod Specification

```yaml
apiVersion: v1
kind: Pod
metadata:
  name: layer4-lb
spec:
  containers:
  - name: lb
    image: layer4-lb
    resources:
      requests:
        cpu: "4"
        memory: "4Gi"
      limits:
        cpu: "8"
        memory: "8Gi"
    env:
    - name: NUM_ACCEPTORS
      value: "8" # Match CPU limits
    - name: RUST_LOG
      value: "error"
    securityContext:
      # Allow process to maximize open files
      capabilities:
        add: ["IPC_LOCK", "SYS_RESOURCE"]
  securityContext:
    sysctls:
    - name: net.ipv4.tcp_tw_reuse
      value: "1"
    - name: net.core.somaxconn
      value: "65535"
```

*Note: Some sysctls might be considered "unsafe" by default kubelet settings and require cluster admin configuration to allow.*

## 4. Hardware/Node Requirements

- **CPU**: 500k OPS is CPU heavy. Plan for 1 core per 20k-50k active concurrent connections (depends on SSL/Logic).
- **NIC**: Use SR-IOV or Host Networking if possible to avoid bridge overhead.
