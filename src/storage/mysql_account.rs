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
        
        // 재시도 로직을 위한 루프
        let mut retry_count = 0;
        let max_retries = 2;
        
        loop {
            // 트랜잭션 시작
            let mut conn = pool.get_conn().await.map_err(|e| {
                error!("MySQL connection failed: {}", e);
                StorageError::Database(format!("Failed to get connection: {}", e))
            })?;
            
            // 트랜잭션 시작
            conn.query_drop("START TRANSACTION").await.map_err(|e| {
                error!("Failed to start transaction: {}", e);
                StorageError::Database(format!("Failed to start transaction: {}", e))
            })?;
            
            info!("🔄 Creating account in MySQL database: account_hash={}, email={}", 
                  account.account_hash, account.email);
            
            // 계정 정보 삽입
            match conn.exec_drop(
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
            ).await {
                Ok(_) => {
                    info!("✅ Account created successfully in database: account_hash={}", account.account_hash);
                    
                    // 트랜잭션 커밋
                    match conn.query_drop("COMMIT").await {
                        Ok(_) => {
                            info!("✅ Transaction committed successfully");
                            
                            // 새로운 연결을 사용하여 데이터베이스에 실제로 저장되었는지 확인
                            let mut verify_conn = pool.get_conn().await.map_err(|e| {
                                error!("❌ Failed to get connection for verification: {}", e);
                                StorageError::Connection(format!("Failed to get connection for verification: {}", e))
                            })?;
                            
                            // 명시적인 SELECT 쿼리로 계정 조회
                            match verify_conn.exec_first::<(String, String, String, String, i64, i64, i64, bool), _, _>(
                                "SELECT account_hash, email, name, id, created_at, updated_at, last_login, is_active FROM accounts WHERE account_hash = ?",
                                (&account.account_hash,)
                            ).await {
                                Ok(Some((db_hash, db_email, db_name, db_id, db_created, db_updated, db_login, db_active))) => {
                                    info!("✅ Verified account exists in database with explicit query");
                                    info!("✅ Account details: hash={}, email={}, name={}, id={}, created_at={}, updated_at={}, last_login={}, is_active={}",
                                        db_hash, db_email, db_name, db_id, db_created, db_updated, db_login, db_active);
                                },
                                Ok(None) => {
                                    error!("❌ Account not found in database after creation: account_hash={}", account.account_hash);
                                },
                                Err(e) => {
                                    error!("❌ Failed to verify account creation: {}", e);
                                }
                            }
                        },
                        Err(e) => {
                            error!("❌ Failed to commit transaction: {}", e);
                            // 롤백 시도
                            if let Err(e) = conn.query_drop("ROLLBACK").await {
                                error!("❌ Failed to rollback transaction: {}", e);
                            }
                            return Err(StorageError::Database(format!("Failed to commit transaction: {}", e)));
                        }
                    }
                    
                    return Ok(());
                },
                Err(e) => {
                    error!("❌ Failed to insert account into database: {}", e);
                    
                    // 롤백
                    if let Err(e) = conn.query_drop("ROLLBACK").await {
                        error!("❌ Failed to rollback transaction: {}", e);
                    }
                    
                    // 실패 원인 분석
                    if e.to_string().contains("Duplicate entry") {
                        if e.to_string().contains("PRIMARY") {
                            error!("❌ Duplicate primary key (id): {}", account.id);
                        } else if e.to_string().contains("account_hash") {
                            error!("❌ Duplicate account_hash: {}", account.account_hash);
                        } else if e.to_string().contains("email") {
                            error!("❌ Duplicate email: {}", account.email);
                        }
                        return Err(StorageError::Database(format!("Duplicate entry: {}", e)));
                    }
                    
                    // 테이블 존재 여부 확인
                    match conn.query_drop("SHOW TABLES LIKE 'accounts'").await {
                        Ok(_) => {
                            info!("✅ accounts 테이블이 존재함");
                            
                            // 테이블 구조 확인
                            match conn.query_drop("DESCRIBE accounts").await {
                                Ok(_) => {
                                    info!("✅ accounts 테이블 구조 확인됨");
                                },
                                Err(e) => {
                                    error!("❌ accounts 테이블 구조 확인 실패: {}", e);
                                }
                            }
                            
                            // 테이블은 존재하지만 삽입 실패, 최대 재시도 횟수 초과시 에러 반환
                            if retry_count >= max_retries {
                                return Err(StorageError::Database(format!("Failed to insert account after {} retries: {}", max_retries, e)));
                            }
                        },
                        Err(e) => {
                            error!("❌ accounts 테이블이 존재하지 않음: {}", e);
                            
                            // 테이블 생성 시도
                            let create_accounts_table = r"
                            CREATE TABLE IF NOT EXISTS accounts (
                                id VARCHAR(36) NOT NULL,
                                email VARCHAR(255) NOT NULL,
                                account_hash VARCHAR(255) NOT NULL PRIMARY KEY,
                                name VARCHAR(255) NOT NULL,
                                password_hash VARCHAR(255),
                                salt VARCHAR(255),
                                created_at BIGINT NOT NULL,
                                updated_at BIGINT NOT NULL,
                                last_login BIGINT,
                                is_active BOOLEAN NOT NULL DEFAULT TRUE
                            )";
                            
                            match conn.query_drop(create_accounts_table).await {
                                Ok(_) => {
                                    info!("✅ accounts 테이블 생성 성공, 계정 생성 재시도");
                                    // 다음 반복에서 다시 시도
                                    retry_count += 1;
                                    continue;
                                },
                                Err(e) => {
                                    error!("❌ accounts 테이블 생성 실패: {}", e);
                                    return Err(StorageError::Database(format!("Failed to create accounts table: {}", e)));
                                }
                            }
                        }
                    }
                }
            }
            
            // 여기까지 왔다면 테이블은 존재하지만 삽입 실패, 재시도
            retry_count += 1;
            if retry_count >= max_retries {
                return Err(StorageError::Database("Failed to insert account after maximum retries".to_string()));
            }
        }
    }
    
    /// ID로 계정 조회
    async fn get_account_by_id(&self, _id: &str) -> Result<Option<Account>> {
        Err(StorageError::NotImplemented("get_account_by_id not implemented for MySQL storage".to_string()))
    }
    
    /// 이메일로 계정 조회
    async fn get_account_by_email(&self, email: &str) -> Result<Option<Account>> {
        let pool = self.get_pool();
        let mut conn = pool.get_conn().await.map_err(|e| {
            error!("MySQL connection failed: {}", e);
            StorageError::Database(format!("Failed to get connection: {}", e))
        })?;
        
        info!("🔍 Looking up account by email: {}", email);
        
        // email로 계정 조회
        let account: Option<(String, String, String, String, String, String, i64, i64, i64, bool)> = conn.exec_first(
            r"SELECT 
                account_hash, id, email, name, password_hash, salt, 
                created_at, updated_at, last_login, is_active 
              FROM accounts 
              WHERE email = ?",
            (email,)
        ).await.map_err(|e| {
            error!("❌ Failed to query account by email: {}", e);
            StorageError::Database(format!("Failed to query account by email: {}", e))
        })?;
        
        if let Some((account_hash, id, email, name, password_hash, salt, created_at, updated_at, last_login, is_active)) = account {
            info!("✅ Found account by email: {}, account_hash={}", email, account_hash);
            
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
            info!("❓ No account found with email: {}", email);
            Ok(None)
        }
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