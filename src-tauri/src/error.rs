use serde::Serialize;
use thiserror::Error;
use ts_rs::TS;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("{message}")]
    User {
        code: String,
        message: String,
        details: Option<String>,
    },
    #[error("Database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Serialization error: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct AppErrorPayload {
    pub code: String,
    pub message: String,
    pub details: Option<String>,
}

impl AppError {
    pub fn user(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::User {
            code: code.into(),
            message: message.into(),
            details: None,
        }
    }

    pub fn with_details(
        code: impl Into<String>,
        message: impl Into<String>,
        details: impl Into<String>,
    ) -> Self {
        Self::User {
            code: code.into(),
            message: message.into(),
            details: Some(details.into()),
        }
    }

    pub fn to_payload(&self) -> AppErrorPayload {
        match self {
            AppError::User {
                code,
                message,
                details,
            } => AppErrorPayload {
                code: code.clone(),
                message: message.clone(),
                details: details.clone(),
            },
            AppError::Db(e) => AppErrorPayload {
                code: "DB_ERROR".to_string(),
                message: "A local database error occurred.".to_string(),
                details: Some(e.to_string()),
            },
            AppError::Io(e) => AppErrorPayload {
                code: "IO_ERROR".to_string(),
                message: "A file or process operation failed.".to_string(),
                details: Some(e.to_string()),
            },
            AppError::Json(e) => AppErrorPayload {
                code: "JSON_ERROR".to_string(),
                message: "Failed to serialize or parse structured data.".to_string(),
                details: Some(e.to_string()),
            },
        }
    }
}

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.to_payload().serialize(serializer)
    }
}

pub type AppResult<T> = Result<T, AppError>;
