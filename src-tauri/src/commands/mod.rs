pub mod activate;
pub mod history;
pub mod importers;
pub mod profiles;
pub mod settings;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum CommandError {
    #[error("{0}")]
    Generic(String),
}

impl From<anyhow::Error> for CommandError {
    fn from(e: anyhow::Error) -> Self {
        Self::Generic(format!("{e:#}"))
    }
}

impl From<crate::fs::FsError> for CommandError {
    fn from(e: crate::fs::FsError) -> Self {
        Self::Generic(e.to_string())
    }
}

impl From<std::io::Error> for CommandError {
    fn from(e: std::io::Error) -> Self {
        Self::Generic(e.to_string())
    }
}

impl From<serde_json::Error> for CommandError {
    fn from(e: serde_json::Error) -> Self {
        Self::Generic(format!("json: {e}"))
    }
}

impl serde::Serialize for CommandError {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

pub type CmdResult<T> = Result<T, CommandError>;
