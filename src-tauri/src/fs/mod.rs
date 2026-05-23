pub mod atomic;
pub mod merge;
pub mod paths;

#[doc(hidden)]
pub use atomic::write_temp_only;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum FsError {
    #[error("home directory not found")]
    NoHome,
    #[error("io error at {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid path: {0}")]
    InvalidPath(String),
}

impl FsError {
    pub fn io(path: impl Into<String>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}
