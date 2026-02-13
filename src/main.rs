use clap::Parser;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::net::TcpListener;
use log::{info, error, warn};
use notify::{Watcher, RecursiveMode, RecommendedWatcher, Event};
use tokio::sync::mpsc;
use tokio::sync::RwLock;
use std::collections::HashMap;


mod config;
mod core;
mod networking;
mod traffic;
mod common;
mod cluster;

use config::{Config, RateLimitConfig, BandwidthLimitConfig};
use traffic::limiter::{RateLimiter, BandwidthManager};
use networking::proxy::{self, ProxyConfig};
use core::{balancer, health};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long, default_value = "lb.yaml")]
    config: PathBuf,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();
    let args = Args::parse();

    // 1. Load Initial Configuration
    let config_content = std::fs::read_to_string(&args.config)?;
    let config: Config = serde_yaml::from_str(&config_content)?;
    config.validate()?;

    info!("Loaded configuration with {} rules", config.rules.len());

    // Store LBs for hot reload: Rule Name -> LoadBalancer
    let lbs: Arc<RwLock<HashMap<String, Arc<balancer::LoadBalancer>>>> = Arc::new(RwLock::new(HashMap::new()));
    
    // 2. Initialize Rules & spawn listeners
    for rule in config.rules.iter() {
        info!("Initializing rule: {}", rule.name);
        
        let lb = Arc::new(balancer::LoadBalancer::new(rule.backends.clone(), rule.backend_connection_limit));
        lbs.write().await.insert(rule.name.clone(), lb.clone());

        // Spawn Health Checkers
        if let Some(hc_config) = &rule.health_check {
            info!("Spawning health checkers for rule '{}'", rule.name);
            for backend_addr in &rule.backends {
                health::start_health_check(lb.clone(), backend_addr.clone(), hc_config.clone());
            }
        }

        let rate_limiter = Arc::new(RateLimiter::new(rule.rate_limit.clone().unwrap_or(RateLimitConfig {
            enabled: false,
            requests_per_second: 0,
            burst: 0,
        })));

        let bandwidth_manager = Arc::new(BandwidthManager::new(rule.bandwidth_limit.clone().unwrap_or(BandwidthLimitConfig {
            enabled: false,
            client: None,
            backend: None,
        })));

        // TLS Setup
        let tls_acceptor = if let Some(tls_config) = &rule.tls {
             if tls_config.enabled {
                 Some(Arc::new(crate::networking::tls::load_tls_config(&tls_config.cert, &tls_config.key)?))
             } else {
                 None
             }
        } else {
            None
        };

        // Create a socket2 TCP builder
        use socket2::{Socket, Domain, Type, Protocol};
        use std::net::SocketAddr;
        
        let addr: SocketAddr = rule.listen.parse().map_err(|e| anyhow::anyhow!("Invalid address: {}", e))?;
        
        
        // Spawn multiple acceptors (one per core is good for high ops)
        // Default to available parallelism or 4 if unknown.
        let default_acceptors = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
        let num_acceptors = std::env::var("NUM_ACCEPTORS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(default_acceptors);
        
        info!("Starting {} acceptors for rule: {}", num_acceptors, rule.name);

        for i in 0..num_acceptors {
            let rule_name = rule.name.clone();
            // let lb_clone = lb.clone(); // Unused here
            // let tls_acceptor = tls_acceptor.clone(); // Unused here 
            // let bw_clone = bandwidth_manager.clone();
            // let rl_clone = rate_limiter.clone();
            let backend_tls_config = rule.backend_tls.clone(); // Clone config for closure capture

            // Re-bind needs a new socket for each thread if using SO_REUSEPORT
            let socket = Socket::new(Domain::IPV4, Type::STREAM, Some(Protocol::TCP))?;
            
            #[cfg(unix)]
            {
                // socket.set_reuse_port(true)?; // socket2 might need feature "all" or specific handling
                // Manual setsockopt for SO_REUSEPORT (state 15 on linux, 0x0200 on mac?)
                // Actually socket2 has `set_reuse_port` if feature is enabled.
                // Creating socket2 dependency was "all".
                if let Err(e) = socket.set_reuse_port(true) {
                     warn!("Failed to set SO_REUSEPORT: {}", e);
                }
            }
            socket.set_reuse_address(true)?;
            socket.bind(&addr.into())?;
            socket.listen(1024)?; // Increased backlog

            let std_listener: std::net::TcpListener = socket.into();
            std_listener.set_nonblocking(true)?;

            let listener: TcpListener = match TcpListener::from_std(std_listener) {
                Ok(l) => l,
                Err(e) => {
                    error!("Failed to convert to tokio listener: {}", e);
                    continue;
                }
            };

            // Use the outer tls_acceptor

            
            // let tls_acceptor_clone = tls_acceptor.clone(); // No, TlsAcceptor is Arc internally usually, but here we can clone it. 
            // Actually TlsAcceptor is cheap to clone (Arc).

            info!("Spawning acceptor {}/{} for rule '{}' on {}", i+1, num_acceptors, rule_name, addr);

            let lb_clone = lb.clone();
            let bw_clone = bandwidth_manager.clone();
            let rl_clone = rate_limiter.clone();
            let r_name_clone = rule_name.clone();
            let tls_clone = tls_acceptor.clone(); // tokio_rustls::TlsAcceptor is cheap to clone
            let backend_tls_clone = backend_tls_config.clone();

            tokio::spawn(async move {
                loop {
                     match listener.accept().await {
                        Ok((stream, client_addr)) => {
                            // Rate Limit
                             if !rl_clone.check(client_addr.ip()) {
                                continue;
                            }
                            
                            let lb = lb_clone.clone();
                            let bw = bw_clone.clone();
                            let r_name = r_name_clone.clone();
                            let tls = tls_clone.clone();
                            let b_tls = backend_tls_clone.clone(); // Clone for this connection

                            tokio::spawn(async move {
                                // ... existing proxy logic ...
                                // Select Backend
                                let backend = match lb.next_backend() {
                                    Some(b) => b,
                                    None => {
                                        // error!("[{}] No available backends", r_name);
                                        return;
                                    }
                                };
                                let (backend_addr, _guard) = backend;

                                // Bandwidth Limiters
                                let proxy_config = ProxyConfig {
                                    client_read_limiter: bw.get_client_upload_limiter(client_addr.ip()),
                                    client_write_limiter: bw.get_client_download_limiter(client_addr.ip()),
                                    backend_read_limiter: bw.get_backend_download_limiter(client_addr.ip().to_string()), 
                                    backend_write_limiter: bw.get_backend_upload_limiter(client_addr.ip().to_string()),
                                    backend_tls: b_tls,
                                };

                                if let Some(acceptor) = tls {
                                    match acceptor.accept(stream).await {
                                        Ok(tls_stream) => {
                                            if let Err(_e) = proxy::proxy_connection(tls_stream, backend_addr, proxy_config).await {
                                                // error!("[{}] Proxy error: {}", r_name, e);
                                            }
                                         }
                                        Err(e) => error!("[{}] TLS handshake error: {}", r_name, e),
                                    }
                                } else {
                                    if let Err(_e) = proxy::proxy_connection(stream, backend_addr, proxy_config).await {
                                        // error!("[{}] Proxy error: {}", r_name, e);
                                    }
                                }
                            });
                        }
                        Err(e) => error!("Accept error: {}", e),
                     }
                }
            });
        }
    }


    // --- Cluster Setup ---
    // Channel for application to send commands to cluster
    let (_tx_cluster_cmd, rx_cluster_cmd) = mpsc::channel(100);
    // Channel for cluster to send state updates (node_id, key, usage)
    let (tx_cluster_state, mut rx_cluster_state) = mpsc::channel(1000);

    if let Some(cluster_config) = &config.cluster {
        if cluster_config.enabled {
            info!("Initializing Cluster on {}", cluster_config.bind_addr);
            let bind_addr = cluster_config.bind_addr.parse().expect("Invalid cluster bind address");
            let seeds: Vec<std::net::SocketAddr> = cluster_config.peers.iter()
                .map(|s| s.parse().expect("Invalid seed address"))
                .collect();
            
            match cluster::Cluster::new(bind_addr, seeds.clone(), rx_cluster_cmd, tx_cluster_state).await {
                Ok(cluster) => {
                    tokio::spawn(async move {
                        cluster.run(seeds).await;
                    });
                    info!("Cluster started.");
                }
                Err(e) => error!("Failed to start cluster: {}", e),
            }
        }
    }
    
    // Spawn a task to handle cluster state updates (placeholder for now)
    tokio::spawn(async move {
        while let Some((node_id, key, usage)) = rx_cluster_state.recv().await {
            info!("Cluster Update: Node {} Key {} Usage {}", node_id, key, usage);
            // TODO: Update global rate limiter
        }
    });


    // 3. Setup Config Watcher (Hot Reload)
    let (tx, mut rx) = mpsc::channel(1);
    let config_path = args.config.clone();
    
    let mut watcher = RecommendedWatcher::new(move |res: Result<Event, notify::Error>| {
        match res {
            Ok(event) => {
                if event.kind.is_modify() {
                    let _ = tx.blocking_send(());
                }
            },
            Err(e) => error!("Watch error: {:?}", e),
        }
    }, notify::Config::default())?;

    watcher.watch(&config_path, RecursiveMode::NonRecursive)?;
    info!("Watching config file for changes...");

    // Main loop: wait for config updates
    while let Some(_) = rx.recv().await {
        info!("Config change detected, reloading...");
        match std::fs::read_to_string(&config_path) {
            Ok(content) => {
                match serde_yaml::from_str::<Config>(&content) {
                    Ok(new_config) => {
                        // Reconcile rules
                        let lbs_read = lbs.read().await;
                        for rule in new_config.rules {
                            if let Some(lb) = lbs_read.get(&rule.name) {
                                info!("Updating backends for rule '{}'", rule.name);
                                lb.update_backends(rule.backends.clone()).await;
                                
                                // Spawn health checks for new backends (NOTE: this duplicates checkers for existing backends)
                                if let Some(hc_config) = &rule.health_check {
                                     for backend_addr in &rule.backends {
                                         health::start_health_check(lb.clone(), backend_addr.clone(), hc_config.clone());
                                     }
                                }
                            } else {
                                warn!("New rule '{}' detected but dynamic listener spawning is not yet supported. Restart required.", rule.name);
                            }
                        }
                    }
                    Err(e) => error!("Failed to parse new config: {}", e),
                }
            },
            Err(e) => error!("Failed to read config file: {}", e),
        }
    }

    Ok(())
}
