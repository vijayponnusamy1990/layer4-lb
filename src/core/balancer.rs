use std::sync::atomic::{AtomicUsize, AtomicBool, Ordering};
use std::sync::Arc;
use arc_swap::ArcSwap;
use log::{warn, info};

#[derive(Clone)]
pub struct LoadBalancer {
    pub rule_name: String, // Added for metrics
    pub backends: Arc<ArcSwap<Vec<Arc<Backend>>>>, 
    current: Arc<AtomicUsize>,
    connection_limit: Option<usize>,
}

#[derive(Clone)]
pub struct Backend {
    pub rule_name: String, // Added for metrics
    pub addr: String,
    pub active_connections: Arc<AtomicUsize>,
    pub healthy: Arc<AtomicBool>,
    pub drain: Arc<AtomicBool>, // Configured state (true = draining, false = accept traffic)
}

impl LoadBalancer {
    pub fn new(rule_name: String, backend_configs: Vec<crate::config::BackendConfig>, connection_limit: Option<usize>) -> Self {
        let backends: Vec<Arc<Backend>> = backend_configs.into_iter().map(|config| {
            let (addr, drain) = match config {
                crate::config::BackendConfig::Simple(a) => (a, false),
                crate::config::BackendConfig::Detailed { addr, drain } => (addr, drain),
            };

            // Init Metric
            crate::metrics::BACKEND_HEALTH_STATUS.with_label_values(&[&rule_name, &addr]).set(1.0);
            crate::metrics::BACKEND_ACTIVE_CONNECTIONS.with_label_values(&[&rule_name, &addr]).set(0.0);
            
            Arc::new(Backend {
                rule_name: rule_name.clone(),
                addr,
                active_connections: Arc::new(AtomicUsize::new(0)),
                healthy: Arc::new(AtomicBool::new(true)), // Optimistic init
                drain: Arc::new(AtomicBool::new(drain)),
            })
        }).collect();

        LoadBalancer {
            rule_name,
            backends: Arc::new(ArcSwap::from_pointee(backends)),
            current: Arc::new(AtomicUsize::new(0)),
            connection_limit,
        }
    }

    pub async fn update_backends(&self, new_backend_configs: Vec<crate::config::BackendConfig>) {
        // Construct new backend list
        // Optimization: preserve active connection counters for existing backends if possible
        // We need to read the current backends to match addresses
        let current_backends = self.backends.load();
        
        let new_backends: Vec<Arc<Backend>> = new_backend_configs.into_iter().map(|config| {
             let (addr, drain_cfg) = match config {
                crate::config::BackendConfig::Simple(a) => (a, false),
                crate::config::BackendConfig::Detailed { addr, drain } => (addr, drain),
            };

             // Try to find existing backend state
             if let Some(existing) = current_backends.iter().find(|b| b.addr == addr) {
                 // Update drain state if changed
                 existing.drain.store(drain_cfg, Ordering::Relaxed);
                 existing.clone()
             } else {
                 // Init Metric for new backend
                 crate::metrics::BACKEND_HEALTH_STATUS.with_label_values(&[&self.rule_name, &addr]).set(1.0);
                 crate::metrics::BACKEND_ACTIVE_CONNECTIONS.with_label_values(&[&self.rule_name, &addr]).set(0.0);
                 
                 Arc::new(Backend {
                    rule_name: self.rule_name.clone(),
                    addr,
                    active_connections: Arc::new(AtomicUsize::new(0)),
                    healthy: Arc::new(AtomicBool::new(true)),
                    drain: Arc::new(AtomicBool::new(drain_cfg)),
                 })
             }
        }).collect();

        self.backends.store(Arc::new(new_backends));
    }
    
    // Used by Health Checker
    pub async fn set_backend_health(&self, backend_addr: &str, healthy: bool) {
        // Update Metric
        crate::metrics::BACKEND_HEALTH_STATUS.with_label_values(&[&self.rule_name, backend_addr]).set(if healthy { 1.0 } else { 0.0 });

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
            } else {
                log::debug!("Health check update for {}: no change (healthy={})", backend_addr, healthy);
            }
        }
    }

    pub fn next_backend(&self) -> Option<(String, ConnectionGuard)> {
        // Wait-free read!
        let backends = self.backends.load();
        if backends.is_empty() {
            log::debug!("No backends configured");
            return None;
        }

        let start_index = self.current.fetch_add(1, Ordering::Relaxed);
        let len = backends.len();

        for i in 0..len {
            let idx = (start_index + i) % len;
            let backend = &backends[idx];

            // Check if backend is manually disabled (draining)
            if backend.drain.load(Ordering::Relaxed) {
                log::debug!("Backend {} skipped (draining)", backend.addr);
                continue;
            }

            if !backend.healthy.load(Ordering::Relaxed) {
                log::debug!("Backend {} skipped (unhealthy)", backend.addr);
                continue; // Skip unhealthy backends
            }

            if let Some(limit) = self.connection_limit {
                let current_conns = backend.active_connections.load(Ordering::Relaxed);
                if current_conns >= limit {
                    log::debug!("Backend {} skipped (connection limit reached: {}/{})", backend.addr, current_conns, limit);
                    continue; // Backend full, try next
                }
            }

            // Increment active connections
            backend.active_connections.fetch_add(1, Ordering::Relaxed);
            
            // Metric Increment
            crate::metrics::BACKEND_ACTIVE_CONNECTIONS.with_label_values(&[&backend.rule_name, &backend.addr]).inc();

            log::debug!("Selected backend: {} (active: {})", backend.addr, backend.active_connections.load(Ordering::Relaxed));
            return Some((
                backend.addr.clone(),
                ConnectionGuard {
                    rule_name: backend.rule_name.clone(), // Added
                    backend_addr: backend.addr.clone(),   // Added
                    counter: backend.active_connections.clone(),
                }
            ));
        }

        warn!("All backends are at capacity, unhealthy, or draining");
        None
    }
}

pub struct ConnectionGuard {
    rule_name: String,
    backend_addr: String,
    counter: Arc<AtomicUsize>,
}

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        self.counter.fetch_sub(1, Ordering::Relaxed);
        // Metric Decrement
        crate::metrics::BACKEND_ACTIVE_CONNECTIONS.with_label_values(&[&self.rule_name, &self.backend_addr]).dec();
    }
}
