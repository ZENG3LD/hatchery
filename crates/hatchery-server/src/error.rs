use thiserror::Error;

#[derive(Debug, Error)]
pub enum ServerError {
    #[error("configuration: {0}")]
    Config(String),
    #[error("subservice {name}: {message}")]
    Subservice { name: &'static str, message: String },
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

impl ServerError {
    pub fn sub(name: &'static str, message: impl Into<String>) -> Self {
        Self::Subservice { name, message: message.into() }
    }
}
