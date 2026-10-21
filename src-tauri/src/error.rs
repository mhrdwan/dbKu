use serde::{Serialize, Serializer};

/// Every failure that can cross the IPC boundary. It serializes to
/// `{ kind, message }` so the UI can branch on `kind` and show `message`.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    Database(String),
    #[error("Not connected: {0}")]
    NotConnected(String),
    #[error("{0}")]
    Invalid(String),
    #[error("Blocked by read-only mode: {0}")]
    ReadOnly(String),
    #[error("{0}")]
    Storage(String),
    #[error("Keychain error: {0}")]
    Secret(String),
}

impl AppError {
    fn kind(&self) -> &'static str {
        match self {
            AppError::Database(_) => "database",
            AppError::NotConnected(_) => "not_connected",
            AppError::Invalid(_) => "invalid",
            AppError::ReadOnly(_) => "read_only",
            AppError::Storage(_) => "storage",
            AppError::Secret(_) => "secret",
        }
    }
}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("AppError", 2)?;
        s.serialize_field("kind", self.kind())?;
        s.serialize_field("message", &self.to_string())?;
        s.end()
    }
}

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        match e {
            sqlx::Error::Database(db) => AppError::Database(db.message().to_string()),
            other => AppError::Database(other.to_string()),
        }
    }
}

impl From<tiberius::error::Error> for AppError {
    fn from(e: tiberius::error::Error) -> Self {
        match e {
            tiberius::error::Error::Server(token) => {
                AppError::Database(token.message().to_string())
            }
            other => AppError::Database(other.to_string()),
        }
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        AppError::Storage(e.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        AppError::Storage(e.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;
