use governor::{Quota, RateLimiter as GovernorLimiter};
use governor::state::{InMemoryState, NotKeyed};
use governor::clock::DefaultClock;
use std::num::NonZeroU32;
use dashmap::DashMap;
use std::net::IpAddr;
use std::sync::Arc;
use crate::config::RateLimitConfig;
use crate::config::BandwidthLimitConfig;
use log::warn;

// Make this public so bandwidth.rs can see it
pub type RateLimiterType = GovernorLimiter<NotKeyed, InMemoryState, DefaultClock>;

#[derive(Clone)]
pub struct RateLimiter {
    // Map IP address to a rate limiter instance
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

        // Get or create limiter for this IP
        let limiter = self.limiters.entry(ip).or_insert_with(|| {
            let quota = Quota::per_second(NonZeroU32::new(self.config.requests_per_second).unwrap_or(NonZeroU32::new(1).unwrap()))
                .allow_burst(NonZeroU32::new(self.config.burst).unwrap_or(NonZeroU32::new(1).unwrap()));
            Arc::new(GovernorLimiter::direct(quota))
        }).value().clone();

        match limiter.check() {
            Ok(_) => true,
            Err(_) => {
                warn!("Rate limit exceeded for IP: {}", ip);
                false
            }
        }
    }
}

// Reuse GovernorLimiter logic for bandwidth. 1 token = 1 byte.
// We need separate limiters for upload (read from client/backend) and download (write to client/backend).
#[derive(Clone)]
pub struct BandwidthManager {
    config: BandwidthLimitConfig,
    // Maps for different flows.
    // Client Upload: Client IP -> Limiter
    client_upload: Arc<DashMap<IpAddr, Arc<RateLimiterType>>>,
    // Client Download: Client IP -> Limiter
    client_download: Arc<DashMap<IpAddr, Arc<RateLimiterType>>>,
    // Backend Upload: Backend IP (String for now) -> Limiter
    backend_upload: Arc<DashMap<String, Arc<RateLimiterType>>>,
    // Backend Download: Backend IP (String for now) -> Limiter
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



    // Helper to get or create a limiter
    fn get_limiter(&self, map: &Arc<DashMap<String, Arc<RateLimiterType>>>, key: String, rate_per_sec: u32) -> Arc<RateLimiterType> {
         map.entry(key.clone()).or_insert_with(|| {
            // println!("Creating new limiter for {} with rate {}", key, rate_per_sec); // Removed dead code
            // Burst size matters for smooth streaming. Let's say burst = rate (1 second buffer)
            let burst = rate_per_sec;
             let quota = Quota::per_second(NonZeroU32::new(rate_per_sec).unwrap_or(NonZeroU32::new(1024).unwrap()))
                .allow_burst(NonZeroU32::new(burst).unwrap_or(NonZeroU32::new(1024).unwrap()));
            Arc::new(GovernorLimiter::direct(quota))
        }).value().clone()
    }

    pub fn get_client_upload_limiter(&self, ip: IpAddr) -> Option<Arc<RateLimiterType>> {
        let limits = self.config.client.as_ref()?;
        Some(self.client_upload.entry(ip).or_insert_with(|| {
             let rate = limits.upload_per_sec;
             println!("Creating new Client Upload limiter for {} with rate {}", ip, rate);
             let quota = Quota::per_second(NonZeroU32::new(rate).unwrap_or(NonZeroU32::new(1024).unwrap()))
                .allow_burst(NonZeroU32::new(rate).unwrap_or(NonZeroU32::new(1024).unwrap()));
            Arc::new(GovernorLimiter::direct(quota))
        }).value().clone())
    }

    pub fn get_client_download_limiter(&self, ip: IpAddr) -> Option<Arc<RateLimiterType>> {
        let limits = self.config.client.as_ref()?;
        Some(self.client_download.entry(ip).or_insert_with(|| {
             let rate = limits.download_per_sec;
             let quota = Quota::per_second(NonZeroU32::new(rate).unwrap_or(NonZeroU32::new(1024).unwrap()))
                .allow_burst(NonZeroU32::new(rate).unwrap_or(NonZeroU32::new(1024).unwrap()));
            Arc::new(GovernorLimiter::direct(quota))
        }).value().clone())
    }

    pub fn get_backend_upload_limiter(&self, addr: String) -> Option<Arc<RateLimiterType>> {
        let limits = self.config.backend.as_ref()?;
        Some(self.get_limiter(&self.backend_upload, addr, limits.upload_per_sec))
    }

    pub fn get_backend_download_limiter(&self, addr: String) -> Option<Arc<RateLimiterType>> {
        let limits = self.config.backend.as_ref()?;
        Some(self.get_limiter(&self.backend_download, addr, limits.download_per_sec))
    }
}
