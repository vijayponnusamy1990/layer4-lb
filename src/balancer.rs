use std::sync::atomic::{AtomicUsize, AtomicBool, Ordering};
use std::sync::Arc;
use arc_swap::ArcSwap;
use log::{warn, info};

#[derive(Clone)]
pub struct LoadBalancer {
    pub backends: Arc<ArcSwap<Vec<Arc<Backend>>>>, // Changed to ArcSwap for wait-free reads
    current: Arc<AtomicUsize>,
    connection_limit: Option<usize>,
}

#[derive(Clone)]
pub struct Backend {
    pub addr: String,
    pub active_connections: Arc<AtomicUsize>,
    pub healthy: Arc<AtomicBool>,
}

impl LoadBalancer {
    pub fn new(backend_addrs: Vec<String>, connection_limit: Option<usize>) -> Self {
        let backends: Vec<Arc<Backend>> = backend_addrs.into_iter().map(|addr| Arc::new(Backend {
            addr,
            active_connections: Arc::new(AtomicUsize::new(0)),
            healthy: Arc::new(AtomicBool::new(true)), // Optimistic init
        })).collect();

        LoadBalancer {
            backends: Arc::new(ArcSwap::from_pointee(backends)),
            current: Arc::new(AtomicUsize::new(0)),
            connection_limit,
        }
    }

    pub async fn update_backends(&self, new_backend_addrs: Vec<String>) {
        // Construct new backend list
        // Optimization: preserve active connection counters for existing backends if possible
        // We need to read the current backends to match addresses
        let current_backends = self.backends.load();
        
        let new_backends: Vec<Arc<Backend>> = new_backend_addrs.into_iter().map(|addr| {
             // Try to find existing backend state
             if let Some(existing) = current_backends.iter().find(|b| b.addr == addr) {
                 existing.clone()
             } else {
                 Arc::new(Backend {
                    addr,
                    active_connections: Arc::new(AtomicUsize::new(0)),
                    healthy: Arc::new(AtomicBool::new(true)),
                 })
             }
        }).collect();

        self.backends.store(Arc::new(new_backends));
    }
    
    // Used by Health Checker
    pub async fn set_backend_health(&self, backend_addr: &str, healthy: bool) {
        // We can just iterate the current snapshot. Since backends are Arc, 
        // updating atomic bool is visible to everyone.
        let backends = self.backends.load();
        if let Some(backend) = backends.iter().find(|b| b.addr == backend_addr) {
            let old = backend.healthy.swap(healthy, Ordering::Relaxed);
            if old != healthy {
                if healthy {
                    info!("Backend {} marked HEALTHY", backend_addr);
                } else {
                    warn!("Backend {} marked UNHEALTHY", backend_addr);
                }
            }
        }
    }

    pub fn next_backend(&self) -> Option<(String, ConnectionGuard)> {
        // Wait-free read!
        let backends = self.backends.load();
        if backends.is_empty() {
            return None;
        }

        let start_index = self.current.fetch_add(1, Ordering::Relaxed);
        let len = backends.len();

        for i in 0..len {
            let idx = (start_index + i) % len;
            let backend = &backends[idx];

            if !backend.healthy.load(Ordering::Relaxed) {
                continue; // Skip unhealthy backends
            }

            if let Some(limit) = self.connection_limit {
                let current_conns = backend.active_connections.load(Ordering::Relaxed);
                if current_conns >= limit {
                    continue; // Backend full, try next
                }
            }

            // Increment active connections
            backend.active_connections.fetch_add(1, Ordering::Relaxed);
            return Some((
                backend.addr.clone(),
                ConnectionGuard {
                    counter: backend.active_connections.clone(),
                }
            ));
        }

        warn!("All backends are at capacity or unhealthy");
        None
    }
}

pub struct ConnectionGuard {
    counter: Arc<AtomicUsize>,
}

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        self.counter.fetch_sub(1, Ordering::Relaxed);
    }
}
