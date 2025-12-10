use std::io;

#[derive(thiserror::Error, Debug)]
pub enum DlrsError {
    #[error("aria2c not found in PATH")]
    Aria2cNotFound,

    #[error("{message}")]
    DownloadFailed {
        message: String,
        exit_code: Option<i32>,
        recoverable: bool,
    },

    #[error("download cancelled")]
    Cancelled,

    #[error("invalid URL: {0}")]
    InvalidUrl(String),

    #[error("destination error: {0}")]
    DestinationError(String),

    #[allow(dead_code)]
    #[error("file verification failed: expected {expected}B, got {actual}B")]
    VerificationFailed { expected: u64, actual: u64 },

    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    #[error("{0}")]
    Other(String),
}

impl DlrsError {
    pub fn is_transient(&self) -> bool {
        matches!(
            self,
            Self::DownloadFailed {
                recoverable: true,
                ..
            }
        )
    }

    pub fn download_failed(exit_code: i32) -> Self {
        let (message, recoverable) = match exit_code {
            1 => ("size mismatch (may be false positive)".into(), true),
            3 => ("file not found or access denied".into(), false),
            9 => ("not enough disk space".into(), false),
            22 => ("server rejected request (403/range issue)".into(), true),
            28 => ("network timeout".into(), true),
            _ => (format!("aria2c exit code {exit_code}"), false),
        };
        Self::DownloadFailed {
            message,
            exit_code: Some(exit_code),
            recoverable,
        }
    }
}
