use serde::{Deserialize, Serialize};
use std::fmt;
use thiserror::Error;

/// Unified error type for the entire application
#[derive(Debug, Error, Clone, Serialize, Deserialize)]
pub enum SyncError {
    #[error("Storage error: {0}")]
    Storage(String),

    #[error("Database error: {0}")]
    Database(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Authentication error: {0}")]
    Authentication(String),

    #[error("Authorization error: {0}")]
    Authorization(String),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Network error: {0}")]
    Network(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Invalid request: {0}")]
    InvalidRequest(String),

    #[error("Service unavailable: {0}")]
    ServiceUnavailable(String),

    #[error("Timeout: {0}")]
    Timeout(String),

    #[error("Rate limit exceeded: {0}")]
    RateLimit(String),

    #[error("Internal server error: {0}")]
    Internal(String),

    #[error("External service error: {0}")]
    ExternalService(String),

    #[error("File system error: {0}")]
    FileSystem(String),

    #[error("Encryption error: {0}")]
    Encryption(String),
}

/// Result type alias for convenience
pub type Result<T> = std::result::Result<T, SyncError>;

/// Legacy error type for backward compatibility
pub type AppError = SyncError;

impl SyncError {
    /// Create a new storage error
    pub fn storage<T: Into<String>>(msg: T) -> Self {
        Self::Storage(msg.into())
    }

    /// Create a new database error
    pub fn database<T: Into<String>>(msg: T) -> Self {
        Self::Database(msg.into())
    }

    /// Create a new config error
    pub fn config<T: Into<String>>(msg: T) -> Self {
        Self::Config(msg.into())
    }

    /// Create a new authentication error
    pub fn auth<T: Into<String>>(msg: T) -> Self {
        Self::Authentication(msg.into())
    }

    /// Create a new validation error
    pub fn validation<T: Into<String>>(msg: T) -> Self {
        Self::Validation(msg.into())
    }

    /// Create a new not found error
    pub fn not_found<T: Into<String>>(msg: T) -> Self {
        Self::NotFound(msg.into())
    }

    /// Create a new internal error
    pub fn internal<T: Into<String>>(msg: T) -> Self {
        Self::Internal(msg.into())
    }

    /// Get error category for metrics and logging
    pub fn category(&self) -> &'static str {
        match self {
            SyncError::Storage(_) => "storage",
            SyncError::Database(_) => "database",
            SyncError::Config(_) => "config",
            SyncError::Authentication(_) => "auth",
            SyncError::Authorization(_) => "auth",
            SyncError::Validation(_) => "validation",
            SyncError::Network(_) => "network",
            SyncError::Serialization(_) => "serialization",
            SyncError::NotFound(_) => "not_found",
            SyncError::InvalidRequest(_) => "invalid_request",
            SyncError::ServiceUnavailable(_) => "service_unavailable",
            SyncError::Timeout(_) => "timeout",
            SyncError::RateLimit(_) => "rate_limit",
            SyncError::Internal(_) => "internal",
            SyncError::ExternalService(_) => "external_service",
            SyncError::FileSystem(_) => "filesystem",
            SyncError::Encryption(_) => "encryption",
        }
    }

    /// Check if error is retryable
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            SyncError::Network(_)
                | SyncError::ServiceUnavailable(_)
                | SyncError::Timeout(_)
                | SyncError::ExternalService(_)
        )
    }

    /// Get HTTP status code for this error
    pub fn http_status_code(&self) -> u16 {
        match self {
            SyncError::Storage(_) => 500,
            SyncError::Database(_) => 500,
            SyncError::Config(_) => 500,
            SyncError::Authentication(_) => 401,
            SyncError::Authorization(_) => 403,
            SyncError::Validation(_) => 400,
            SyncError::Network(_) => 502,
            SyncError::Serialization(_) => 400,
            SyncError::NotFound(_) => 404,
            SyncError::InvalidRequest(_) => 400,
            SyncError::ServiceUnavailable(_) => 503,
            SyncError::Timeout(_) => 408,
            SyncError::RateLimit(_) => 429,
            SyncError::Internal(_) => 500,
            SyncError::ExternalService(_) => 502,
            SyncError::FileSystem(_) => 500,
            SyncError::Encryption(_) => 500,
        }
    }

    /// Convert to JSON for API responses
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "error": self.category(),
            "message": self.to_string(),
            "code": self.http_status_code(),
            "retryable": self.is_retryable(),
            "timestamp": chrono::Utc::now().timestamp()
        })
    }
}

