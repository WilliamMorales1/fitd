
use thiserror::Error;

#[derive(Error, Debug)]
#[error("{0}")]
pub struct FitdError(String);

impl From<String> for FitdError {
    fn from(message: String) -> Self {
        Self(message)
    }
}

impl From<&str> for FitdError {
    fn from(message: &str) -> Self {
        Self(message.into())
    }
}

pub type FitdResult<T> = Result<T, FitdError>;
