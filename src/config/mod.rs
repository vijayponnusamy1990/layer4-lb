use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Missing configuration: {0}")]
    MissingField(String),
    #[error("Invalid configuration: {0}")]
    InvalidValue(String),
}

#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct Config {
    // List of Load Balancing Rules
    pub rules: Vec<LBRule>,
    
    // Cluster Configuration (Optional)
    pub cluster: Option<ClusterConfig>,
    
    // Logging Configuration (Optional)
    pub log: Option<LogConfig>,
}

#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct LogConfig {
    pub level: String,
}

#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct ClusterConfig {
    pub enabled: bool,
    pub bind_addr: String, // e.g., "0.0.0.0:9090"
    pub peers: Vec<String>, // Seed peers e.g. ["10.0.0.2:9090"]
}

#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct LBRule {
    pub name: String,
    pub listen: String, // e.g., "0.0.0.0:8080"
    pub backends: Vec<String>,
    pub protocol: Option<String>, // Default TCP
    
    // Per-rule configurations
    pub tls: Option<TlsConfig>,
    pub backend_tls: Option<BackendTlsConfig>,
    pub rate_limit: Option<RateLimitConfig>,
    pub bandwidth_limit: Option<BandwidthLimitConfig>,
    pub backend_connection_limit: Option<usize>,
    pub health_check: Option<HealthCheckConfig>,
}

#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct HealthCheckConfig {
    pub enabled: bool,
    pub interval_ms: u64,
    pub timeout_ms: u64,
    pub protocol: String, // "tcp" or "http"
    pub path: Option<String>, // for http
}

#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct TlsConfig {
    pub enabled: bool,
    pub cert: String,
    pub key: String,
}

#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct RateLimitConfig {
    pub enabled: bool,
    pub requests_per_second: u32,
    pub burst: u32,
}

#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct BandwidthLimitConfig {
    pub enabled: bool,
    pub client: Option<ClientBandwidthConfig>,
    pub backend: Option<BackendBandwidthConfig>,
}

#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct ClientBandwidthConfig {
    pub upload_per_sec: u32,
    pub download_per_sec: u32,
}

#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct BackendBandwidthConfig {
    pub upload_per_sec: u32,
    pub download_per_sec: u32,
}

#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct BackendTlsConfig {
    pub enabled: bool,
    #[serde(default)]
    pub ignore_verify: bool,
}

impl Config {
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.rules.is_empty() {
             return Err(ConfigError::MissingField("rules are empty".to_string()));
        }
        for (i, rule) in self.rules.iter().enumerate() {
            if rule.backends.is_empty() {
                return Err(ConfigError::InvalidValue(format!("Rule '{}' (index {}) has no backends", rule.name, i)));
            }
            if rule.listen.is_empty() {
                 return Err(ConfigError::InvalidValue(format!("Rule '{}' has no listen address", rule.name)));
            }
        }
        Ok(())
    }
}
