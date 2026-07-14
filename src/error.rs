use thiserror::Error;

#[derive(Debug, Error)]
pub enum BleatError {
    #[error("{0}")]
    Usage(String),
    #[error("{0}")]
    Runtime(String),
}

impl BleatError {
    pub const fn exit_code(&self) -> u8 {
        match self {
            Self::Usage(_) => 2,
            Self::Runtime(_) => 1,
        }
    }
}
