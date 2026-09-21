use serde::Serialize;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error, Serialize)]
#[serde(tag = "kind", content = "message", rename_all = "snake_case")]
pub enum Error {
    #[error("invalid configuration: {0}")]
    InvalidConfig(String),
    #[error("detection failed: {0}")]
    Detection(String),
    #[error("conversion failed: {0}")]
    Conversion(String),
    #[error("constraint cannot be satisfied: {0}")]
    Constraint(String),
    #[error("cancelled")]
    Cancelled,
    #[error("I/O error: {0}")]
    Io(String),
}
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.to_string())
    }
}
pub(crate) fn invalid(s: impl Into<String>) -> Error {
    Error::InvalidConfig(s.into())
}