// Storage error conversions removed - implemented in storage/mod.rs

// Database error conversions
impl From<sqlx::Error> for SyncError {
    fn from(err: sqlx::Error) -> Self {
        match err {
            sqlx::Error::RowNotFound => SyncError::NotFound("Record not found".to_string()),
            sqlx::Error::Database(db_err) => {
                SyncError::Database(format!("Database error: {}", db_err))
            }
            sqlx::Error::Io(io_err) => {
                SyncError::Network(format!("Database I/O error: {}", io_err))
            }
            sqlx::Error::PoolTimedOut => {
                SyncError::Timeout("Database connection pool timeout".to_string())
            }
            sqlx::Error::PoolClosed => {
                SyncError::ServiceUnavailable("Database connection pool closed".to_string())
            }
            _ => SyncError::Database(format!("Database error: {}", err)),
        }
    }
}

// I/O error conversions
impl From<std::io::Error> for SyncError {
    fn from(err: std::io::Error) -> Self {
        match err.kind() {
            std::io::ErrorKind::NotFound => SyncError::NotFound(err.to_string()),
            std::io::ErrorKind::PermissionDenied => SyncError::Authorization(err.to_string()),
            std::io::ErrorKind::TimedOut => SyncError::Timeout(err.to_string()),
            std::io::ErrorKind::InvalidInput => SyncError::InvalidRequest(err.to_string()),
            _ => SyncError::FileSystem(format!("I/O error: {}", err)),
        }
    }
}

// Serialization error conversions
impl From<serde_json::Error> for SyncError {
    fn from(err: serde_json::Error) -> Self {
        SyncError::Serialization(format!("JSON error: {}", err))
    }
}

// Network error conversions
impl From<reqwest::Error> for SyncError {
    fn from(err: reqwest::Error) -> Self {
        if err.is_timeout() {
            SyncError::Timeout(format!("HTTP timeout: {}", err))
        } else if err.is_connect() {
            SyncError::Network(format!("HTTP connection error: {}", err))
        } else {
            SyncError::ExternalService(format!("HTTP error: {}", err))
        }
    }
}

// tonic Status conversions for gRPC
impl From<SyncError> for tonic::Status {
    fn from(error: SyncError) -> Self {
        let code = match error {
            SyncError::Storage(_) => tonic::Code::Internal,
            SyncError::Database(_) => tonic::Code::Internal,
            SyncError::Config(_) => tonic::Code::Internal,
            SyncError::Authentication(_) => tonic::Code::Unauthenticated,
            SyncError::Authorization(_) => tonic::Code::PermissionDenied,
            SyncError::Validation(_) => tonic::Code::InvalidArgument,
            SyncError::Network(_) => tonic::Code::Unavailable,
            SyncError::Serialization(_) => tonic::Code::InvalidArgument,
            SyncError::NotFound(_) => tonic::Code::NotFound,
            SyncError::InvalidRequest(_) => tonic::Code::InvalidArgument,
            SyncError::ServiceUnavailable(_) => tonic::Code::Unavailable,
            SyncError::Timeout(_) => tonic::Code::DeadlineExceeded,
            SyncError::RateLimit(_) => tonic::Code::ResourceExhausted,
            SyncError::Internal(_) => tonic::Code::Internal,
            SyncError::ExternalService(_) => tonic::Code::Unavailable,
            SyncError::FileSystem(_) => tonic::Code::Internal,
            SyncError::Encryption(_) => tonic::Code::Internal,
        };

        tonic::Status::new(code, error.to_string())
    }
}

