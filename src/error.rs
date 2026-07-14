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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_errors_exit_with_code_2() {
        let error = BleatError::Usage("bad input".into());

        assert_eq!(error.exit_code(), 2);
    }

    #[test]
    fn runtime_errors_exit_with_code_1() {
        let error = BleatError::Runtime("I/O failed".into());

        assert_eq!(error.exit_code(), 1);
    }
}
