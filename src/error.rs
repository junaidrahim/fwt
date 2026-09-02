use std::process::ExitStatus;

use thiserror::Error;

pub type Result<T> = std::result::Result<T, FwtError>;

#[derive(Debug, Error)]
pub enum FwtError {
    #[error("{0}")]
    Validation(String),

    #[error("{program} failed{status}: {message}")]
    Underlying {
        program: String,
        status: StatusDisplay,
        message: String,
    },

    #[error("{context}: {source}")]
    Io {
        context: String,
        #[source]
        source: std::io::Error,
    },

    #[error("{context}: {source}")]
    Json {
        context: String,
        #[source]
        source: serde_json::Error,
    },

    #[error("{context}: {source}")]
    Yaml {
        context: String,
        #[source]
        source: serde_yaml::Error,
    },
}

#[derive(Debug)]
pub struct StatusDisplay(pub Option<i32>);

impl std::fmt::Display for StatusDisplay {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0 {
            Some(code) => write!(f, " (exit {code})"),
            None => write!(f, ""),
        }
    }
}

impl FwtError {
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::Underlying { .. } => 2,
            Self::Validation(_) | Self::Io { .. } | Self::Json { .. } | Self::Yaml { .. } => 1,
        }
    }

    pub fn io(context: impl Into<String>, source: std::io::Error) -> Self {
        Self::Io {
            context: context.into(),
            source,
        }
    }

    pub fn underlying(program: impl Into<String>, status: ExitStatus, stderr: &[u8]) -> Self {
        let message = String::from_utf8_lossy(stderr).trim().to_owned();
        Self::Underlying {
            program: program.into(),
            status: StatusDisplay(status.code()),
            message: if message.is_empty() {
                "no diagnostic output".to_owned()
            } else {
                message
            },
        }
    }

    pub fn underlying_message(program: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Underlying {
            program: program.into(),
            status: StatusDisplay(None),
            message: message.into(),
        }
    }

    pub fn command_start(program: impl Into<String>, error: std::io::Error) -> Self {
        let program = program.into();
        Self::underlying_message(
            program.clone(),
            format!("could not start {program}: {error}"),
        )
    }
}
