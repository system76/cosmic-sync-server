use chrono::prelude::*;
// migrated to sqlx; removed mysql_async
use tracing::{debug, error, info};
use uuid::Uuid;

use crate::models::device::Device;
use crate::storage::mysql::MySqlStorage;
use crate::storage::{Result, StorageError};
// removed unused mysql_models::DeviceData

/// MySQL 장치 관련 기능 확장 트레이트
pub trait MySqlDeviceExt {
    /// 장치 등록
    async fn register_device(&self, device: &Device) -> Result<()>;

    /// 장치 조회
    async fn get_device(&self, account_hash: &str, device_hash: &str) -> Result<Option<Device>>;

    /// 장치 목록 조회
    async fn list_devices(&self, account_hash: &str) -> Result<Vec<Device>>;

    /// 장치 업데이트
    async fn update_device(&self, device: &Device) -> Result<()>;

    /// 장치 삭제
    async fn delete_device(&self, account_hash: &str, device_hash: &str) -> Result<()>;

    /// 장치 유효성 검증
    async fn validate_device(&self, account_hash: &str, device_hash: &str) -> Result<bool>;
}

impl MySqlDeviceExt for MySqlStorage {
    /// 장치 등록
    async fn register_device(&self, device: &Device) -> Result<()> {
        info!("Registering device: {}", device.device_hash);

        let mut tx = self.get_sqlx_pool().begin().await.map_err(|e| {
            error!("Failed to start transaction: {}", e);
            StorageError::Database(format!("Transaction start failed: {}", e))
        })?;

        // 타임스탬프 형식으로 변환 (문자열이 아닌 BIGINT로)
        let now = Utc::now().timestamp();
        let registered_at = device.registered_at.timestamp();
        let last_sync = device.last_sync.timestamp();

        debug!(
            "Device registration details - account_hash: {}, device_hash: {}, registered_at_ts: {}",
            device.account_hash, device.device_hash, registered_at
        );

        // 기존 장치 존재 여부 확인 (트랜잭션 내에서)
        let device_exists: Option<String> = sqlx::query_scalar(
            r#"SELECT device_hash FROM devices WHERE account_hash = ? AND device_hash = ?"#,
        )
        .bind(&device.account_hash)
        .bind(&device.device_hash)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| {
            error!("Database error when checking for existing device: {}", e);
            StorageError::Database(e.to_string())
        })?;

