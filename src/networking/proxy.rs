use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use log::{info, debug};
use std::sync::Arc;
use crate::traffic::bandwidth::RateLimitedStream;
use crate::traffic::limiter::RateLimiterType;
use crate::config::BackendTlsConfig;
use anyhow::Result;
use tokio_rustls::TlsConnector;
use rustls::pki_types::ServerName;
use rustls::{ClientConfig, RootCertStore};
use webpki_roots;

pub struct ProxyConfig {
    pub client_read_limiter: Option<Arc<RateLimiterType>>,
    pub client_write_limiter: Option<Arc<RateLimiterType>>,
    pub backend_read_limiter: Option<Arc<RateLimiterType>>,
    pub backend_write_limiter: Option<Arc<RateLimiterType>>,
    pub backend_tls: Option<BackendTlsConfig>,
}

pub async fn proxy_connection<I>(
    mut client_stream: I,
    backend_addr: String,
    config: ProxyConfig,
) -> Result<()>
where
    I: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    // Connect to backend (TCP)
    let backend_stream = TcpStream::connect(&backend_addr).await?;
    
    // Handle Backend TLS if enabled
    if let Some(tls_cfg) = config.backend_tls {
        if tls_cfg.enabled {
            debug!("Starting TLS handshake with backend {}", backend_addr);
            
            // Configure TLS Client
            let mut root_store = RootCertStore::empty();
            root_store.extend(
                webpki_roots::TLS_SERVER_ROOTS
                    .iter()
                    .cloned()
            );

            let mut client_config = ClientConfig::builder()
                .with_root_certificates(root_store)
                .with_no_client_auth();

            if tls_cfg.ignore_verify {
                // Insecure: verify nothing
                client_config.dangerous().set_certificate_verifier(Arc::new(NoVerify));
            }

            let connector = TlsConnector::from(Arc::new(client_config));
            let domain = ServerName::try_from("localhost").unwrap().to_owned(); 
            
            let tls_stream = connector.connect(domain, backend_stream).await?;
            
            // Wrap in Bandwidth Limiter
            let mut backend_stream_limited = RateLimitedStream::new(
                tls_stream,
                config.backend_read_limiter, // Read from backend (Download)
                config.backend_write_limiter // Write to backend (Upload)
            );
            
            let mut client_stream_limited = RateLimitedStream::new(
                client_stream,
                config.client_read_limiter, // Read from client (Upload)
                config.client_write_limiter // Write to client (Download)
            );

            let (mut c_read, mut c_write) = tokio::io::split(&mut client_stream_limited);
            let (mut b_read, mut b_write) = tokio::io::split(&mut backend_stream_limited);

            let client_to_backend = tokio::io::copy(&mut c_read, &mut b_write);
            let backend_to_client = tokio::io::copy(&mut b_read, &mut c_write);

            let (client_to_backend_bytes, backend_to_client_bytes) = tokio::try_join!(client_to_backend, backend_to_client)?;

            info!(
                "TLS Connection closed. Client sent: {} bytes, Backend sent: {} bytes",
                client_to_backend_bytes, backend_to_client_bytes
            );
            return Ok(());
        }
    }
    
    // Plain TCP
    let mut backend_stream_limited = RateLimitedStream::new(
        backend_stream,
        config.backend_read_limiter, 
        config.backend_write_limiter 
    );

    let mut client_stream_limited = RateLimitedStream::new(
        client_stream,
        config.client_read_limiter,
        config.client_write_limiter
    );

    let (mut c_read, mut c_write) = tokio::io::split(&mut client_stream_limited);
    let (mut b_read, mut b_write) = tokio::io::split(&mut backend_stream_limited);

    let client_to_backend = tokio::io::copy(&mut c_read, &mut b_write);
    let backend_to_client = tokio::io::copy(&mut b_read, &mut c_write);

    let (client_to_backend_bytes, backend_to_client_bytes) = tokio::try_join!(client_to_backend, backend_to_client)?;
    
    info!(
        "Connection closed. Client sent: {} bytes, Backend sent: {} bytes",
        client_to_backend_bytes, backend_to_client_bytes
    );

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
