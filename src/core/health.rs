use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::time::{sleep, Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use log::{debug, info};
use crate::config::HealthCheckConfig;
use crate::core::balancer::LoadBalancer;

pub fn start_health_check(
    lb: Arc<LoadBalancer>,
    backend_addr: String,
    config: HealthCheckConfig,
) {
    let config = config.clone();
    tokio::spawn(async move {
        // Initial delay to let things start?
        sleep(Duration::from_millis(100)).await;
        
        info!("Starting health check for {} ({})", backend_addr, config.protocol);

        loop {
            let timeout = Duration::from_millis(config.timeout_ms);
            let check_res = match config.protocol.as_str() {
                "http" => {
                    let path = config.path.as_deref().unwrap_or("/");
                    check_http(&backend_addr, path, timeout).await
                },
                _ => check_tcp(&backend_addr, timeout).await,
            };

            lb.set_backend_health(&backend_addr, check_res).await;

            sleep(Duration::from_millis(config.interval_ms)).await;
        }
    });
}

async fn check_tcp(addr: &str, timeout: Duration) -> bool {
    let start = std::time::Instant::now();
    let connect = TcpStream::connect(addr);
    match tokio::time::timeout(timeout, connect).await {
        Ok(Ok(_)) => {
            debug!("TCP check passed for {} in {:?}", addr, start.elapsed());
            true
        },
        Ok(Err(e)) => {
            debug!("TCP check failed for {}: {} (took {:?})", addr, e, start.elapsed());
            false
        },
        Err(_) => {
            debug!("TCP check timed out for {} (after {:?})", addr, start.elapsed());
            false
        }
    }
}

async fn check_http(addr: &str, path: &str, timeout: Duration) -> bool {
    let check_fut = async {
        match TcpStream::connect(addr).await {
            Ok(mut stream) => {
                let request = format!("GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n", path, addr);
                if let Err(e) = stream.write_all(request.as_bytes()).await {
                    debug!("HTTP write failed for {}: {}", addr, e);
                    return false;
                }

                let mut buf = [0u8; 1024];
                match stream.read(&mut buf).await {
                    Ok(n) if n > 0 => {
                        let response = String::from_utf8_lossy(&buf[..n]);
                        if response.contains("200 OK") {
                            true
                        } else {
                            debug!("HTTP check failed for {}: Status not 200", addr);
                            false
                        }
                    }
                    Ok(_) => false,
                    Err(e) => {
                        debug!("HTTP read failed for {}: {}", addr, e);
                        false
                    }
                }
            }
            Err(e) => {
                debug!("HTTP Connect failed for {}: {}", addr, e);
                false
            }
        }
    };

    match tokio::time::timeout(timeout, check_fut).await {
        Ok(res) => res,
        Err(_) => {
            debug!("HTTP check timed out for {}", addr);
            false
        }
    }
}