        if device_exists.is_some() {
            // 기존 장치가 존재하면 update 수행
            info!("Device already exists, updating: {}", device.device_hash);

            sqlx::query(
                r#"UPDATE devices SET 
                    device_name = ?, 
                    device_type = ?, 
                    os_type = ?, 
                    os_version = ?, 
                    app_version = ?, 
                    last_sync = ?, 
                    updated_at = ?,
                    is_active = ?
                  WHERE account_hash = ? AND device_hash = ?"#,
            )
            .bind(&device.user_id)
            .bind("desktop")
            .bind("Linux")
            .bind(&device.os_version)
            .bind(&device.app_version)
            .bind(last_sync)
            .bind(now)
            .bind(device.is_active)
            .bind(&device.account_hash)
            .bind(&device.device_hash)
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                error!(
                    "Failed to update existing device: {}, error: {}",
                    device.device_hash, e
                );
                StorageError::Database(e.to_string())
            })?;

            info!("Successfully updated device: {}", device.device_hash);
        } else {
            // 새 장치 등록
            info!("Registering new device: {}", device.device_hash);

            // 고유 UUID 생성
            let device_id = Uuid::new_v4().to_string();
            info!("Generated UUID for new device: {}", device_id);

            sqlx::query(
                r#"INSERT INTO devices (
                    id, account_hash, device_hash, device_name, device_type, 
                    os_type, os_version, app_version, last_sync, 
                    created_at, updated_at, is_active
                  ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
            )
            .bind(&device_id)
            .bind(&device.account_hash)
            .bind(&device.device_hash)
            .bind(&device.user_id)
            .bind("desktop")
            .bind("Linux")
            .bind(&device.os_version)
            .bind(&device.app_version)
            .bind(last_sync)
            .bind(registered_at)
            .bind(now)
            .bind(device.is_active)
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                error!(
                    "Failed to insert new device: {}, error: {}",
                    device.device_hash, e
                );
                StorageError::Database(e.to_string())
            })?;

            info!("Successfully registered new device: {}", device.device_hash);
        }

        // Commit transaction
        tx.commit().await.map_err(|e| {
            error!("Failed to commit device registration transaction: {}", e);
            StorageError::Database(format!("Transaction commit failed: {}", e))
        })?;

        Ok(())
    }

    /// 장치 조회
    async fn get_device(&self, account_hash: &str, device_hash: &str) -> Result<Option<Device>> {
        let device_data: Option<(
            String,
            String,
            String,
            String,
            String,
            String,
            String,
            i64,
            i64,
            i64,
            bool,
        )> = sqlx::query_as(
            r#"SELECT 
                account_hash, device_hash, device_name, device_type, 
                os_type, os_version, app_version, 
                last_sync, created_at, updated_at, is_active
              FROM devices 
              WHERE account_hash = ? AND device_hash = ?"#,
        )
        .bind(account_hash)
        .bind(device_hash)
        .fetch_optional(self.get_sqlx_pool())
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        if let Some((
            acc_hash,
            dev_hash,
            device_name,
            _device_type,
            _os_type,
            os_version,
            app_version,
            last_sync_ts,
            created_at_ts,
            updated_at_ts,
            is_active,
        )) = device_data
        {
            // 타임스탬프를 DateTime으로 변환
            let created_at = match Utc.timestamp_opt(created_at_ts, 0) {
                chrono::LocalResult::Single(dt) => dt,
                _ => Utc::now(),
            };

            let updated_at = match Utc.timestamp_opt(updated_at_ts, 0) {
                chrono::LocalResult::Single(dt) => dt,
                _ => Utc::now(),
            };

            let last_sync = match Utc.timestamp_opt(last_sync_ts, 0) {
                chrono::LocalResult::Single(dt) => dt,
                _ => Utc::now(),
            };

            // Device 객체 생성
            let device = Device {
                user_id: device_name,
                account_hash: acc_hash,
                device_hash: dev_hash,
                updated_at,
                registered_at: created_at,
                last_sync,
                is_active,
                os_version,
                app_version,
            };

            Ok(Some(device))
        } else {
            Ok(None)
        }
    }

    /// 장치 목록 조회
    async fn list_devices(&self, account_hash: &str) -> Result<Vec<Device>> {
        let devices_data: Vec<(
            String,
            String,
            String,
            String,
            String,
            String,
            String,
            i64,
            i64,
            i64,
            bool,
        )> = sqlx::query_as(
            r#"SELECT account_hash, device_hash, device_name, device_type, os_type, 
            os_version, app_version, last_sync, created_at, updated_at, is_active
            FROM devices 
            WHERE account_hash = ? AND is_active = TRUE
            ORDER BY created_at DESC"#,
        )
        .bind(account_hash)
        .fetch_all(self.get_sqlx_pool())
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        let mut result = Vec::with_capacity(devices_data.len());
        for (
            acc_hash,
            dev_hash,
            device_name,
            _device_type,
            _os_type,
            os_version,
            app_version,
            last_sync_ts,
            created_at_ts,
            updated_at_ts,
            is_active,
        ) in devices_data
        {
            // 타임스탬프를 DateTime으로 변환
            let created_at = match Utc.timestamp_opt(created_at_ts, 0) {
                chrono::LocalResult::Single(dt) => dt,
                _ => Utc::now(),
            };

            let updated_at = match Utc.timestamp_opt(updated_at_ts, 0) {
                chrono::LocalResult::Single(dt) => dt,
                _ => Utc::now(),
            };

            let last_sync = match Utc.timestamp_opt(last_sync_ts, 0) {
                chrono::LocalResult::Single(dt) => dt,
                _ => Utc::now(),
            };

            // Device 객체 생성
            let device = Device {
                user_id: device_name,
                account_hash: acc_hash,
                device_hash: dev_hash,
                updated_at,
                registered_at: created_at,
                last_sync,
                is_active,
                os_version,
                app_version,
            };

            result.push(device);
        }

        Ok(result)
    }

    /// 장치 업데이트
    async fn update_device(&self, device: &Device) -> Result<()> {
        // 타임스탬프 형식으로 변환 (문자열이 아닌 BIGINT로)
        let now = Utc::now().timestamp();
        let last_sync = device.last_sync.timestamp();

        info!(
            "Updating device: {}, last_sync_ts: {}",
            device.device_hash, last_sync
        );

        sqlx::query(
            r#"UPDATE devices SET 
                device_name = ?, 
                os_version = ?, 
                app_version = ?, 
                last_sync = ?, 
                updated_at = ?,
                is_active = ?
              WHERE account_hash = ? AND device_hash = ?"#,
        )
        .bind(&device.user_id)
        .bind(&device.os_version)
        .bind(&device.app_version)
        .bind(last_sync)
        .bind(now)
        .bind(device.is_active)
        .bind(&device.account_hash)
        .bind(&device.device_hash)
        .execute(self.get_sqlx_pool())
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        Ok(())
    }

    /// 장치 삭제
    async fn delete_device(&self, account_hash: &str, device_hash: &str) -> Result<()> {
        sqlx::query(r#"DELETE FROM devices WHERE account_hash = ? AND device_hash = ?"#)
            .bind(account_hash)
            .bind(device_hash)
            .execute(self.get_sqlx_pool())
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?;

        Ok(())
    }

    /// 장치 유효성 검증
    async fn validate_device(&self, account_hash: &str, device_hash: &str) -> Result<bool> {
        let result: Option<String> = sqlx::query_scalar(
            r#"SELECT device_hash FROM devices WHERE account_hash = ? AND device_hash = ? AND is_active = TRUE"#
        )
        .bind(account_hash)
        .bind(device_hash)
        .fetch_optional(self.get_sqlx_pool())
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;
        Ok(result.is_some())
    }
}
