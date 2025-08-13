use crate::models::Account;
use crate::storage::Storage;
use tracing::{error, warn, debug};
use chrono::Utc;
use std::sync::Arc;

// oauth 모듈을 공개로 설정
pub mod oauth;
pub mod token;

/// 인증 오류 열거형
#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("Invalid credentials: {0}")]
    InvalidCredentials(String),
    
    #[error("Invalid token: {0}")]
    InvalidToken(String),
    
    #[error("Authentication required: {0}")]
    AuthenticationRequired(String),
    
    #[error("Permission denied: {0}")]
    PermissionDenied(String),
    
    #[error("Database error: {0}")]
    DatabaseError(String),
    
    #[error("External service error: {0}")]
    ExternalServiceError(String),
    
    #[error("Authentication error: {0}")]
    AuthenticationError(String),
    
    #[error("User not found: {0}")]
    UserNotFound(String),
    
    #[error("Missing user data: {0}")]
    MissingUserData(String),
    
    #[error("Invalid response format: {0}")]
    InvalidResponseFormat(String),
    
    #[error("Unknown error: {0}")]
    Unknown(String),
}

/// 인증 결과 타입
pub type Result<T> = std::result::Result<T, AuthError>;

// REMOVED: deprecated and unused validate_token function. Use OAuthService::verify_token instead.