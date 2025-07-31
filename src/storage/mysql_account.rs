use chrono::prelude::*;
use mysql_async::prelude::*;
use tracing::{debug, error, info};

use crate::models::account::Account;
use crate::storage::{Result, StorageError};
use crate::storage::mysql::MySqlStorage;

/// MySQL 계정 관련 기능 확장 트레이트
pub trait MySqlAccountExt {
    /// 계정 생성
    async fn create_account(&self, account: &Account) -> Result<()>;
    
    /// ID로 계정 조회
    async fn get_account_by_id(&self, id: &str) -> Result<Option<Account>>;
    
    /// 이메일로 계정 조회
    async fn get_account_by_email(&self, email: &str) -> Result<Option<Account>>;
    
    /// 해시로 계정 조회
    async fn get_account_by_hash(&self, account_hash: &str) -> Result<Option<Account>>;
    
    /// 계정 업데이트
    async fn update_account(&self, account: &Account) -> Result<()>;
    
    /// 계정 삭제
    async fn delete_account(&self, account_hash: &str) -> Result<()>;
}

impl MySqlAccountExt for MySqlStorage {
    /// 계정 생성
    async fn create_account(&self, account: &Account) -> Result<()> {
        let pool = self.get_pool();
        let mut conn = pool.get_conn().await.map_err(|e| {
            StorageError::Database(format!("Failed to get connection: {}", e))
        })?;
        
        // 계정 정보 삽입
        conn.exec_drop(
            r"INSERT INTO accounts (
                id, account_hash, email, name, 
                password_hash, salt, created_at, updated_at, 
                last_login, is_active
              ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            (
                &account.id,
                &account.account_hash,
                &account.email,
                &account.name,
                &account.password_hash,
                &account.salt,
                account.created_at.timestamp(),
                account.updated_at.timestamp(),
                account.last_login.timestamp(),
                account.is_active,
            ),
        ).await.map_err(|e| {
            StorageError::Database(format!("Failed to insert account: {}", e))
        })?;
        
        Ok(())
    }
    
    /// ID로 계정 조회
    async fn get_account_by_id(&self, _id: &str) -> Result<Option<Account>> {
        Err(StorageError::NotImplemented("get_account_by_id not implemented for MySQL storage".to_string()))
    }
    
    /// 이메일로 계정 조회
    async fn get_account_by_email(&self, _email: &str) -> Result<Option<Account>> {
        Err(StorageError::NotImplemented("get_account_by_email not implemented for MySQL storage".to_string()))
    }
    
    /// 해시로 계정 조회
    async fn get_account_by_hash(&self, account_hash: &str) -> Result<Option<Account>> {
        let pool = self.get_pool();
        let mut conn = pool.get_conn().await.map_err(|e| {
            StorageError::Database(format!("Failed to get connection: {}", e))
        })?;
        
        // account_hash로 계정 조회
        let account: Option<(String, String, String, String, String, String, i64, i64, i64, bool)> = conn.exec_first(
            r"SELECT 
                account_hash, id, email, name, password_hash, salt, 
                created_at, updated_at, last_login, is_active 
              FROM accounts 
              WHERE account_hash = ?",
            (account_hash,)
        ).await.map_err(|e| {
            StorageError::Database(format!("Failed to query account: {}", e))
        })?;
        
        if let Some((account_hash, id, email, name, password_hash, salt, created_at, updated_at, last_login, is_active)) = account {
            // 각 필드 값을 사용하여 Account 객체 생성
            let created_at = chrono::DateTime::from_timestamp(created_at, 0)
                .unwrap_or_else(|| chrono::Utc::now());
            let updated_at = chrono::DateTime::from_timestamp(updated_at, 0)
                .unwrap_or_else(|| chrono::Utc::now());
            let last_login = chrono::DateTime::from_timestamp(last_login, 0)
                .unwrap_or_else(|| chrono::Utc::now());
                
            let account = Account {
                account_hash,
                id,
                email,
                name,
                user_type: "oauth".to_string(), // 기본값 설정
                password_hash,
                salt,
                is_active,
                created_at,
                updated_at,
                last_login,
                user_id: "".to_string(), // 필요한 경우 값 설정
            };
            
            Ok(Some(account))
        } else {
            Ok(None)
        }
    }
    
    /// 계정 업데이트
    async fn update_account(&self, account: &Account) -> Result<()> {
        let pool = self.get_pool();
        let mut conn = pool.get_conn().await.map_err(|e| {
            StorageError::Database(format!("Failed to get connection: {}", e))
        })?;
        
        // 계정 정보 업데이트
        conn.exec_drop(
            r"UPDATE accounts SET 
                name = ?, 
                email = ?, 
                password_hash = ?, 
                salt = ?, 
                updated_at = ?, 
                last_login = ?, 
                is_active = ?
              WHERE account_hash = ?",
            (
                &account.name,
                &account.email,
                &account.password_hash,
                &account.salt,
                account.updated_at.timestamp(),
                account.last_login.timestamp(),
                account.is_active,
                &account.account_hash,
            ),
        ).await.map_err(|e| {
            StorageError::Database(format!("Failed to update account: {}", e))
        })?;
        
        Ok(())
    }
    
    /// 계정 삭제
    async fn delete_account(&self, _account_hash: &str) -> Result<()> {
        Err(StorageError::NotImplemented("delete_account not implemented for MySQL storage".to_string()))
    }
}