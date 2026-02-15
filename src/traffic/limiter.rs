// Custom Simple Limiter to debug Governor issues
use std::sync::Mutex;
use std::time::{Instant, Duration};
use tokio::time::sleep;
use std::sync::Arc;
use dashmap::DashMap;
use std::net::IpAddr;
use crate::config::RateLimitConfig;
use crate::config::BandwidthLimitConfig;

#[derive(Debug)]
pub struct SimpleLimiter {
    rate_per_sec: u32,
    burst_size: u32,
    state: Mutex<SimpleLimiterState>,
}

#[derive(Debug)]
struct SimpleLimiterState {
    tokens: f64,
    last_update: Instant,
}

impl SimpleLimiter {
    pub fn new(rate_per_sec: u32, burst_size: u32) -> Self {
        SimpleLimiter {
            rate_per_sec,
            burst_size,
            state: Mutex::new(SimpleLimiterState {
                tokens: burst_size as f64,
                last_update: Instant::now(),
            }),
        }
    }

    // Returns Ok if tokens consumed, Err if not enough
    pub fn check_n(&self, n: u32) -> Result<(), ()> {
        let mut state = self.state.lock().unwrap();
        self.refill(&mut state);

        if state.tokens >= n as f64 {
            state.tokens -= n as f64;
            Ok(())
        } else {
            Err(())
        }
    }

    // Async wait for tokens
    pub async fn until_n_ready(&self, n: u32) -> Result<(), ()> {
        loop {
            let wait_duration = {
                let mut state = self.state.lock().unwrap();
                self.refill(&mut state);
                if state.tokens >= n as f64 {
                    state.tokens -= n as f64;
                    return Ok(());
                }
                
                // Calculate time needed to get enough tokens
                let missing = (n as f64) - state.tokens;
                let seconds_needed = missing / (self.rate_per_sec as f64);
                Duration::from_secs_f64(seconds_needed)
            };

            // Sleep for the calculated duration (plus a tiny buffer to be safe?)
            // We just sleep and retry.
            sleep(wait_duration).await;
        }
    }

    fn refill(&self, state: &mut SimpleLimiterState) {
        let now = Instant::now();
        let elapsed = now.duration_since(state.last_update).as_secs_f64();
        let new_tokens = elapsed * self.rate_per_sec as f64;
        
        if new_tokens > 0.0 {
            state.tokens = (state.tokens + new_tokens).min(self.burst_size as f64);
            state.last_update = now;
        }
    }
}

pub type RateLimiterType = SimpleLimiter;

#[derive(Clone)]
pub struct RateLimiter {
    limiters: Arc<DashMap<IpAddr, Arc<RateLimiterType>>>,
    config: RateLimitConfig,
}

impl RateLimiter {
    pub fn new(config: RateLimitConfig) -> Self {
        RateLimiter {
            limiters: Arc::new(DashMap::new()),
            config,
        }
    }

    pub fn check(&self, ip: IpAddr) -> bool {
        if !self.config.enabled {
            return true;
        }
        
        let limiter = self.limiters.entry(ip).or_insert_with(|| {
            Arc::new(SimpleLimiter::new(
                self.config.requests_per_second.max(1),
                self.config.burst.max(1)
            ))
        }).value().clone();

        limiter.check_n(1).is_ok()
    }
}

#[derive(Clone)]
pub struct BandwidthManager {
    config: BandwidthLimitConfig,
    client_upload: Arc<DashMap<IpAddr, Arc<RateLimiterType>>>,
    client_download: Arc<DashMap<IpAddr, Arc<RateLimiterType>>>,
    backend_upload: Arc<DashMap<String, Arc<RateLimiterType>>>,
    backend_download: Arc<DashMap<String, Arc<RateLimiterType>>>,
}

impl BandwidthManager {
    pub fn new(config: BandwidthLimitConfig) -> Self {
        BandwidthManager {
            config,
            client_upload: Arc::new(DashMap::new()),
            client_download: Arc::new(DashMap::new()),
            backend_upload: Arc::new(DashMap::new()),
            backend_download: Arc::new(DashMap::new()),
        }
    }

    fn get_or_create_limiter<K: std::hash::Hash + Eq + Clone + std::fmt::Display>(
        map: &Arc<DashMap<K, Arc<RateLimiterType>>>, 
        key: K, 
        rate_per_sec: u32,
        context: &str
    ) -> Arc<RateLimiterType> {
        if let Some(limiter) = map.get(&key) {
            return limiter.clone();
        }

        map.entry(key.clone()).or_insert_with(|| {
            let burst = 65536; // 64KB buffer for smooth throttling 
            log::info!("Creating new SimpleLimiter for {} {} with rate {} B/s", context, key, rate_per_sec);
            Arc::new(SimpleLimiter::new(rate_per_sec.max(1024), burst))
        }).value().clone()
    }

    pub fn get_client_upload_limiter(&self, ip: IpAddr) -> Option<Arc<RateLimiterType>> {
        if !self.config.enabled { return None; }
        let limits = self.config.client.as_ref()?;
        Some(Self::get_or_create_limiter(&self.client_upload, ip, limits.upload_per_sec, "Client Upload"))
    }

    pub fn get_client_download_limiter(&self, ip: IpAddr) -> Option<Arc<RateLimiterType>> {
         if !self.config.enabled { return None; }
        let limits = self.config.client.as_ref()?;
        Some(Self::get_or_create_limiter(&self.client_download, ip, limits.download_per_sec, "Client Download"))
    }

    pub fn get_backend_upload_limiter(&self, key: String) -> Option<Arc<RateLimiterType>> {
        if !self.config.enabled { return None; }
        let limits = self.config.backend.as_ref()?;
        Some(Self::get_or_create_limiter(&self.backend_upload, key, limits.upload_per_sec, "Backend Upload"))
    }

    pub fn get_backend_download_limiter(&self, key: String) -> Option<Arc<RateLimiterType>> {
        if !self.config.enabled { return None; }
        let limits = self.config.backend.as_ref()?;
        Some(Self::get_or_create_limiter(&self.backend_download, key, limits.download_per_sec, "Backend Download"))
    }
}
