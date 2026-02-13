use thiserror::Error;
 
#[derive(Error, Debug)]
pub enum LbError {
    #[error("IO Error: {0}")]
    Io(#[from] std::io::Error),
    #[error("YAML Error: {0}")]
    Yaml(#[from] serde_yaml::Error),
    #[error("Anyhow Error: {0}")]
    Anyhow(#[from] anyhow::Error),
    #[error("TLS Error: {0}")]
    Tls(String),
}

pub type Result<T> = std::result::Result<T, LbError>;