impl From<tonic::Status> for SyncError {
    fn from(status: tonic::Status) -> Self {
        match status.code() {
            tonic::Code::NotFound => SyncError::NotFound(status.message().to_string()),
            tonic::Code::Unauthenticated => SyncError::Authentication(status.message().to_string()),
            tonic::Code::PermissionDenied => SyncError::Authorization(status.message().to_string()),
            tonic::Code::InvalidArgument => SyncError::InvalidRequest(status.message().to_string()),
            tonic::Code::DeadlineExceeded => SyncError::Timeout(status.message().to_string()),
            tonic::Code::Unavailable => SyncError::ServiceUnavailable(status.message().to_string()),
            tonic::Code::ResourceExhausted => SyncError::RateLimit(status.message().to_string()),
            _ => SyncError::Internal(format!("gRPC error: {}", status.message())),
        }
    }
}

// Actix web error conversions
impl From<SyncError> for actix_web::Error {
    fn from(error: SyncError) -> Self {
        let status_code = match error.http_status_code() {
            400 => actix_web::http::StatusCode::BAD_REQUEST,
            401 => actix_web::http::StatusCode::UNAUTHORIZED,
            403 => actix_web::http::StatusCode::FORBIDDEN,
            404 => actix_web::http::StatusCode::NOT_FOUND,
            408 => actix_web::http::StatusCode::REQUEST_TIMEOUT,
            429 => actix_web::http::StatusCode::TOO_MANY_REQUESTS,
            500 => actix_web::http::StatusCode::INTERNAL_SERVER_ERROR,
            502 => actix_web::http::StatusCode::BAD_GATEWAY,
            503 => actix_web::http::StatusCode::SERVICE_UNAVAILABLE,
            _ => actix_web::http::StatusCode::INTERNAL_SERVER_ERROR,
        };

        actix_web::error::InternalError::new(error.to_string(), status_code).into()
    }
}

/// Error context trait for adding additional context to errors
pub trait ErrorContext<T> {
    fn with_context<F>(self, f: F) -> Result<T>
    where
        F: FnOnce() -> String;

    fn context(self, msg: &str) -> Result<T>;
}

impl<T, E> ErrorContext<T> for std::result::Result<T, E>
where
    E: Into<SyncError>,
{
    fn with_context<F>(self, f: F) -> Result<T>
    where
        F: FnOnce() -> String,
    {
        self.map_err(|e| {
            let base_error: SyncError = e.into();
            SyncError::Internal(format!("{}: {}", f(), base_error))
        })
    }

    fn context(self, msg: &str) -> Result<T> {
        self.with_context(|| msg.to_string())
    }
}

impl<T> ErrorContext<T> for Option<T> {
    fn with_context<F>(self, f: F) -> Result<T>
    where
        F: FnOnce() -> String,
    {
        self.ok_or_else(|| SyncError::NotFound(f()))
    }

    fn context(self, msg: &str) -> Result<T> {
        self.with_context(|| msg.to_string())
    }
}

/// Convenience macros for error handling
#[macro_export]
macro_rules! ensure {
    ($cond:expr, $err:expr) => {
        if !$cond {
            return Err($err.into());
        }
    };
}

#[macro_export]
macro_rules! bail {
    ($err:expr) => {
        return Err($err.into())
    };
}

#[macro_export]
macro_rules! anyhow {
    ($msg:literal $(,)?) => {
        $crate::error::SyncError::Internal($msg.to_string())
    };
    ($fmt:expr, $($arg:tt)*) => {
        $crate::error::SyncError::Internal(format!($fmt, $($arg)*))
    };
}
