use lazy_static::lazy_static;
use prometheus::{
    register_gauge_vec, register_int_counter_vec, register_histogram_vec,
    GaugeVec, IntCounterVec, HistogramVec
};

lazy_static! {
    // --- Rule Level Metrics ---
    pub static ref ACTIVE_CONNECTIONS: GaugeVec = register_gauge_vec!(
        "l4lb_active_connections",
        "Current number of active connections per rule",
        &["rule_name"]
    ).unwrap();

    pub static ref TOTAL_CONNECTIONS: IntCounterVec = register_int_counter_vec!(
        "l4lb_connections_total",
        "Total number of connections accepted",
        &["rule_name"]
    ).unwrap();

    // --- Traffic Metrics ---
    // incoming traffic: client -> lb -> backend
    // outgoing traffic: backend -> lb -> client
    // We track bytes received/sent from the LB's perspective relative to the client/backend stream.
    // simpler: "direction" = "client_in", "client_out", "backend_in", "backend_out"
    pub static ref TRAFFIC_BYTES: IntCounterVec = register_int_counter_vec!(
        "l4lb_traffic_bytes_total",
        "Total bytes transferred",
        &["rule_name", "direction"]
    ).unwrap();

    // --- Backend Metrics ---
    pub static ref BACKEND_ACTIVE_CONNECTIONS: GaugeVec = register_gauge_vec!(
        "l4lb_backend_active_connections",
        "Active connections to a specific backend",
        &["rule_name", "backend_addr"]
    ).unwrap();

    pub static ref BACKEND_HEALTH_STATUS: GaugeVec = register_gauge_vec!(
        "l4lb_backend_health_status",
        "Health status of backend (1 = healthy, 0 = unhealthy)",
        &["rule_name", "backend_addr"]
    ).unwrap();

    // --- Latency (P95, P99, etc. calculated by histogram) ---
    pub static ref CONNECTION_DURATION: HistogramVec = register_histogram_vec!(
        "l4lb_connection_duration_seconds",
        "Duration of connections in seconds",
        &["rule_name"],
        vec![0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0, 10.0, 30.0, 60.0, 300.0] 
    ).unwrap();
}

use hyper::{Request, Response, StatusCode};
use http_body_util::Full;
use bytes::Bytes;
use prometheus::{Encoder, TextEncoder};

// ... existing lazy_static ...

pub async fn metrics_handler(_req: Request<hyper::body::Incoming>) -> Result<Response<Full<Bytes>>, hyper::Error> {
    let encoder = TextEncoder::new();
    let metric_families = prometheus::gather();
    let mut buffer = vec![];
    
    if let Err(e) = encoder.encode(&metric_families, &mut buffer) {
        // error!("Metrics encoding error: {}", e);
        return Ok(Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(Full::new(Bytes::from(format!("Error: {}", e))))
            .unwrap());
    }

    Ok(Response::builder()
        .header("Content-Type", encoder.format_type())
        .body(Full::new(Bytes::from(buffer)))
        .unwrap())
}
