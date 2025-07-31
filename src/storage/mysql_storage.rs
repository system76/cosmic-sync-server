use sqlx::{MySql, Pool};
use sqlx::mysql::MySqlPool;
use crate::models::{Account, AuthToken, Device, SyncFile, WatcherGroup, Watcher, WatcherCondition, ConditionType};
use crate::storage::Storage;
use std::error::Error;
use chrono::{DateTime, Utc};
use uuid::Uuid;
use tracing::{debug, error, info, warn};
use url;

/// MySQL 기반 스토리지 구현
pub struct MySqlStorage {
    pool: Pool<MySql>,
}

impl MySqlStorage {
    /// 새 MySQL 스토리지 생성
    pub async fn new(connect_string: &str) -> Result<Self, Box<dyn std::error::Error>> {
        // Create pool with MySQL connection
        info!("Connecting to MySQL database: {}", connect_string);
        
        let opts = url::Url::parse(connect_string)?;
        let pool = MySqlPool::connect(connect_string).await?;
        
        Ok(Self {
            pool,
        })
    }
    
    /// UUID 문자열을 바이너리로 변환
    fn uuid_to_binary(uuid: &str) -> Result<Vec<u8>, Box<dyn Error>> {
        let uuid_value = Uuid::parse_str(uuid)?;
        Ok(uuid_value.as_bytes().to_vec())
    }
    
    /// 바이너리 UUID를 문자열로 변환
    fn binary_to_uuid(binary: &[u8]) -> Result<String, Box<dyn Error>> {
        if binary.len() != 16 {
            return Err("잘못된 UUID 바이너리 길이".into());
        }
        
        let mut bytes = [0u8; 16];
        bytes.copy_from_slice(binary);
        let uuid = Uuid::from_bytes(bytes);
        
        Ok(uuid.to_string())
    }
}

