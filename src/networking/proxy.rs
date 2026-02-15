use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use log::debug;
use std::sync::Arc;
use crate::traffic::bandwidth::RateLimitedStream;
use crate::traffic::limiter::RateLimiterType;
use crate::config::BackendTlsConfig;
use anyhow::Result;
use tokio_rustls::TlsConnector;
use rustls::pki_types::ServerName;
use rustls::{ClientConfig, RootCertStore};
use webpki_roots;
use std::net::SocketAddr;

pub struct ProxyConfig {
    pub client_read_limiter: Option<Arc<RateLimiterType>>,
    pub client_write_limiter: Option<Arc<RateLimiterType>>,
    pub backend_read_limiter: Option<Arc<RateLimiterType>>,
    pub backend_write_limiter: Option<Arc<RateLimiterType>>,
    pub backend_tls: Option<BackendTlsConfig>,
    pub proxy_protocol: bool,
    pub client_addr: SocketAddr,
    pub local_addr: SocketAddr,
}

pub async fn proxy_connection<I>(
    client_stream: I,
    backend_addr: String,
    config: ProxyConfig,
    rule_name: String, // Added rule_name for metrics
) -> Result<()>
where
    I: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let start_time = std::time::Instant::now();
    
    // Metrics: Increment Active & Total
    crate::metrics::ACTIVE_CONNECTIONS.with_label_values(&[&rule_name]).inc();
    crate::metrics::TOTAL_CONNECTIONS.with_label_values(&[&rule_name]).inc();

    // Guard to decrement active connections on drop (ensure it runs even on error)
    struct ConnectionMetricGuard {
        rule_name: String,
    }
    
    impl Drop for ConnectionMetricGuard {
        fn drop(&mut self) {
            crate::metrics::ACTIVE_CONNECTIONS.with_label_values(&[&self.rule_name]).dec();
        }
    }
    
    let _metric_guard = ConnectionMetricGuard { rule_name: rule_name.clone() };

    // Connect to backend (TCP)
    let mut backend_stream = TcpStream::connect(&backend_addr).await?;
    if let Err(e) = backend_stream.set_nodelay(true) {
        debug!("Failed to set nodelay on backend stream: {}", e);
    }

    // Send Proxy Protocol Header if enabled
    if config.proxy_protocol {
        let header = crate::networking::proxy_protocol::create_v2_header(config.client_addr, config.local_addr);
        backend_stream.write_all(&header).await?;
        debug!("Sent Proxy Protocol v2 header to {}", backend_addr);
    }
    
    // ... TLS handling logic ... (simplified for brevity match structure in original)
    // We need to match the original structure. I'll paste the full updated function body.
    
    // Handle Backend TLS if enabled
    if let Some(tls_cfg) = config.backend_tls {
        if tls_cfg.enabled {
             // ... TLS logic ...
             // Replicating internal logic for TLS path to include metrics at end
             debug!("Starting TLS handshake with backend {}", backend_addr);
             
             let mut root_store = RootCertStore::empty();
             root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
             let mut client_config = ClientConfig::builder()
                .with_root_certificates(root_store)
                .with_no_client_auth();
             if tls_cfg.ignore_verify {
                client_config.dangerous().set_certificate_verifier(Arc::new(NoVerify));
             }
             let connector = TlsConnector::from(Arc::new(client_config));
             let domain = ServerName::try_from("localhost").unwrap().to_owned(); 
             let tls_stream = connector.connect(domain, backend_stream).await?;

             let mut backend_stream_limited = RateLimitedStream::new(tls_stream, config.backend_read_limiter, config.backend_write_limiter);
             let mut client_stream_limited = RateLimitedStream::new(client_stream, config.client_read_limiter, config.client_write_limiter);

             let (c2b, b2c) = tokio::io::copy_bidirectional(&mut client_stream_limited, &mut backend_stream_limited).await?;

             // Record Traffic & Duration
             crate::metrics::TRAFFIC_BYTES.with_label_values(&[&rule_name, "client_in"]).inc_by(c2b);
             crate::metrics::TRAFFIC_BYTES.with_label_values(&[&rule_name, "backend_out"]).inc_by(c2b); // sent to backend
             crate::metrics::TRAFFIC_BYTES.with_label_values(&[&rule_name, "backend_in"]).inc_by(b2c);
             crate::metrics::TRAFFIC_BYTES.with_label_values(&[&rule_name, "client_out"]).inc_by(b2c); // sent to client
             crate::metrics::CONNECTION_DURATION.with_label_values(&[&rule_name]).observe(start_time.elapsed().as_secs_f64());

             debug!("TLS Connection closed. Client sent: {} bytes, Backend sent: {} bytes", c2b, b2c);
             return Ok(());
        }
    }
    
    // Plain TCP
    let mut backend_stream_limited = RateLimitedStream::new(backend_stream, config.backend_read_limiter, config.backend_write_limiter);
    let mut client_stream_limited = RateLimitedStream::new(client_stream, config.client_read_limiter, config.client_write_limiter);

    let (c2b, b2c) = tokio::io::copy_bidirectional(&mut client_stream_limited, &mut backend_stream_limited).await?;
    
    // Record Traffic & Duration
    crate::metrics::TRAFFIC_BYTES.with_label_values(&[&rule_name, "client_in"]).inc_by(c2b);
    crate::metrics::TRAFFIC_BYTES.with_label_values(&[&rule_name, "backend_out"]).inc_by(c2b);
    crate::metrics::TRAFFIC_BYTES.with_label_values(&[&rule_name, "backend_in"]).inc_by(b2c);
    crate::metrics::TRAFFIC_BYTES.with_label_values(&[&rule_name, "client_out"]).inc_by(b2c);
    crate::metrics::CONNECTION_DURATION.with_label_values(&[&rule_name]).observe(start_time.elapsed().as_secs_f64());

    debug!("Connection closed. Client sent: {} bytes, Backend sent: {} bytes", c2b, b2c);

    Ok(())
}

#[derive(Debug)]
struct NoVerify;

impl rustls::client::danger::ServerCertVerifier for NoVerify {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }
    
    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }
    
    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        vec![
            rustls::SignatureScheme::RSA_PKCS1_SHA1,
            rustls::SignatureScheme::ECDSA_SHA1_Legacy,
            rustls::SignatureScheme::RSA_PKCS1_SHA256,
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            rustls::SignatureScheme::RSA_PKCS1_SHA384,
            rustls::SignatureScheme::ECDSA_NISTP384_SHA384,
            rustls::SignatureScheme::RSA_PKCS1_SHA512,
            rustls::SignatureScheme::ECDSA_NISTP521_SHA512,
            rustls::SignatureScheme::RSA_PSS_SHA256,
            rustls::SignatureScheme::RSA_PSS_SHA384,
            rustls::SignatureScheme::RSA_PSS_SHA512,
            rustls::SignatureScheme::ED25519,
            rustls::SignatureScheme::ED448,
        ]
    }
}
