use rustls::pki_types::PrivateKeyDer;
use rustls::ServerConfig;
use rustls_pemfile::{certs, pkcs8_private_keys};
use std::fs::File;
use std::io::BufReader;
use std::sync::Arc;
use tokio_rustls::TlsAcceptor;
use crate::common::error::{LbError, Result};

pub fn load_tls_config(cert_path: &str, key_path: &str) -> Result<TlsAcceptor> {
    let cert_file = File::open(cert_path).map_err(LbError::Io)?;
    let mut cert_reader = BufReader::new(cert_file);
    let certs = certs(&mut cert_reader)
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(LbError::Io)?;

    let key_file = File::open(key_path).map_err(LbError::Io)?;
    let mut key_reader = BufReader::new(key_file);
    let mut keys = pkcs8_private_keys(&mut key_reader)
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(LbError::Io)?;

    if keys.is_empty() {
        return Err(LbError::Tls("No private keys found".to_string()));
    }

    let key = PrivateKeyDer::Pkcs8(keys.remove(0));

    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| LbError::Tls(e.to_string()))?;

    Ok(TlsAcceptor::from(Arc::new(config)))
}
