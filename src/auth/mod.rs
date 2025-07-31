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
    
    #[error("Unknown error: {0}")]
    Unknown(String),
}

/// 인증 결과 타입
pub type Result<T> = std::result::Result<T, AuthError>;

/// 토큰 검증
pub async fn validate_token(token: &str, storage: Arc<dyn Storage>) -> Result<String> {
    if token.is_empty() {
        return Err(AuthError::InvalidToken("Empty token provided".to_string()));
    }
    
    match storage.get_auth_token(token).await {
        Ok(Some(token_obj)) => {
            // 토큰 만료 여부 확인
            let now = Utc::now();
            if token_obj.expires_at < now {
                return Err(AuthError::InvalidToken("Token has expired".to_string()));
            }
            
            Ok(token_obj.account_hash)
        },
        Ok(None) => {
            Err(AuthError::InvalidToken("Token not found".to_string()))
        },
        Err(e) => {
            Err(AuthError::DatabaseError(format!("Database error: {}", e)))
        }
    }
}