#[async_trait::async_trait]
impl Storage for MySqlStorage {
    /// 계정 생성
    async fn create_account(&self, account: Account) -> Result<(), Box<dyn Error>> {
        let id = Uuid::new_v4();
        
        sqlx::query(
            "INSERT INTO users (id, account_hash, user_id, name, user_type, created_at, last_login) 
             VALUES (?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(id.as_bytes().to_vec())
        .bind(&account.account_hash)
        .bind(&account.user_id)
        .bind(&account.name)
        .bind(&account.user_type)
        .bind(account.created_at)
        .bind(account.last_login)
        .execute(&self.pool)
        .await?;
        
        Ok(())
    }
    
    /// 계정 조회
    async fn get_account(&self, account_hash: &str) -> Result<Option<Account>, Box<dyn Error>> {
        let record = sqlx::query!(
            "SELECT account_hash, user_id, name, user_type, created_at, last_login 
             FROM users 
             WHERE account_hash = ?",
            account_hash
        )
        .fetch_optional(&self.pool)
        .await?;
        
        Ok(record.map(|row| Account {
            account_hash: row.account_hash,
            user_id: row.user_id,
            name: row.name,
            user_type: row.user_type,
            created_at: row.created_at,
            last_login: row.last_login,
        }))
    }
    
    /// 계정 마지막 로그인 시간 업데이트
    async fn update_account_last_login(&self, account_hash: &str) -> Result<(), Box<dyn Error>> {
        sqlx::query(
            "UPDATE users 
             SET last_login = ? 
             WHERE account_hash = ?"
        )
        .bind(Utc::now())
        .bind(account_hash)
        .execute(&self.pool)
        .await?;
        
        Ok(())
    }
    
    /// 인증 토큰 저장
    async fn save_auth_token(&self, token: &AuthToken) -> Result<(), Box<dyn Error>> {
        // 테이블 스키마: token(PK), account_hash, created_at, expires_at
        sqlx::query(
            "INSERT INTO auth_tokens (token, account_hash, created_at, expires_at) 
             VALUES (?, ?, ?, ?)"
        )
        .bind(&token.access_token)
        .bind(&token.account_hash)
        .bind(token.created_at)
        .bind(token.expires_at)
        .execute(&self.pool)
        .await?;
        
        Ok(())
    }
    
    /// 토큰으로 계정 해시 조회
    async fn get_token_data(&self, token: &str) -> Result<Option<String>, Box<dyn Error>> {
        let record = sqlx::query!(
            "SELECT account_hash
             FROM auth_tokens
             WHERE token = ? AND expires_at > ?",
            token,
            Utc::now()
        )
        .fetch_optional(&self.pool)
        .await?;
        
        Ok(record.map(|row| row.account_hash))
    }
    
    /// 토큰 저장
    async fn store_token(&self, token: &str, account_hash: &str) -> Result<(), Box<dyn Error>> {
        let now = Utc::now();
        let expires_at = now + chrono::Duration::days(30);
        
        sqlx::query(
            "INSERT INTO auth_tokens (token, account_hash, created_at, expires_at) 
             VALUES (?, ?, ?, ?)"
        )
        .bind(token)
        .bind(account_hash)
        .bind(now)
        .bind(expires_at)
        .execute(&self.pool)
        .await?;
        
        Ok(())
    }
    
    /// 암호화 키 조회
    async fn get_encryption_key(&self, account_hash: &str) -> Result<Option<String>, Box<dyn Error>> {
        let record = sqlx::query!(
            "SELECT k.encryption_key 
             FROM user_encryption_keys k
             JOIN users u ON k.user_id = u.id
             WHERE u.account_hash = ? AND k.active = TRUE
             ORDER BY k.created_at DESC
             LIMIT 1",
            account_hash
        )
        .fetch_optional(&self.pool)
        .await?;
        
        Ok(record.map(|row| row.encryption_key))
    }
    
    /// 암호화 키 저장
    async fn store_encryption_key(&self, account_hash: &str, key: &str) -> Result<(), Box<dyn Error>> {
        // 계정 ID 조회
        let user_id = sqlx::query!(
            "SELECT id FROM users WHERE account_hash = ?",
            account_hash
        )
        .fetch_one(&self.pool)
        .await?.id;
        
        let id = Uuid::new_v4();
        
        // 기존 키 비활성화
        sqlx::query(
            "UPDATE user_encryption_keys 
             SET active = FALSE
             WHERE user_id = ? AND active = TRUE"
        )
        .bind(&user_id)
        .execute(&self.pool)
        .await?;
        
        // 새 키 저장
        sqlx::query(
            "INSERT INTO user_encryption_keys (id, user_id, encryption_key, created_at, active)
             VALUES (?, ?, ?, ?, TRUE)"
        )
        .bind(id.as_bytes().to_vec())
        .bind(user_id)
        .bind(key)
        .bind(Utc::now())
        .execute(&self.pool)
        .await?;
        
        Ok(())
    }
    
    /// 계정의 기기 목록 조회
    async fn get_devices_by_account(&self, account_hash: &str) -> Result<Vec<Device>, Box<dyn Error>> {
        let records = sqlx::query!(
            "SELECT d.id, d.device_hash,
                    d.os_version, d.app_version,
                    d.created_at, d.last_sync, d.is_active
             FROM devices d
             JOIN users u ON d.user_id = u.id
             WHERE u.account_hash = ? AND d.is_active = TRUE",
            account_hash
        )
        .fetch_all(&self.pool)
        .await?;
        
        let mut devices = Vec::with_capacity(records.len());
        
        for row in records {
            let id = Self::binary_to_uuid(&row.id)?;
            
            devices.push(Device {
                id,
                user_id: row.user_id,
                device_hash: row.device_hash,
                device_name: row.device_name,
                device_type: row.device_type,
                os_version: row.os_version,
                app_version: row.app_version,
                created_at: row.created_at,
                last_sync: row.last_sync,
                is_active: row.is_active,
            });
        }
        
        Ok(devices)
    }
    
    /// 기기 등록
    async fn register_device(&self, device: &Device) -> Result<(), Box<dyn Error>> {
        // 계정 ID 조회
        let user_id = sqlx::query!(
            "SELECT id FROM users WHERE account_hash = ?",
            device.user_id
        )
        .fetch_one(&self.pool)
        .await?.id;
        
        let id = Uuid::parse_str(&device.id)?;
        
        sqlx::query(
            "INSERT INTO devices (id, user_id, device_hash, 
                                os_version, app_version,
                                created_at, last_sync, is_active)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(id.as_bytes().to_vec())
        .bind(user_id)
        .bind(&device.device_hash)
        .bind(&device.os_version)
        .bind(&device.app_version)
        .bind(device.created_at)
        .bind(device.last_sync)
        .bind(device.is_active)
        .execute(&self.pool)
        .await?;
        
        Ok(())
    }
    
    /// 기기 조회
    async fn get_device(&self, device_hash: &str) -> Result<Option<Device>, Box<dyn Error>> {
        let record = sqlx::query!(
            "SELECT d.id, u.account_hash,  
                    d.os_version, d.app_version,
                    d.created_at, d.last_sync, d.is_active
             FROM devices d
             JOIN users u ON d.user_id = u.id
             WHERE d.device_hash = ?",
            device_hash
        )
        .fetch_optional(&self.pool)
        .await?;
        
        if let Some(row) = record {
            let id = Self::binary_to_uuid(&row.id)?;
            
            Ok(Some(Device {
                id,
                user_id: row.account_hash,
                device_hash: device_hash.to_string(),
                os_version: row.os_version,
                app_version: row.app_version,
                created_at: row.created_at,
                last_sync: row.last_sync,
                is_active: row.is_active,
            }))
        } else {
            Ok(None)
        }
    }
    
    /// 기기 정보 업데이트
    async fn update_device(&self, device: &Device) -> Result<(), Box<dyn Error>> {
        sqlx::query(
            "UPDATE devices 
             SET os_version = ?, 
                 app_version = ?
             WHERE device_hash = ?"
        )
        .bind(&device.os_version)
        .bind(&device.app_version)
        .bind(&device.device_hash)
        .execute(&self.pool)
        .await?;
        
        Ok(())
    }
    
    /// 기기 삭제 (비활성화)
    async fn delete_device(&self, device_hash: &str) -> Result<(), Box<dyn Error>> {
        sqlx::query(
            "UPDATE devices 
             SET is_active = FALSE
             WHERE device_hash = ?"
        )
        .bind(device_hash)
        .execute(&self.pool)
        .await?;
        
        Ok(())
    }
    
    // 추가적인 스토리지 메소드는 여기에 구현
}