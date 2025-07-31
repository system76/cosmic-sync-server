use chrono::prelude::*;
use mysql_async::prelude::*;
use tracing::{debug, error, info};

use crate::models::auth::AuthToken;
use crate::storage::{Result, StorageError};
use crate::storage::mysql::MySqlStorage;

/// MySQL 인증 관련 기능 확장 트레이트
pub trait MySqlAuthExt {
    /// 인증 토큰 생성
    async fn create_auth_token(&self, auth_token: &AuthToken) -> Result<()>;
    
    /// 인증 토큰 조회
    async fn get_auth_token(&self, token: &str) -> Result<Option<AuthToken>>;
    
    /// 인증 토큰 검증
    async fn validate_auth_token(&self, token: &str, account_hash: &str) -> Result<bool>;
    
    /// 인증 토큰 업데이트
    async fn update_auth_token(&self, auth_token: &AuthToken) -> Result<()>;
    
    /// 인증 토큰 삭제
    async fn delete_auth_token(&self, token: &str) -> Result<()>;
}

impl MySqlAuthExt for MySqlStorage {
    /// 인증 토큰 생성
    async fn create_auth_token(&self, auth_token: &AuthToken) -> Result<()> {
        let pool = self.get_pool();
        let mut conn = pool.get_conn().await.map_err(|e| {
            StorageError::Database(format!("Failed to get connection: {}", e))
        })?;
        
        // refresh_token이 옵션 타입이므로 적절하게 처리
        let refresh_token = auth_token.refresh_token.as_deref().unwrap_or("");
        let scope = auth_token.scope.as_deref().unwrap_or("");
        
        // 인증 토큰 정보 삽입
        conn.exec_drop(
            r"INSERT INTO auth_tokens (
                id, account_hash, access_token, refresh_token,
                token_type, expires_at, created_at
              ) VALUES (?, ?, ?, ?, ?, ?, ?)",
            (
                &auth_token.token_id,
                &auth_token.account_hash,
                &auth_token.access_token,
                refresh_token,
                &auth_token.token_type,
                auth_token.expires_at.timestamp(),
                auth_token.created_at.timestamp(),
            ),
        ).await.map_err(|e| {
            StorageError::Database(format!("Failed to insert auth token: {}", e))
        })?;
        
        Ok(())
    }
    
    /// 인증 토큰 조회
    async fn get_auth_token(&self, token: &str) -> Result<Option<AuthToken>> {
        let pool = self.get_pool();
        let mut conn = pool.get_conn().await.map_err(|e| {
            StorageError::Database(format!("데이터베이스 연결 실패: {}", e))
        })?;
        
        debug!("데이터베이스에서 인증 토큰 조회: {}", token);
        
        // 토큰으로 인증 정보 조회
        let token_data: Option<(String, String, String, String, Option<String>, i64, i64)> = conn.exec_first(
            r"SELECT 
                id, account_hash, access_token, token_type, 
                refresh_token, expires_at, created_at
              FROM auth_tokens 
              WHERE access_token = ?",
            (token,)
        ).await.map_err(|e| {
            StorageError::Database(format!("토큰 조회 쿼리 실패: {}", e))
        })?;
        
        match token_data {
            Some((token_id, account_hash, access_token, token_type, refresh_token, expires_at, created_at)) => {
                // 타임스탬프를 DateTime으로 변환
                let expires_at = match Utc.timestamp_opt(expires_at, 0) {
                    chrono::LocalResult::Single(dt) => dt,
                    _ => {
                        error!("만료 시간 변환 실패: {}", expires_at);
                        return Err(StorageError::General(format!("만료 시간 변환 실패: {}", expires_at)));
                    }
                };
                
                let created_at = match Utc.timestamp_opt(created_at, 0) {
                    chrono::LocalResult::Single(dt) => dt,
                    _ => {
                        error!("생성 시간 변환 실패: {}", created_at);
                        return Err(StorageError::General(format!("생성 시간 변환 실패: {}", created_at)));
                    }
                };
                
                // AuthToken 객체 생성
                let auth_token = AuthToken {
                    token_id,
                    account_hash,
                    access_token,
                    token_type,
                    refresh_token,
                    scope: None,
                    expires_at,
                    created_at,
                    is_valid: true,
                };
                
                Ok(Some(auth_token))
            },
            None => {
                debug!("인증 토큰을 찾을 수 없음: {}", token);
                Ok(None)
            }
        }
    }
    
    /// 인증 토큰 검증
    async fn validate_auth_token(&self, token: &str, account_hash: &str) -> Result<bool> {
        let pool = self.get_pool();
        let mut conn = pool.get_conn().await.map_err(|e| {
            StorageError::Database(format!("Failed to get connection: {}", e))
        })?;
        
        let now = chrono::Utc::now().timestamp();
        
        // 유효한 토큰이 있는지 확인 (만료되지 않은)
        let result: Option<(String,)> = conn.exec_first(
            r"SELECT account_hash 
            FROM auth_tokens 
            WHERE access_token = ? 
            AND account_hash = ? 
            AND expires_at > ?",
            (token, account_hash, now)
        ).await.map_err(|e| StorageError::Database(e.to_string()))?;
        
        Ok(result.is_some())
    }
    
    /// 인증 토큰 업데이트
    async fn update_auth_token(&self, _auth_token: &AuthToken) -> Result<()> {
        // 스텁 구현 - 추후 필드 매핑을 올바르게 수정하여 구현 예정
        Err(StorageError::NotImplemented("update_auth_token not implemented yet for MySQL storage".to_string()))
    }
    
    /// 인증 토큰 삭제
    async fn delete_auth_token(&self, token: &str) -> Result<()> {
        let pool = self.get_pool();
        let mut conn = pool.get_conn().await.map_err(|e| {
            StorageError::Database(format!("Failed to get connection: {}", e))
        })?;
        
        conn.exec_drop(
            "DELETE FROM auth_tokens WHERE access_token = ?",
            (token,)
        ).await.map_err(|e| StorageError::Database(e.to_string()))?;
        
        Ok(())
    }
}