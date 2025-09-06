use async_trait::async_trait;
use chrono::prelude::*;
// mysql_async removed; using only sqlx
use sqlx::mysql::MySqlPoolOptions as SqlxMySqlPoolOptions;
use sqlx::MySqlPool as SqlxMySqlPool;
use tracing::{debug, error, info, warn};

use crate::models::account::Account;
use crate::models::device::Device;
use crate::models::file::FileInfo;
use crate::models::file::FileNotice;
use crate::models::watcher::WatcherCondition;
use crate::storage::{Result, Storage, StorageError, StorageMetrics};

// MySQL 모듈 사용
use crate::storage::mysql_account::*;
use crate::storage::mysql_auth::*;
use crate::storage::mysql_device::*;
use crate::storage::mysql_file::*;
use crate::storage::mysql_watcher::*;

const CONNECTION_POOL_MIN: usize = 5;
const CONNECTION_POOL_MAX: usize = 50;

/// MySQL storage implementation
pub struct MySqlStorage {
    sqlx_pool: SqlxMySqlPool,
}

impl MySqlStorage {
    /// Create a new MySQL storage with existing mysql_async pool and a sqlx pool (transition)
    pub fn new_with_pool(_pool: (), sqlx_pool: SqlxMySqlPool) -> Result<Self> {
        Ok(Self { sqlx_pool })
    }

    /// Create new storage from URL (builds both mysql_async and sqlx pools)
    pub async fn new_with_url(url: &str) -> Result<Self> {
        let sqlx_pool = SqlxMySqlPoolOptions::new()
            .max_connections(10)
            .connect(url)
            .await
            .map_err(|e| StorageError::Connection(format!("Failed to connect via sqlx: {}", e)))?;
        Ok(Self { sqlx_pool })
    }

    /// Ensure server-side watcher_group and watcher mapping exists for given client IDs
    /// Returns (server_group_id, server_watcher_id)
    pub async fn ensure_server_ids_for(
        &self,
        account_hash: &str,
        device_hash: &str,
        client_group_id: i32,
        client_watcher_id: i32,
        folder_hint: Option<&str>,
    ) -> Result<(i32, i32)> {
        let mut tx =
            self.get_sqlx_pool().begin().await.map_err(|e| {
                StorageError::Database(format!("Failed to start transaction: {}", e))
            })?;

        // 1) Ensure watcher_group row for (account_hash, client_group_id)
        let existing_group: Option<i32> = sqlx::query_scalar(
            r#"SELECT id FROM watcher_groups WHERE account_hash = ? AND group_id = ?"#,
        )
        .bind(account_hash)
        .bind(client_group_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| StorageError::Database(format!("Failed to query watcher_groups: {}", e)))?;

        let server_group_id = if let Some(id) = existing_group {
            id
        } else {
            // Insert minimal watcher_group without touching other groups
            let now_dt = chrono::Utc::now();
            let created_at = crate::utils::time::datetime_to_mysql_string(&now_dt);
            let updated_at = created_at.clone();
            let title = format!("Client Group {}", client_group_id);

            sqlx::query(
                r#"INSERT INTO watcher_groups (
                    group_id, account_hash, device_hash, title,
                    created_at, updated_at, is_active
                ) VALUES (?, ?, ?, ?, ?, ?, 1)"#,
            )
            .bind(client_group_id)
            .bind(account_hash)
            .bind(device_hash)
            .bind(&title)
            .bind(&created_at)
            .bind(&updated_at)
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                StorageError::Database(format!("Failed to insert watcher_group: {}", e))
            })?;

            // last inserted id
            // Note: previous execute succeeded, so fetch from last result using ROW_COUNT() pattern is not available here; we re-run an id fetch
            // Instead, insert returns last_insert_id on mysql; use a separate insert capture
            {
                // re-insert is not desired; instead query last_insert_id via scalar u64 and cast
                let id_u64: u64 = sqlx::query_scalar("SELECT LAST_INSERT_ID()")
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(|e| {
                        StorageError::Database(format!("Failed to get last insert id: {}", e))
                    })?;
                if id_u64 > i32::MAX as u64 {
                    i32::MAX
                } else {
                    id_u64 as i32
                }
            }
        };

        if server_group_id == 0 {
            // Fallback reselect if needed
            let re: Option<i32> = sqlx::query_scalar(
                r#"SELECT id FROM watcher_groups WHERE account_hash = ? AND group_id = ?"#,
            )
            .bind(account_hash)
            .bind(client_group_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| {
                StorageError::Database(format!("Failed to reselect watcher_groups: {}", e))
            })?;
            if let Some(id) = re {
                id
            } else {
                0
            }
        } else {
            server_group_id
        };

        let server_group_id: i32 = if server_group_id == 0 {
            return Err(StorageError::Database(
                "Failed to ensure watcher_group id".to_string(),
            ));
        } else {
            server_group_id
        };

        // 2) Ensure watcher row mapping for (account_hash, local_group_id=client_group_id, watcher_id=client_watcher_id)
        let existing_watcher: Option<i32> = sqlx::query_scalar(
            r#"SELECT id FROM watchers WHERE account_hash = ? AND local_group_id = ? AND watcher_id = ?"#
        )
        .bind(account_hash)
        .bind(client_group_id)
        .bind(client_watcher_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| StorageError::Database(format!("Failed to query watchers: {}", e)))?;

        let server_watcher_id = if let Some(id) = existing_watcher {
            id
        } else {
            let folder = folder_hint
                .map(|s| s.to_string())
                .unwrap_or_else(|| "~/".to_string());
            let folder_name = folder.split('/').last().unwrap_or("Watcher").to_string();
            let title = format!("Watcher for {}", folder_name);
            let now_ts = chrono::Utc::now().timestamp();

            sqlx::query(
                r#"INSERT INTO watchers (
                    watcher_id, account_hash, group_id, local_group_id, folder, title,
                    is_recursive, created_at, updated_at, is_active, extra_json
                ) VALUES (?, ?, ?, ?, ?, ?, 1, ?, ?, 1, '{}')"#,
            )
            .bind(client_watcher_id)
            .bind(account_hash)
            .bind(server_group_id)
            .bind(client_group_id)
            .bind(&folder)
            .bind(&title)
            .bind(now_ts)
            .bind(now_ts)
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::Database(format!("Failed to insert watcher: {}", e)))?;

            // last inserted id
            // Note: previous execute succeeded, so fetch from last result using ROW_COUNT() pattern is not available here; we re-run an id fetch
            // Instead, insert returns last_insert_id on mysql; use a separate insert capture
            {
                // re-insert is not desired; instead query last_insert_id via scalar u64 and cast
                let id_u64: u64 = sqlx::query_scalar("SELECT LAST_INSERT_ID()")
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(|e| {
                        StorageError::Database(format!("Failed to get last insert id: {}", e))
                    })?;
                if id_u64 > i32::MAX as u64 {
                    i32::MAX
                } else {
                    id_u64 as i32
                }
            }
        };

        if server_watcher_id == 0 {
            return Err(StorageError::Database(
                "Failed to ensure watcher id".to_string(),
            ));
        }

        tx.commit().await.map_err(|e| {
            StorageError::Database(format!("Failed to commit ensure_server_ids_for: {}", e))
        })?;

        Ok((server_group_id, server_watcher_id))
    }

    /// Legacy constructor removed; use new_with_url instead
    pub fn new(_opts: ()) -> Result<Self> {
        Err(StorageError::NotImplemented("Use new_with_url".to_string()))
    }

    /// Check database connection
    pub async fn check_connection(&self) -> Result<()> {
        // Execute simple query to verify connection (sqlx)
        let result: Option<String> = sqlx::query_scalar("SELECT 'Connection OK'")
            .fetch_optional(self.get_sqlx_pool())
            .await
            .map_err(|e| StorageError::Database(format!("Failed to execute test query: {}", e)))?;
        if result.unwrap_or_default() != "Connection OK" {
            return Err(StorageError::Database(
                "Database connection check failed".to_string(),
            ));
        }

        Ok(())
    }

    /// Initialize database schema
    pub async fn init_schema(&self) -> Result<()> {
        info!("🔄 Initializing database schema...");

        // Create accounts table
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

        sqlx::query(create_accounts_table)
            .execute(self.get_sqlx_pool())
            .await
            .map_err(|e| {
                StorageError::Database(format!("Failed to create accounts table: {}", e))
            })?;

        info!("✅ accounts 테이블 생성 확인");

        // Create auth_tokens table
        let create_auth_tokens_table = r"
        CREATE TABLE IF NOT EXISTS auth_tokens (
            id VARCHAR(36) NOT NULL,
            token VARCHAR(255) NOT NULL PRIMARY KEY,
            account_hash VARCHAR(255) NOT NULL,
            created_at BIGINT NOT NULL,
            expires_at BIGINT NOT NULL,
            is_active BOOLEAN NOT NULL DEFAULT TRUE,
            INDEX (account_hash),
            FOREIGN KEY (account_hash) REFERENCES accounts(account_hash) ON DELETE CASCADE
        )";

        sqlx::query(create_auth_tokens_table)
            .execute(self.get_sqlx_pool())
            .await
            .map_err(|e| {
                StorageError::Database(format!("Failed to create auth_tokens table: {}", e))
            })?;

        info!("✅ auth_tokens 테이블 생성 확인");

        // Create devices table
        let create_devices_table = r"
        CREATE TABLE IF NOT EXISTS devices (
            id VARCHAR(36) NOT NULL,
            account_hash VARCHAR(255) NOT NULL,
            device_hash VARCHAR(255) NOT NULL PRIMARY KEY,
            device_name VARCHAR(255) NOT NULL,
            device_type VARCHAR(50),
            os_type VARCHAR(50),
            os_version VARCHAR(50),
            app_version VARCHAR(50),
            last_sync BIGINT,
            created_at BIGINT NOT NULL,
            updated_at BIGINT NOT NULL,
            is_active BOOLEAN NOT NULL DEFAULT TRUE,
            INDEX (account_hash),
            FOREIGN KEY (account_hash) REFERENCES accounts(account_hash) ON DELETE CASCADE
        )";

        sqlx::query(create_devices_table)
            .execute(self.get_sqlx_pool())
            .await
            .map_err(|e| {
                StorageError::Database(format!("Failed to create devices table: {}", e))
            })?;

        info!("✅ devices 테이블 생성 확인");

        // Create files table
        let create_files_table = r"
        CREATE TABLE IF NOT EXISTS files (
            id BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
            file_id BIGINT UNSIGNED NOT NULL,
            account_hash VARCHAR(255) NOT NULL,
            device_hash VARCHAR(255) NOT NULL,
            file_path VARBINARY(2048) NOT NULL,
            filename VARBINARY(512) NOT NULL,
            file_hash VARCHAR(255) NOT NULL,
            size BIGINT NOT NULL DEFAULT 0,
            is_deleted BOOLEAN NOT NULL DEFAULT FALSE,
            is_encrypted BOOLEAN NOT NULL DEFAULT FALSE,
            revision BIGINT NOT NULL DEFAULT 1,
            modified_time BIGINT NOT NULL,
            upload_time BIGINT NOT NULL,
            group_id INT NOT NULL DEFAULT 0,
            watcher_id INT NOT NULL DEFAULT 0,
            server_group_id INT NOT NULL DEFAULT 0,
            server_watcher_id INT NOT NULL DEFAULT 0,
            eq_index VARBINARY(32) NOT NULL,
            token_path VARCHAR(4096) NOT NULL,
            key_id VARCHAR(64) NULL,
            INDEX (account_hash),
            INDEX (file_id),
            INDEX (file_hash),
            INDEX (account_hash, group_id),
            INDEX (eq_index),
            INDEX token_path_prefix (token_path(255)),
            INDEX (account_hash, key_id),
            FOREIGN KEY (account_hash) REFERENCES accounts(account_hash) ON DELETE CASCADE
        )";

        sqlx::query(create_files_table)
            .execute(self.get_sqlx_pool())
            .await
            .map_err(|e| StorageError::Database(format!("Failed to create files table: {}", e)))?;

        info!("✅ files 테이블 생성 확인");

        // Create encryption_keys table
        let create_encryption_keys_table = r"
        CREATE TABLE IF NOT EXISTS encryption_keys (
            id BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
            account_hash VARCHAR(255) NOT NULL UNIQUE,
            encryption_key VARCHAR(255) NOT NULL,
            created_at BIGINT NOT NULL,
            updated_at BIGINT NOT NULL,
            FOREIGN KEY (account_hash) REFERENCES accounts(account_hash) ON DELETE CASCADE
        )";

        sqlx::query(create_encryption_keys_table)
            .execute(self.get_sqlx_pool())
            .await
            .map_err(|e| {
                StorageError::Database(format!("Failed to create encryption_keys table: {}", e))
            })?;

        info!("✅ encryption_keys 테이블 생성 확인");

        info!("✅ 데이터베이스 스키마 초기화 완료");

        Ok(())
    }

    /// 데이터베이스 스키마 마이그레이션
    pub async fn migrate_schema(&self) -> Result<()> {
        info!("데이터베이스 스키마 마이그레이션 시작");

        // watcher_presets 테이블에 presets 컬럼이 있는지 확인하고 preset_json으로 변경
        let has_presets_column: bool = sqlx::query_scalar(
            r#"SELECT COUNT(*) > 0
               FROM information_schema.columns
               WHERE table_schema = DATABASE()
                 AND table_name = 'watcher_presets'
                 AND column_name = 'presets'"#,
        )
        .fetch_one(self.get_sqlx_pool())
        .await
        .map_err(|e| {
            error!("컬럼 존재 여부 확인 실패: {}", e);
            StorageError::Database(format!("컬럼 존재 여부 확인 실패: {}", e))
        })?;

        if has_presets_column {
            info!("watcher_presets 테이블의 presets 컬럼을 preset_json으로 변경");

            // presets 컬럼을 preset_json으로 이름 변경
            sqlx::query(
                r#"ALTER TABLE watcher_presets CHANGE COLUMN presets preset_json TEXT NOT NULL"#,
            )
            .execute(self.get_sqlx_pool())
            .await
            .map_err(|e| {
                error!("presets 컬럼 이름 변경 실패: {}", e);
                StorageError::Database(format!("presets 컬럼 이름 변경 실패: {}", e))
            })?;

            info!("watcher_presets 테이블의 presets 컬럼을 preset_json으로 변경 완료");
        }

        // files 테이블에 is_encrypted 컬럼 존재 여부 확인
        let has_is_encrypted: bool = sqlx::query_scalar(
            r#"SELECT COUNT(*) > 0
               FROM information_schema.columns
               WHERE table_schema = DATABASE()
                 AND table_name = 'files'
                 AND column_name = 'is_encrypted'"#,
        )
        .fetch_one(self.get_sqlx_pool())
        .await
        .map_err(|e| {
            error!("컬럼 존재 여부 확인 실패: {}", e);
            StorageError::Database(format!("컬럼 존재 여부 확인 실패: {}", e))
        })?;

        if !has_is_encrypted {
            info!("files 테이블에 is_encrypted 컬럼 추가");

            // is_encrypted 컬럼 추가
            sqlx::query(
                r#"ALTER TABLE files ADD COLUMN is_encrypted BOOLEAN NOT NULL DEFAULT FALSE"#,
            )
            .execute(self.get_sqlx_pool())
            .await
            .map_err(|e| {
                error!("is_encrypted 컬럼 추가 실패: {}", e);
                StorageError::Database(format!("is_encrypted 컬럼 추가 실패: {}", e))
            })?;

            info!("files 테이블에 is_encrypted 컬럼 추가 완료");
        }

        // files 테이블에 operation_type 컬럼 존재 여부 확인
        let has_operation_type: bool = sqlx::query_scalar(
            r#"SELECT COUNT(*) > 0
               FROM information_schema.columns
               WHERE table_schema = DATABASE()
                 AND table_name = 'files'
                 AND column_name = 'operation_type'"#,
        )
        .fetch_one(self.get_sqlx_pool())
        .await
        .map_err(|e| {
            error!("컬럼 존재 여부 확인 실패: {}", e);
            StorageError::Database(format!("컬럼 존재 여부 확인 실패: {}", e))
        })?;

        if !has_operation_type {
            info!("files 테이블에 operation_type 컬럼 추가");

            // operation_type 컬럼 추가
            sqlx::query(
                r#"ALTER TABLE files ADD COLUMN operation_type VARCHAR(20) NOT NULL DEFAULT 'UPLOAD'"#
            ).execute(self.get_sqlx_pool()).await.map_err(|e| { error!("operation_type 컬럼 추가 실패: {}", e); StorageError::Database(format!("operation_type 컬럼 추가 실패: {}", e)) })?;

            // 기존 데이터 업데이트
            sqlx::query(r#"UPDATE files SET operation_type = 'UPLOAD' WHERE operation_type = ''"#)
                .execute(self.get_sqlx_pool())
                .await
                .map_err(|e| {
                    error!("operation_type 기본값 설정 실패: {}", e);
                    StorageError::Database(format!("operation_type 기본값 설정 실패: {}", e))
                })?;

            info!("files 테이블에 operation_type 컬럼 추가 완료");
        }

        // files 테이블에 eq_index/token_path/server_group_id/server_watcher_id 컬럼 추가, 경로 컬럼 형식 변경
        let has_eq_index: bool = sqlx::query_scalar(
            r#"SELECT COUNT(*) > 0 FROM information_schema.columns
               WHERE table_schema = DATABASE() AND table_name = 'files' AND column_name = 'eq_index'"#
        ).fetch_one(self.get_sqlx_pool()).await.map_err(|e| { error!("eq_index 컬럼 존재 여부 확인 실패: {}", e); StorageError::Database(format!("컬럼 존재 여부 확인 실패: {}", e)) })?;
        if !has_eq_index {
            info!("files 테이블에 eq_index 컬럼 추가");
            sqlx::query(r#"ALTER TABLE files ADD COLUMN eq_index VARBINARY(32) NULL"#)
                .execute(self.get_sqlx_pool())
                .await
                .map_err(|e| {
                    error!("eq_index 컬럼 추가 실패: {}", e);
                    StorageError::Database(format!("eq_index 컬럼 추가 실패: {}", e))
                })?;
            // 인덱스 확인 후 생성
            let has_eq_index_idx: bool = sqlx::query_scalar(
                r#"SELECT COUNT(*) > 0 FROM information_schema.statistics
                   WHERE table_schema = DATABASE() AND table_name = 'files' AND column_name = 'eq_index'"#
            ).fetch_one(self.get_sqlx_pool()).await.map_err(|e| StorageError::Database(format!("eq_index 인덱스 확인 실패: {}", e)))?;
            if !has_eq_index_idx {
                sqlx::query(r#"CREATE INDEX idx_files_eq_index ON files(eq_index)"#)
                    .execute(self.get_sqlx_pool())
                    .await
                    .map_err(|e| {
                        StorageError::Database(format!("eq_index 인덱스 생성 실패: {}", e))
                    })?;
            }
        }

        let has_token_path: bool = sqlx::query_scalar(
            r#"SELECT COUNT(*) > 0 FROM information_schema.columns
               WHERE table_schema = DATABASE() AND table_name = 'files' AND column_name = 'token_path'"#
        ).fetch_one(self.get_sqlx_pool()).await.map_err(|e| { error!("token_path 컬럼 존재 여부 확인 실패: {}", e); StorageError::Database(format!("컬럼 존재 여부 확인 실패: {}", e)) })?;
        if !has_token_path {
            info!("files 테이블에 token_path 컬럼 추가");
            sqlx::query(r#"ALTER TABLE files ADD COLUMN token_path VARCHAR(4096) NULL"#)
                .execute(self.get_sqlx_pool())
                .await
                .ok();

            // Optional: composite index for account_hash + key_id to support operational queries
            sqlx::query(r#"CREATE INDEX idx_files_account_keyid ON files (account_hash, key_id)"#)
                .execute(self.get_sqlx_pool())
                .await
                .ok();

            sqlx::query(r#"ALTER TABLE files ADD COLUMN server_group_id INT NOT NULL DEFAULT 0"#)
                .execute(self.get_sqlx_pool())
                .await
                .ok();
        }

        // files 테이블에 key_id 컬럼 존재 여부 확인 (token_path와 독립적으로 확인)
        let has_key_id: bool = sqlx::query_scalar(
            r#"SELECT COUNT(*) > 0 FROM information_schema.columns
               WHERE table_schema = DATABASE() AND table_name = 'files' AND column_name = 'key_id'"#
        ).fetch_one(self.get_sqlx_pool()).await.map_err(|e| {
            error!("key_id 컬럼 존재 여부 확인 실패: {}", e);
            StorageError::Database(format!("컬럼 존재 여부 확인 실패: {}", e))
        })?;
        if !has_key_id {
            info!("files 테이블에 key_id 컬럼 추가");
            sqlx::query(r#"ALTER TABLE files ADD COLUMN key_id VARCHAR(64) NULL"#)
                .execute(self.get_sqlx_pool())
                .await
                .map_err(|e| StorageError::Database(format!("key_id 컬럼 추가 실패: {}", e)))?;
            // 인덱스는 이미 존재할 수 있으니 실패는 무시
            sqlx::query(r#"CREATE INDEX idx_files_account_keyid ON files (account_hash, key_id)"#)
                .execute(self.get_sqlx_pool())
                .await
                .ok();
        }

        let has_server_group_id: bool = sqlx::query_scalar(
            r#"SELECT COUNT(*) > 0 FROM information_schema.columns
               WHERE table_schema = DATABASE() AND table_name = 'files' AND column_name = 'server_group_id'"#
        ).fetch_one(self.get_sqlx_pool()).await.map_err(|e| StorageError::Database(format!("server_group_id 컬럼 존재 여부 확인 실패: {}", e)))?;
        if !has_server_group_id {
            info!("files 테이블에 server_group_id 컬럼 추가");
            sqlx::query(r#"ALTER TABLE files ADD COLUMN server_group_id INT NOT NULL DEFAULT 0"#)
                .execute(self.get_sqlx_pool())
                .await
                .map_err(|e| {
                    StorageError::Database(format!("server_group_id 컬럼 추가 실패: {}", e))
                })?;
        }

        let has_server_watcher_id: bool = sqlx::query_scalar(
            r#"SELECT COUNT(*) > 0 FROM information_schema.columns
               WHERE table_schema = DATABASE() AND table_name = 'files' AND column_name = 'server_watcher_id'"#
        ).fetch_one(self.get_sqlx_pool()).await.map_err(|e| StorageError::Database(format!("server_watcher_id 컬럼 존재 여부 확인 실패: {}", e)))?;
        if !has_server_watcher_id {
            info!("files 테이블에 server_watcher_id 컬럼 추가");
            sqlx::query(r#"ALTER TABLE files ADD COLUMN server_watcher_id INT NOT NULL DEFAULT 0"#)
                .execute(self.get_sqlx_pool())
                .await
                .map_err(|e| {
                    StorageError::Database(format!("server_watcher_id 컬럼 추가 실패: {}", e))
                })?;
        }

        // file_path/filename 컬럼 형식 수정 (VARBINARY)
        let path_is_varbinary: bool = sqlx::query_scalar(
            r#"SELECT DATA_TYPE = 'varbinary' FROM information_schema.columns
               WHERE table_schema = DATABASE() AND table_name = 'files' AND column_name = 'file_path'"#
        ).fetch_one(self.get_sqlx_pool()).await.unwrap_or(false);
        if !path_is_varbinary {
            info!("files.file_path 컬럼을 VARBINARY(2048)로 변경");
            sqlx::query(r#"ALTER TABLE files MODIFY COLUMN file_path VARBINARY(2048) NOT NULL"#)
                .execute(self.get_sqlx_pool())
                .await
                .map_err(|e| {
                    StorageError::Database(format!("file_path 컬럼 형식 변경 실패: {}", e))
                })?;
        }
        let name_is_varbinary: bool = sqlx::query_scalar(
            r#"SELECT DATA_TYPE = 'varbinary' FROM information_schema.columns
               WHERE table_schema = DATABASE() AND table_name = 'files' AND column_name = 'filename'"#
        ).fetch_one(self.get_sqlx_pool()).await.unwrap_or(false);
        if !name_is_varbinary {
            info!("files.filename 컬럼을 VARBINARY(512)로 변경");
            sqlx::query(r#"ALTER TABLE files MODIFY COLUMN filename VARBINARY(512) NOT NULL"#)
                .execute(self.get_sqlx_pool())
                .await
                .map_err(|e| {
                    StorageError::Database(format!("filename 컬럼 형식 변경 실패: {}", e))
                })?;
        }

        // watcher_conditions 테이블 존재 여부 확인
        let has_watcher_conditions_table: bool = sqlx::query_scalar(
            r#"SELECT COUNT(*) > 0
               FROM information_schema.tables
               WHERE table_schema = DATABASE()
                 AND table_name = 'watcher_conditions'"#,
        )
        .fetch_one(self.get_sqlx_pool())
        .await
        .map_err(|e| {
            error!("테이블 존재 여부 확인 실패: {}", e);
            StorageError::Database(format!("테이블 존재 여부 확인 실패: {}", e))
        })?;

        if !has_watcher_conditions_table {
            info!("watcher_conditions 테이블 생성");

            // watcher_conditions 테이블 생성
            let create_watcher_conditions_table = r"
            CREATE TABLE watcher_conditions (
                id BIGINT AUTO_INCREMENT PRIMARY KEY,
                account_hash VARCHAR(255) NOT NULL,
                watcher_id INT NOT NULL,
                condition_type ENUM('union', 'subtract') NOT NULL,
                `key` VARCHAR(255) NOT NULL,
                value JSON NOT NULL,
                operator VARCHAR(50) NOT NULL DEFAULT 'equals',
                created_at BIGINT NOT NULL,
                updated_at BIGINT NOT NULL,
                INDEX (account_hash),
                INDEX (watcher_id),
                INDEX (account_hash, watcher_id),
                FOREIGN KEY (account_hash) REFERENCES accounts(account_hash) ON DELETE CASCADE,
                FOREIGN KEY (watcher_id) REFERENCES watchers(id) ON DELETE CASCADE
            )";

            sqlx::query(create_watcher_conditions_table)
                .execute(self.get_sqlx_pool())
                .await
                .map_err(|e| {
                    error!("watcher_conditions 테이블 생성 실패: {}", e);
                    StorageError::Database(format!("watcher_conditions 테이블 생성 실패: {}", e))
                })?;

            info!("watcher_conditions 테이블 생성 완료");
        } else {
            // watcher_conditions 테이블에 account_hash 컬럼 존재 여부 확인
            let has_account_hash_in_conditions: bool = sqlx::query_scalar(
                r#"SELECT COUNT(*) > 0
                 FROM information_schema.columns
                 WHERE table_schema = DATABASE()
                 AND table_name = 'watcher_conditions'
                 AND column_name = 'account_hash'"#,
            )
            .fetch_one(self.get_sqlx_pool())
            .await
            .map_err(|e| {
                error!("컬럼 존재 여부 확인 실패: {}", e);
                StorageError::Database(format!("컬럼 존재 여부 확인 실패: {}", e))
            })?;

            if !has_account_hash_in_conditions {
                info!("watcher_conditions 테이블에 account_hash 컬럼 추가");

                // account_hash 컬럼 추가
                sqlx::query(
                    r#"ALTER TABLE watcher_conditions
                     ADD COLUMN account_hash VARCHAR(255) NOT NULL DEFAULT ''"#,
                )
                .execute(self.get_sqlx_pool())
                .await
                .map_err(|e| {
                    error!("account_hash 컬럼 추가 실패: {}", e);
                    StorageError::Database(format!("account_hash 컬럼 추가 실패: {}", e))
                })?;

                // 기존 데이터의 account_hash 업데이트 (watchers 테이블에서 가져오기)
                sqlx::query(
                    r#"UPDATE watcher_conditions wc
                     JOIN watchers w ON wc.watcher_id = w.id
                     SET wc.account_hash = w.account_hash
                     WHERE wc.account_hash = ''"#,
                )
                .execute(self.get_sqlx_pool())
                .await
                .map_err(|e| {
                    error!("account_hash 업데이트 실패: {}", e);
                    StorageError::Database(format!("account_hash 업데이트 실패: {}", e))
                })?;

                // account_hash에 대한 외래키 제약조건 추가
                sqlx::query(
                    r#"ALTER TABLE watcher_conditions
                     ADD CONSTRAINT fk_watcher_conditions_account
                     FOREIGN KEY (account_hash) REFERENCES accounts(account_hash) ON DELETE CASCADE"#
                ).execute(self.get_sqlx_pool()).await.map_err(|e| { error!("외래키 제약조건 추가 실패: {}", e); StorageError::Database(format!("외래키 제약조건 추가 실패: {}", e)) })?;

                // account_hash에 대한 인덱스 추가
                sqlx::query(
                    r#"CREATE INDEX idx_watcher_conditions_account_hash ON watcher_conditions(account_hash)"#
                ).execute(self.get_sqlx_pool()).await.map_err(|e| { error!("인덱스 추가 실패: {}", e); StorageError::Database(format!("인덱스 추가 실패: {}", e)) })?;

                sqlx::query(
                    r#"CREATE INDEX idx_watcher_conditions_account_watcher ON watcher_conditions(account_hash, watcher_id)"#
                ).execute(self.get_sqlx_pool()).await.map_err(|e| { error!("복합 인덱스 추가 실패: {}", e); StorageError::Database(format!("복합 인덱스 추가 실패: {}", e)) })?;

                info!("watcher_conditions 테이블에 account_hash 컬럼 추가 완료");
            }
        }

        // watcher_groups 테이블 생성 여부 확인 및 생성
        let has_watcher_groups_table: bool = sqlx::query_scalar(
            r#"SELECT COUNT(*) > 0
               FROM information_schema.tables
               WHERE table_schema = DATABASE()
                 AND table_name = 'watcher_groups'"#,
        )
        .fetch_one(self.get_sqlx_pool())
        .await
        .map_err(|e| {
            error!("watcher_groups 테이블 존재 여부 확인 실패: {}", e);
            StorageError::Database(format!("테이블 존재 여부 확인 실패: {}", e))
        })?;
        if !has_watcher_groups_table {
            info!("watcher_groups 테이블 생성");
            let ddl = r#"
            CREATE TABLE watcher_groups (
                id INT AUTO_INCREMENT PRIMARY KEY,
                account_hash VARCHAR(255) NOT NULL,
                group_id INT NOT NULL,
                title VARCHAR(255) NOT NULL,
                created_at BIGINT NOT NULL,
                updated_at BIGINT NOT NULL,
                is_active BOOLEAN NOT NULL DEFAULT TRUE,
                UNIQUE KEY uniq_account_group (account_hash, group_id),
                INDEX (account_hash),
                FOREIGN KEY (account_hash) REFERENCES accounts(account_hash) ON DELETE CASCADE
            )"#;
            sqlx::query(ddl)
                .execute(self.get_sqlx_pool())
                .await
                .map_err(|e| {
                    error!("watcher_groups 테이블 생성 실패: {}", e);
                    StorageError::Database(format!("watcher_groups 테이블 생성 실패: {}", e))
                })?;
            info!("watcher_groups 테이블 생성 완료");
        }

        // watchers 테이블 생성 여부 확인 및 생성
        let has_watchers_table: bool = sqlx::query_scalar(
            r#"SELECT COUNT(*) > 0
               FROM information_schema.tables
               WHERE table_schema = DATABASE()
                 AND table_name = 'watchers'"#,
        )
        .fetch_one(self.get_sqlx_pool())
        .await
        .map_err(|e| {
            error!("watchers 테이블 존재 여부 확인 실패: {}", e);
            StorageError::Database(format!("테이블 존재 여부 확인 실패: {}", e))
        })?;
        if !has_watchers_table {
            info!("watchers 테이블 생성");
            let ddl = r#"
            CREATE TABLE watchers (
                id INT AUTO_INCREMENT PRIMARY KEY,
                watcher_id INT NOT NULL DEFAULT 0,
                account_hash VARCHAR(255) NOT NULL,
                group_id INT NOT NULL,
                local_group_id INT NOT NULL,
                folder VARCHAR(2048) NOT NULL,
                title VARCHAR(255) NOT NULL,
                is_recursive BOOLEAN NOT NULL DEFAULT TRUE,
                created_at BIGINT NOT NULL,
                updated_at BIGINT NOT NULL,
                is_active BOOLEAN NOT NULL DEFAULT TRUE,
                extra_json TEXT NOT NULL,
                INDEX (account_hash),
                INDEX (group_id),
                INDEX (account_hash, local_group_id, watcher_id),
                FOREIGN KEY (account_hash) REFERENCES accounts(account_hash) ON DELETE CASCADE,
                FOREIGN KEY (group_id) REFERENCES watcher_groups(id) ON DELETE CASCADE
            )"#;
            sqlx::query(ddl)
                .execute(self.get_sqlx_pool())
                .await
                .map_err(|e| {
                    error!("watchers 테이블 생성 실패: {}", e);
                    StorageError::Database(format!("watchers 테이블 생성 실패: {}", e))
                })?;
            info!("watchers 테이블 생성 완료");
        }

        // watchers 복합 유니크 인덱스 보장 (account_hash, local_group_id, watcher_id)
        let has_unique_acc_local_wid: Option<i64> = sqlx::query_scalar(
            r#"SELECT COUNT(*) FROM information_schema.statistics
               WHERE table_schema = DATABASE() AND table_name = 'watchers'
                 AND index_name = 'uniq_watchers_acc_local_wid' AND non_unique = 0"#,
        )
        .fetch_one(self.get_sqlx_pool())
        .await
        .ok();
        if has_unique_acc_local_wid.unwrap_or(0) == 0 {
            info!("watchers 테이블에 uniq_watchers_acc_local_wid 유니크 인덱스 생성");
            // 인덱스 생성 시 컬럼 구성 일치 필요
            if let Err(e) = sqlx::query(
                r#"CREATE UNIQUE INDEX uniq_watchers_acc_local_wid
                   ON watchers(account_hash, local_group_id, watcher_id)"#,
            )
            .execute(self.get_sqlx_pool())
            .await
            {
                warn!("유니크 인덱스 생성 경고(이미 존재 가능): {}", e);
            }
        }

        // watchers 테이블에 watcher_id 컬럼 존재 여부 확인
        let has_watcher_id_in_watchers: bool = sqlx::query_scalar(
            r#"SELECT COUNT(*) > 0
             FROM information_schema.columns
             WHERE table_schema = DATABASE()
             AND table_name = 'watchers'
             AND column_name = 'watcher_id'"#,
        )
        .fetch_one(self.get_sqlx_pool())
        .await
        .map_err(|e| {
            error!("컬럼 존재 여부 확인 실패: {}", e);
            StorageError::Database(format!("컬럼 존재 여부 확인 실패: {}", e))
        })?;

        if !has_watcher_id_in_watchers {
            info!("watchers 테이블에 watcher_id 컬럼 추가");

            // watcher_id 컬럼 추가
            sqlx::query(
                r#"ALTER TABLE watchers ADD COLUMN watcher_id INT NOT NULL DEFAULT 0 AFTER id"#,
            )
            .execute(self.get_sqlx_pool())
            .await
            .map_err(|e| {
                error!("watcher_id 컬럼 추가 실패: {}", e);
                StorageError::Database(format!("watcher_id 컬럼 추가 실패: {}", e))
            })?;

            // 기존 데이터의 watcher_id를 id와 동일하게 설정 (임시 처리)
            sqlx::query(r#"UPDATE watchers SET watcher_id = id WHERE watcher_id = 0"#)
                .execute(self.get_sqlx_pool())
                .await
                .map_err(|e| {
                    error!("watcher_id 기본값 설정 실패: {}", e);
                    StorageError::Database(format!("watcher_id 기본값 설정 실패: {}", e))
                })?;

            info!("watchers 테이블에 watcher_id 컬럼 추가 완료");
        }

        // watchers 테이블의 기타 필요 컬럼 보강(local_group_id, is_recursive, extra_json, title, folder, timestamps, flags, indexes)
        let need_local_group_id: bool = sqlx::query_scalar(
            r#"SELECT COUNT(*) = 0 FROM information_schema.columns
               WHERE table_schema = DATABASE() AND table_name = 'watchers' AND column_name = 'local_group_id'"#
        ).fetch_one(self.get_sqlx_pool()).await.unwrap_or(false);
        if need_local_group_id {
            sqlx::query(r#"ALTER TABLE watchers ADD COLUMN local_group_id INT NOT NULL DEFAULT 0 AFTER group_id"#)
                .execute(self.get_sqlx_pool()).await.map_err(|e| StorageError::Database(format!("local_group_id 추가 실패: {}", e)))?;
            sqlx::query(r#"CREATE INDEX idx_watchers_account_local_wid ON watchers(account_hash, local_group_id, watcher_id)"#)
                .execute(self.get_sqlx_pool()).await.ok();
        }
        let need_is_recursive: bool = sqlx::query_scalar(
            r#"SELECT COUNT(*) = 0 FROM information_schema.columns
               WHERE table_schema = DATABASE() AND table_name = 'watchers' AND column_name = 'is_recursive'"#
        ).fetch_one(self.get_sqlx_pool()).await.unwrap_or(false);
        if need_is_recursive {
            sqlx::query(
                r#"ALTER TABLE watchers ADD COLUMN is_recursive BOOLEAN NOT NULL DEFAULT TRUE"#,
            )
            .execute(self.get_sqlx_pool())
            .await
            .map_err(|e| StorageError::Database(format!("is_recursive 추가 실패: {}", e)))?;
        }
        let need_extra_json: bool = sqlx::query_scalar(
            r#"SELECT COUNT(*) = 0 FROM information_schema.columns
               WHERE table_schema = DATABASE() AND table_name = 'watchers' AND column_name = 'extra_json'"#
        ).fetch_one(self.get_sqlx_pool()).await.unwrap_or(false);
        if need_extra_json {
            sqlx::query(r#"ALTER TABLE watchers ADD COLUMN extra_json TEXT NOT NULL DEFAULT ''"#)
                .execute(self.get_sqlx_pool())
                .await
                .map_err(|e| StorageError::Database(format!("extra_json 추가 실패: {}", e)))?;
        }
        let need_title: bool = sqlx::query_scalar(
            r#"SELECT COUNT(*) = 0 FROM information_schema.columns
               WHERE table_schema = DATABASE() AND table_name = 'watchers' AND column_name = 'title'"#
        ).fetch_one(self.get_sqlx_pool()).await.unwrap_or(false);
        if need_title {
            sqlx::query(r#"ALTER TABLE watchers ADD COLUMN title VARCHAR(255) NOT NULL DEFAULT 'Watcher' AFTER folder"#)
                .execute(self.get_sqlx_pool()).await.map_err(|e| StorageError::Database(format!("title 추가 실패: {}", e)))?;
        }
        let need_folder: bool = sqlx::query_scalar(
            r#"SELECT COUNT(*) = 0 FROM information_schema.columns
               WHERE table_schema = DATABASE() AND table_name = 'watchers' AND column_name = 'folder'"#
        ).fetch_one(self.get_sqlx_pool()).await.unwrap_or(false);
        if need_folder {
            sqlx::query(r#"ALTER TABLE watchers ADD COLUMN folder VARCHAR(2048) NOT NULL DEFAULT '' AFTER local_group_id"#)
                .execute(self.get_sqlx_pool()).await.map_err(|e| StorageError::Database(format!("folder 추가 실패: {}", e)))?;
        }
        let need_created: bool = sqlx::query_scalar(
            r#"SELECT COUNT(*) = 0 FROM information_schema.columns
               WHERE table_schema = DATABASE() AND table_name = 'watchers' AND column_name = 'created_at'"#
        ).fetch_one(self.get_sqlx_pool()).await.unwrap_or(false);
        if need_created {
            sqlx::query(r#"ALTER TABLE watchers ADD COLUMN created_at BIGINT NOT NULL DEFAULT 0"#)
                .execute(self.get_sqlx_pool())
                .await
                .ok();
        }
        let need_updated: bool = sqlx::query_scalar(
            r#"SELECT COUNT(*) = 0 FROM information_schema.columns
               WHERE table_schema = DATABASE() AND table_name = 'watchers' AND column_name = 'updated_at'"#
        ).fetch_one(self.get_sqlx_pool()).await.unwrap_or(false);
        if need_updated {
            sqlx::query(r#"ALTER TABLE watchers ADD COLUMN updated_at BIGINT NOT NULL DEFAULT 0"#)
                .execute(self.get_sqlx_pool())
                .await
                .ok();
        }
        let need_is_active: bool = sqlx::query_scalar(
            r#"SELECT COUNT(*) = 0 FROM information_schema.columns
               WHERE table_schema = DATABASE() AND table_name = 'watchers' AND column_name = 'is_active'"#
        ).fetch_one(self.get_sqlx_pool()).await.unwrap_or(false);
        if need_is_active {
            sqlx::query(
                r#"ALTER TABLE watchers ADD COLUMN is_active BOOLEAN NOT NULL DEFAULT TRUE"#,
            )
            .execute(self.get_sqlx_pool())
            .await
            .ok();
        }

        // watcher_presets 테이블 생성 여부 확인 및 생성/보강
        let has_watcher_presets: bool = sqlx::query_scalar(
            r#"SELECT COUNT(*) > 0 FROM information_schema.tables
               WHERE table_schema = DATABASE() AND table_name = 'watcher_presets'"#,
        )
        .fetch_one(self.get_sqlx_pool())
        .await
        .map_err(|e| StorageError::Database(format!("watcher_presets 테이블 확인 실패: {}", e)))?;
        if !has_watcher_presets {
            info!("watcher_presets 테이블 생성");
            let ddl = r#"
            CREATE TABLE watcher_presets (
                id BIGINT AUTO_INCREMENT PRIMARY KEY,
                account_hash VARCHAR(255) NOT NULL UNIQUE,
                preset_json TEXT NOT NULL,
                created_at BIGINT NOT NULL,
                updated_at BIGINT NOT NULL,
                FOREIGN KEY (account_hash) REFERENCES accounts(account_hash) ON DELETE CASCADE
            )"#;
            sqlx::query(ddl)
                .execute(self.get_sqlx_pool())
                .await
                .map_err(|e| {
                    StorageError::Database(format!("watcher_presets 테이블 생성 실패: {}", e))
                })?;
            info!("watcher_presets 테이블 생성 완료");
        } else {
            // preset_json 컬럼/인덱스 보강
            let has_preset_json: bool = sqlx::query_scalar(
                r#"SELECT COUNT(*) > 0 FROM information_schema.columns
                   WHERE table_schema = DATABASE() AND table_name = 'watcher_presets' AND column_name = 'preset_json'"#
            ).fetch_one(self.get_sqlx_pool()).await.unwrap_or(false);
            if !has_preset_json {
                sqlx::query(r#"ALTER TABLE watcher_presets ADD COLUMN preset_json TEXT NOT NULL"#)
                    .execute(self.get_sqlx_pool())
                    .await
                    .ok();
            }
            // account_hash Unique 보장
            let has_unique: Option<String> = sqlx::query_scalar(
                r#"SELECT INDEX_NAME FROM information_schema.statistics
                   WHERE table_schema = DATABASE() AND table_name = 'watcher_presets' AND NON_UNIQUE = 0 AND COLUMN_NAME = 'account_hash' LIMIT 1"#
            ).fetch_optional(self.get_sqlx_pool()).await.map_err(|e| StorageError::Database(format!("watcher_presets 인덱스 확인 실패: {}", e)))?;
            if has_unique.is_none() {
                let _ = sqlx::query(r#"ALTER TABLE watcher_presets ADD UNIQUE KEY uniq_account_hash (account_hash)"#)
                    .execute(self.get_sqlx_pool()).await;
            }
        }

        info!("데이터베이스 스키마 마이그레이션 완료");
        Ok(())
    }

    // Helper method to convert SQL error to StorageError
    #[allow(dead_code)]
    fn sql_error<E: std::fmt::Display>(e: E) -> StorageError {
        StorageError::Database(format!("SQL error: {}", e))
    }

    // Convert timestamp to MySQL datetime format
    pub fn timestamp_to_datetime(timestamp: i64) -> String {
        let dt = Utc
            .timestamp_opt(timestamp, 0)
            .single()
            .unwrap_or_else(|| Utc::now());
        dt.format("%Y-%m-%d %H:%M:%S").to_string()
    }

    // Convert MySQL datetime to timestamp
    pub fn datetime_to_timestamp(datetime: &str) -> Result<i64> {
        let naive = NaiveDateTime::parse_from_str(datetime, "%Y-%m-%d %H:%M:%S")
            .map_err(|e| StorageError::General(format!("Failed to parse datetime: {}", e)))?;

        let datetime = DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc);
        Ok(datetime.timestamp())
    }

    // pool 가져오기
    // mysql_async pool removed

    /// sqlx pool getter (transition)
    pub fn get_sqlx_pool(&self) -> &SqlxMySqlPool {
        &self.sqlx_pool
    }

    pub fn decrypt_text(
        &self,
        account_hash: &str,
        group_id: i32,
        watcher_id: i32,
        data: Vec<u8>,
    ) -> String {
        let cfg = crate::server::app_state::AppState::get_config();
        if let Some(kv) = cfg.server_encode_key.as_ref() {
            if kv.len() == 32 {
                let key: &[u8; 32] = kv.as_slice().try_into().expect("len checked");
                let aad = format!("{}:{}:{}", account_hash, group_id, watcher_id);
                if let Ok(pt) = crate::utils::crypto::aead_decrypt(key, &data, aad.as_bytes()) {
                    return String::from_utf8_lossy(&pt).to_string();
                }
            }
        }
        String::from_utf8_lossy(&data).to_string()
    }

    pub async fn migrate_encrypt_paths(&self, batch_size: usize) -> Result<u64> {
        use sqlx::Row;
        let cfg = crate::server::app_state::AppState::get_config();
        let Some(kv) = cfg.server_encode_key.as_ref() else {
            return Ok(0);
        };
        if kv.len() != 32 {
            return Ok(0);
        }
        let key: &[u8; 32] = kv.as_slice().try_into().expect("len checked");

        let mut tx = self
            .sqlx_pool
            .begin()
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?;
        // Pick candidate rows: empty eq_index or empty token_path
        let rows = sqlx::query(
            r#"SELECT file_id, account_hash, group_id, watcher_id, file_path, filename
               FROM files
               WHERE (eq_index IS NULL OR LENGTH(eq_index)=0) OR (token_path IS NULL OR token_path='')
               LIMIT ?"#
        )
            .bind(batch_size as i64)
            .fetch_all(&mut *tx)
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?;

        let mut updated: u64 = 0;
        for row in rows.into_iter() {
            let file_id: u64 = row.try_get("file_id").unwrap_or(0);
            let account_hash: String = row.try_get("account_hash").unwrap_or_default();
            let group_id: i32 = row.try_get("group_id").unwrap_or(0);
            let watcher_id: i32 = row.try_get("watcher_id").unwrap_or(0);
            let file_path_b: Vec<u8> = row.try_get("file_path").unwrap_or_default();
            let filename_b: Vec<u8> = row.try_get("filename").unwrap_or_default();

            // Try decrypt to detect if already encrypted; if decryption succeeds, skip encrypting again
            let aad = format!("{}:{}:{}", account_hash, group_id, watcher_id);
            let maybe_plain_path =
                match crate::utils::crypto::aead_decrypt(key, &file_path_b, aad.as_bytes()) {
                    Ok(pt) => Some(pt),
                    Err(_) => None,
                };
            let maybe_plain_name =
                match crate::utils::crypto::aead_decrypt(key, &filename_b, aad.as_bytes()) {
                    Ok(pt) => Some(pt),
                    Err(_) => None,
                };

            let (cipher_path, cipher_name, eq_index, token_path) = if let (Some(_), Some(_)) =
                (maybe_plain_path.as_ref(), maybe_plain_name.as_ref())
            {
                // Already encrypted; recompute indexes from decrypted plaintext
                let plain_path =
                    String::from_utf8_lossy(maybe_plain_path.as_ref().unwrap()).to_string();
                let salt = crate::utils::crypto::derive_salt(key, "meta-index", &account_hash);
                (
                    file_path_b,
                    filename_b,
                    crate::utils::crypto::make_eq_index(&salt, &plain_path),
                    crate::utils::crypto::make_token_path(&salt, &plain_path),
                )
            } else {
                // Treat current bytes as plaintext UTF-8
                let plain_path = String::from_utf8_lossy(&file_path_b).to_string();
                let plain_name = String::from_utf8_lossy(&filename_b).to_string();
                let ct_path =
                    crate::utils::crypto::aead_encrypt(key, plain_path.as_bytes(), aad.as_bytes());
                let ct_name =
                    crate::utils::crypto::aead_encrypt(key, plain_name.as_bytes(), aad.as_bytes());
                let salt = crate::utils::crypto::derive_salt(key, "meta-index", &account_hash);
                (
                    ct_path,
                    ct_name,
                    crate::utils::crypto::make_eq_index(&salt, &plain_path),
                    crate::utils::crypto::make_token_path(&salt, &plain_path),
                )
            };

            sqlx::query(r#"UPDATE files SET file_path = ?, filename = ?, eq_index = ?, token_path = ? WHERE file_id = ?"#)
                .bind(cipher_path)
                .bind(cipher_name)
                .bind(eq_index)
                .bind(token_path)
                .bind(file_id)
                .execute(&mut *tx)
                .await
                .map_err(|e| StorageError::Database(e.to_string()))?;
            updated += 1;
        }

        tx.commit()
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?;
        Ok(updated)
    }
}

#[async_trait]
impl Storage for MySqlStorage {
    /// Get the storage instance as Any for downcasting
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    /// Create a new account
    async fn create_account(&self, account: &crate::models::account::Account) -> Result<()> {
        MySqlAccountExt::create_account(self, account).await
    }

    /// Get account by ID
    async fn get_account_by_id(&self, id: &str) -> Result<Option<crate::models::account::Account>> {
        MySqlAccountExt::get_account_by_id(self, id).await
    }

    /// Get account by email
    async fn get_account_by_email(
        &self,
        email: &str,
    ) -> Result<Option<crate::models::account::Account>> {
        MySqlAccountExt::get_account_by_email(self, email).await
    }

    /// Get account by hash
    async fn get_account_by_hash(
        &self,
        account_hash: &str,
    ) -> Result<Option<crate::models::account::Account>> {
        MySqlAccountExt::get_account_by_hash(self, account_hash).await
    }

    /// Update account
    async fn update_account(&self, account: &crate::models::account::Account) -> Result<()> {
        MySqlAccountExt::update_account(self, account).await
    }

    /// Delete account
    async fn delete_account(&self, account_hash: &str) -> Result<()> {
        MySqlAccountExt::delete_account(self, account_hash).await
    }

    /// Create an authentication token
    async fn create_auth_token(&self, auth_token: &crate::models::auth::AuthToken) -> Result<()> {
        MySqlAuthExt::create_auth_token(self, auth_token).await
    }

    /// Get authentication token
    async fn get_auth_token(&self, token: &str) -> Result<Option<crate::models::auth::AuthToken>> {
        MySqlAuthExt::get_auth_token(self, token).await
    }

    /// Validate authentication token
    async fn validate_auth_token(&self, token: &str, account_hash: &str) -> Result<bool> {
        MySqlAuthExt::validate_auth_token(self, token, account_hash).await
    }

    /// Update authentication token
    async fn update_auth_token(&self, auth_token: &crate::models::auth::AuthToken) -> Result<()> {
        MySqlAuthExt::update_auth_token(self, auth_token).await
    }

    /// Delete authentication token
    async fn delete_auth_token(&self, token: &str) -> Result<()> {
        MySqlAuthExt::delete_auth_token(self, token).await
    }

    /// Register device
    async fn register_device(&self, device: &crate::models::device::Device) -> Result<()> {
        MySqlDeviceExt::register_device(self, device).await
    }

    /// Get device
    async fn get_device(
        &self,
        account_hash: &str,
        device_hash: &str,
    ) -> Result<Option<crate::models::device::Device>> {
        MySqlDeviceExt::get_device(self, account_hash, device_hash).await
    }

    /// List devices
    async fn list_devices(&self, account_hash: &str) -> Result<Vec<crate::models::device::Device>> {
        MySqlDeviceExt::list_devices(self, account_hash).await
    }

    /// Update device
    async fn update_device(&self, device: &crate::models::device::Device) -> Result<()> {
        MySqlDeviceExt::update_device(self, device).await
    }

    /// Delete device
    async fn delete_device(&self, account_hash: &str, device_hash: &str) -> Result<()> {
        MySqlDeviceExt::delete_device(self, account_hash, device_hash).await
    }

    /// Validate device
    async fn validate_device(&self, account_hash: &str, device_hash: &str) -> Result<bool> {
        MySqlDeviceExt::validate_device(self, account_hash, device_hash).await
    }

    /// Store file information
    async fn store_file_info(&self, file_info: crate::models::file::FileInfo) -> Result<u64> {
        MySqlFileExt::store_file_info(self, file_info).await
    }

    /// Get file information
    async fn get_file_info(&self, file_id: u64) -> Result<Option<crate::models::file::FileInfo>> {
        MySqlFileExt::get_file_info(self, file_id).await
    }

    /// Get file information by path
    async fn get_file_info_by_path(
        &self,
        account_hash: &str,
        file_path: &str,
        group_id: i32,
    ) -> Result<Option<crate::models::file::FileInfo>> {
        MySqlFileExt::get_file_info_by_path(self, account_hash, file_path, group_id).await
    }

    /// Get file information (including deleted files)
    async fn get_file_info_include_deleted(
        &self,
        file_id: u64,
    ) -> Result<Option<(crate::models::file::FileInfo, bool)>> {
        MySqlFileExt::get_file_info_include_deleted(self, file_id).await
    }

    /// Get file by hash
    async fn get_file_by_hash(
        &self,
        account_hash: &str,
        file_hash: &str,
    ) -> Result<Option<crate::models::file::FileInfo>> {
        MySqlFileExt::get_file_by_hash(self, account_hash, file_hash).await
    }

    /// Find file by path and name
    async fn find_file_by_path_and_name(
        &self,
        account_hash: &str,
        file_path: &str,
        filename: &str,
        revision: i64,
    ) -> Result<Option<crate::models::file::FileInfo>> {
        MySqlFileExt::find_file_by_path_and_name(self, account_hash, file_path, filename, revision)
            .await
    }

    /// Find file by criteria
    async fn find_file_by_criteria(
        &self,
        account_hash: &str,
        group_id: i32,
        watcher_id: i32,
        file_path: &str,
        filename: &str,
    ) -> Result<Option<crate::models::file::FileInfo>> {
        MySqlFileExt::find_file_by_criteria(
            self,
            account_hash,
            group_id,
            watcher_id,
            file_path,
            filename,
        )
        .await
    }

    /// Delete file
    async fn delete_file(&self, account_hash: &str, file_id: u64) -> Result<()> {
        MySqlFileExt::delete_file(self, account_hash, file_id).await
    }

    /// List files
    async fn list_files(
        &self,
        account_hash: &str,
        group_id: i32,
        upload_time_from: Option<i64>,
    ) -> Result<Vec<crate::models::file::FileInfo>> {
        MySqlFileExt::list_files(self, account_hash, group_id, upload_time_from).await
    }

    /// List files except those uploaded by a specific device
    async fn list_files_except_device(
        &self,
        account_hash: &str,
        group_id: i32,
        exclude_device_hash: &str,
        upload_time_from: Option<i64>,
    ) -> Result<Vec<crate::models::file::FileInfo>> {
        MySqlFileExt::list_files_except_device(
            self,
            account_hash,
            group_id,
            exclude_device_hash,
            upload_time_from,
        )
        .await
    }

    /// Store file data
    async fn store_file_data(&self, file_id: u64, data_bytes: Vec<u8>) -> Result<()> {
        MySqlFileExt::store_file_data(self, file_id, data_bytes).await
    }

    /// Get file data
    async fn get_file_data(&self, file_id: u64) -> Result<Option<Vec<u8>>> {
        MySqlFileExt::get_file_data(self, file_id).await
    }

    /// Get encryption key
    async fn get_encryption_key(&self, account_hash: &str) -> Result<Option<String>> {
        MySqlFileExt::get_encryption_key(self, account_hash).await
    }

    /// Store encryption key
    async fn store_encryption_key(&self, account_hash: &str, encryption_key: &str) -> Result<()> {
        MySqlFileExt::store_encryption_key(self, account_hash, encryption_key).await
    }

    /// Register watcher group
    async fn register_watcher_group(
        &self,
        account_hash: &str,
        device_hash: &str,
        watcher_group: &crate::models::watcher::WatcherGroup,
    ) -> Result<i32> {
        MySqlWatcherExt::register_watcher_group(self, account_hash, device_hash, watcher_group)
            .await
    }

    /// Get watcher groups
    async fn get_watcher_groups(
        &self,
        account_hash: &str,
    ) -> Result<Vec<crate::models::watcher::WatcherGroup>> {
        MySqlWatcherExt::get_watcher_groups(self, account_hash).await
    }

    /// Get user watcher group
    async fn get_user_watcher_group(
        &self,
        account_hash: &str,
        group_id: i32,
    ) -> Result<Option<crate::models::watcher::WatcherGroup>> {
        MySqlWatcherExt::get_user_watcher_group(self, account_hash, group_id).await
    }

    /// Update watcher group
    async fn update_watcher_group(
        &self,
        account_hash: &str,
        watcher_group: &crate::models::watcher::WatcherGroup,
    ) -> Result<()> {
        MySqlWatcherExt::update_watcher_group(self, account_hash, watcher_group).await
    }

    /// Delete watcher group
    async fn delete_watcher_group(&self, account_hash: &str, group_id: i32) -> Result<()> {
        MySqlWatcherExt::delete_watcher_group(self, account_hash, group_id).await
    }

    /// Get watcher group by account and ID
    async fn get_watcher_group_by_account_and_id(
        &self,
        account_hash: &str,
        group_id: i32,
    ) -> Result<Option<crate::sync::WatcherGroupData>> {
        MySqlWatcherExt::get_watcher_group_by_account_and_id(self, account_hash, group_id).await
    }

    /// Get watcher preset
    async fn get_watcher_preset(&self, account_hash: &str) -> Result<Vec<String>> {
        MySqlWatcherExt::get_watcher_preset(self, account_hash).await
    }

    /// Register watcher preset proto
    async fn register_watcher_preset_proto(
        &self,
        account_hash: &str,
        device_hash: &str,
        presets: Vec<String>,
    ) -> Result<()> {
        MySqlWatcherExt::register_watcher_preset_proto(self, account_hash, device_hash, presets)
            .await
    }

    /// Update watcher preset proto
    async fn update_watcher_preset_proto(
        &self,
        account_hash: &str,
        device_hash: &str,
        presets: Vec<String>,
    ) -> Result<()> {
        MySqlWatcherExt::update_watcher_preset_proto(self, account_hash, device_hash, presets).await
    }

    // Watcher 관련 메서드
    async fn find_watcher_by_folder(
        &self,
        account_hash: &str,
        group_id: i32,
        folder: &str,
    ) -> crate::storage::Result<Option<i32>> {
        <Self as MySqlWatcherExt>::find_watcher_by_folder(self, account_hash, group_id, folder)
            .await
    }

    // removed: create_watcher without conditions (use create_watcher_with_conditions instead)

    async fn create_watcher_with_conditions(
        &self,
        account_hash: &str,
        group_id: i32,
        watcher_data: &crate::sync::WatcherData,
        timestamp: i64,
    ) -> Result<i32> {
        <Self as MySqlWatcherExt>::create_watcher_with_conditions(
            self,
            account_hash,
            group_id,
            watcher_data,
            timestamp,
        )
        .await
    }

    async fn get_watcher_by_group_and_id(
        &self,
        account_hash: &str,
        group_id: i32,
        watcher_id: i32,
    ) -> Result<Option<crate::sync::WatcherData>> {
        <Self as MySqlWatcherExt>::get_watcher_by_group_and_id(
            self,
            account_hash,
            group_id,
            watcher_id,
        )
        .await
    }

    async fn get_server_group_id(
        &self,
        account_hash: &str,
        client_group_id: i32,
    ) -> Result<Option<i32>> {
        <Self as MySqlWatcherExt>::get_server_group_id(self, account_hash, client_group_id).await
    }

    async fn get_server_ids(
        &self,
        account_hash: &str,
        client_group_id: i32,
        client_watcher_id: i32,
    ) -> Result<Option<(i32, i32)>> {
        <Self as MySqlWatcherExt>::get_server_ids(
            self,
            account_hash,
            client_group_id,
            client_watcher_id,
        )
        .await
    }

    /// 파일 ID로 존재 여부와 삭제 상태 확인
    async fn check_file_exists(&self, file_id: u64) -> Result<(bool, bool)> {
        MySqlFileExt::check_file_exists(self, file_id).await
    }

    /// Create a new watcher condition
    async fn create_watcher_condition(
        &self,
        condition: &crate::models::WatcherCondition,
    ) -> Result<i64> {
        MySqlWatcherExt::create_watcher_condition(self, condition).await
    }

    /// Get all conditions for a watcher
    async fn get_watcher_conditions(
        &self,
        account_hash: &str,
        watcher_id: i32,
    ) -> Result<Vec<crate::models::WatcherCondition>> {
        MySqlWatcherExt::get_watcher_conditions(self, account_hash, watcher_id).await
    }

    /// Update a watcher condition
    async fn update_watcher_condition(
        &self,
        condition: &crate::models::WatcherCondition,
    ) -> Result<()> {
        MySqlWatcherExt::update_watcher_condition(self, condition).await
    }

    /// Delete a watcher condition
    async fn delete_watcher_condition(&self, condition_id: i64) -> Result<()> {
        MySqlWatcherExt::delete_watcher_condition(self, condition_id).await
    }

    /// Delete all conditions for a watcher
    async fn delete_all_watcher_conditions(&self, watcher_id: i32) -> Result<()> {
        MySqlWatcherExt::delete_all_watcher_conditions(self, watcher_id).await
    }

    /// Save watcher conditions (replace all existing conditions)
    async fn save_watcher_conditions(
        &self,
        watcher_id: i32,
        conditions: &[crate::models::WatcherCondition],
    ) -> Result<()> {
        MySqlWatcherExt::save_watcher_conditions(self, watcher_id, conditions).await
    }

    /// Convert server watcher_id to client watcher_id
    async fn get_client_watcher_id(
        &self,
        account_hash: &str,
        server_group_id: i32,
        server_watcher_id: i32,
    ) -> Result<Option<(i32, i32)>> {
        // 서버 watcher_id가 0이면 클라이언트 watcher_id도 0
        if server_watcher_id == 0 {
            return Ok(Some((0, 0)));
        }

        // 서버 watcher_id로 클라이언트 watcher_id 조회(sqlx)
        let result: Option<(i32, i32)> = sqlx::query_as(
            r#"SELECT local_group_id, watcher_id FROM watchers WHERE id = ? AND account_hash = ?"#,
        )
        .bind(server_watcher_id)
        .bind(account_hash)
        .fetch_optional(self.get_sqlx_pool())
        .await
        .map_err(|e| StorageError::Database(format!("Failed to get client watcher_id: {}", e)))?;
        Ok(result)
    }

    // Removed legacy WatcherPreset aliases (use proto-based methods only)

    // WatcherCondition 관련 메서드들
    async fn register_watcher_condition(
        &self,
        account_hash: &str,
        condition: &WatcherCondition,
    ) -> Result<i64> {
        Storage::create_watcher_condition(self, condition).await
    }

    async fn get_watcher_condition(&self, condition_id: i64) -> Result<Option<WatcherCondition>> {
        let result: Option<(i64, String, i32, String, String, String, String, i64, i64)> = sqlx::query_as(
            r#"SELECT id, account_hash, watcher_id, condition_type, `key`, value, operator, created_at, updated_at
              FROM watcher_conditions
              WHERE id = ?"#
        )
        .bind(condition_id)
        .fetch_optional(self.get_sqlx_pool())
        .await
        .map_err(|e| StorageError::Database(format!("Failed to get watcher condition: {}", e)))?;

        if let Some((
            id,
            account_hash,
            watcher_id,
            condition_type_str,
            key,
            value_str,
            operator,
            created_at,
            updated_at,
        )) = result
        {
            let condition_type = match condition_type_str.as_str() {
                "union" => crate::models::watcher::ConditionType::Union,
                "subtract" => crate::models::watcher::ConditionType::Subtract,
                _ => crate::models::watcher::ConditionType::Union,
            };

            let condition = WatcherCondition {
                id: Some(id),
                account_hash,
                watcher_id,
                local_watcher_id: 0, // not needed for lookup
                local_group_id: 0,   // not needed for lookup
                condition_type,
                key,
                value: serde_json::from_str(&value_str).unwrap_or_else(|_| vec![value_str.clone()]),
                operator,
                created_at: DateTime::from_timestamp(created_at, 0).unwrap_or_else(|| Utc::now()),
                updated_at: DateTime::from_timestamp(updated_at, 0).unwrap_or_else(|| Utc::now()),
            };

            Ok(Some(condition))
        } else {
            Ok(None)
        }
    }

    async fn get_watcher_conditions_by_watcher(
        &self,
        account_hash: &str,
        watcher_id: i32,
    ) -> Result<Vec<WatcherCondition>> {
        Storage::get_watcher_conditions(self, account_hash, watcher_id).await
    }

    // 중복 검사 메서드들
    async fn check_duplicate_watcher_group(
        &self,
        account_hash: &str,
        local_group_id: i32,
    ) -> Result<Option<i32>> {
        let server_id: Option<i32> = sqlx::query_scalar(
            r#"SELECT id FROM watcher_groups WHERE account_hash = ? AND group_id = ?"#,
        )
        .bind(account_hash)
        .bind(local_group_id)
        .fetch_optional(self.get_sqlx_pool())
        .await
        .map_err(|e| {
            StorageError::Database(format!("Failed to check duplicate watcher group: {}", e))
        })?;

        Ok(server_id)
    }

    async fn check_duplicate_watcher(
        &self,
        account_hash: &str,
        local_watcher_id: i32,
    ) -> Result<Option<i32>> {
        let server_id: Option<i32> = sqlx::query_scalar(
            r#"SELECT id FROM watchers WHERE account_hash = ? AND watcher_id = ?"#,
        )
        .bind(account_hash)
        .bind(local_watcher_id)
        .fetch_optional(self.get_sqlx_pool())
        .await
        .map_err(|e| StorageError::Database(format!("Failed to check duplicate watcher: {}", e)))?;

        Ok(server_id)
    }

    // FileNotice 관련 메서드들
    async fn store_file_notice(&self, file_notice: &FileNotice) -> Result<()> {
        sqlx::query(r#"INSERT INTO file_notices (account_hash, device_hash, path, action, timestamp, file_id, group_id, watcher_id, revision)
              VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
              ON DUPLICATE KEY UPDATE
              action = VALUES(action),
              timestamp = VALUES(timestamp),
              group_id = VALUES(group_id),
              watcher_id = VALUES(watcher_id),
              revision = VALUES(revision)"#)
            .bind(&file_notice.account_hash)
            .bind(&file_notice.device_hash)
            .bind(&file_notice.path)
            .bind(&file_notice.action)
            .bind(file_notice.timestamp)
            .bind(file_notice.file_id)
            .bind(file_notice.group_id)
            .bind(file_notice.watcher_id)
            .bind(file_notice.revision)
            .execute(self.get_sqlx_pool())
            .await
            .map_err(|e| StorageError::Database(format!("Failed to store file notice: {}", e)))?;

        Ok(())
    }

    async fn get_file_notices(
        &self,
        account_hash: &str,
        device_hash: &str,
    ) -> Result<Vec<FileNotice>> {
        let notices: Vec<(String, String, String, String, i64, u64, i32, i32, i64)> = sqlx::query_as(
            r#"SELECT account_hash, device_hash, path, action, timestamp, file_id, group_id, watcher_id, revision
              FROM file_notices
              WHERE account_hash = ? AND device_hash = ?"#
        )
        .bind(account_hash)
        .bind(device_hash)
        .fetch_all(self.get_sqlx_pool())
        .await
        .map_err(|e| StorageError::Database(format!("Failed to get file notices: {}", e)))?;

        let mut result = Vec::new();
        for (
            account_hash,
            device_hash,
            path,
            action,
            timestamp,
            file_id,
            group_id,
            watcher_id,
            revision,
        ) in notices
        {
            result.push(FileNotice {
                account_hash,
                device_hash,
                path,
                action,
                timestamp,
                file_id,
                group_id,
                watcher_id,
                revision,
            });
        }

        Ok(result)
    }

    async fn delete_file_notice(
        &self,
        account_hash: &str,
        device_hash: &str,
        file_id: u64,
    ) -> Result<()> {
        sqlx::query(r#"DELETE FROM file_notices WHERE account_hash = ? AND device_hash = ? AND file_id = ?"#)
            .bind(account_hash)
            .bind(device_hash)
            .bind(file_id)
            .execute(self.get_sqlx_pool())
            .await
            .map_err(|e| StorageError::Database(format!("Failed to delete file notice: {}", e)))?;

        Ok(())
    }

    // Version management methods implementation
    async fn get_file_history(
        &self,
        account_hash: &str,
        file_path: &str,
        group_id: i32,
    ) -> Result<Vec<crate::models::file::SyncFile>> {
        use sqlx::Row;
        debug!(
            "Getting file history for path: {} in group: {}",
            file_path, group_id
        );

        let rows = sqlx::query(
            r#"SELECT
                    account_hash,
                    device_hash,
                    group_id,
                    watcher_id,
                    file_id,
                    filename,
                    file_hash,
                    file_path,
                    size AS file_size,
                    UNIX_TIMESTAMP(created_time) AS created_ts,
                    UNIX_TIMESTAMP(updated_time) AS updated_ts,
                    is_deleted,
                    revision
               FROM files
               WHERE account_hash = ? AND file_path = ? AND group_id = ?
               ORDER BY revision DESC"#,
        )
        .bind(account_hash)
        .bind(file_path)
        .bind(group_id)
        .fetch_all(self.get_sqlx_pool())
        .await
        .map_err(|e| StorageError::Database(format!("Failed to get file history: {}", e)))?;

        let mut files = Vec::with_capacity(rows.len());
        for row in rows {
            let account_hash: String = row.try_get("account_hash").unwrap_or_default();
            let device_hash: String = row.try_get("device_hash").unwrap_or_default();
            let group_id: i32 = row.try_get("group_id").unwrap_or(0);
            let watcher_id: i32 = row.try_get("watcher_id").unwrap_or(0);
            let file_id: u64 = row.try_get("file_id").unwrap_or(0);
            let filename: String = row.try_get("filename").unwrap_or_default();
            let file_hash: String = row.try_get("file_hash").unwrap_or_default();
            let file_path: String = row.try_get("file_path").unwrap_or_default();
            let file_size: i64 = row.try_get("file_size").unwrap_or(0);
            let created_ts: i64 = row.try_get("created_ts").unwrap_or(0);
            let updated_ts: i64 = row.try_get("updated_ts").unwrap_or(0);
            let is_deleted: bool = row.try_get("is_deleted").unwrap_or(false);
            let revision: i64 = row.try_get("revision").unwrap_or(0);

            let upload_time = chrono::DateTime::from_timestamp(created_ts, 0)
                .unwrap_or_else(|| chrono::Utc::now());
            let last_updated = chrono::DateTime::from_timestamp(updated_ts, 0)
                .unwrap_or_else(|| chrono::Utc::now());

            files.push(crate::models::file::SyncFile {
                id: format!("{}_{}", file_id, revision),
                user_id: account_hash,
                device_hash,
                group_id,
                watcher_id,
                file_id,
                filename,
                file_hash,
                file_path,
                file_size,
                mime_type: "application/octet-stream".to_string(),
                modified_time: updated_ts,
                is_encrypted: false,
                upload_time,
                last_updated,
                is_deleted,
                revision,
            });
        }

        Ok(files)
    }

    async fn get_file_versions_by_id(
        &self,
        account_hash: &str,
        file_id: u64,
    ) -> Result<Vec<crate::models::file::SyncFile>> {
        use sqlx::Row;
        debug!("Getting file versions for file_id: {}", file_id);

        let rows = sqlx::query(
            r#"SELECT
                    account_hash,
                    device_hash,
                    group_id,
                    watcher_id,
                    file_id,
                    filename,
                    file_hash,
                    file_path,
                    size AS file_size,
                    UNIX_TIMESTAMP(created_time) AS created_ts,
                    UNIX_TIMESTAMP(updated_time) AS updated_ts,
                    is_deleted,
                    revision
               FROM files
               WHERE account_hash = ? AND file_id = ?
               ORDER BY revision DESC"#,
        )
        .bind(account_hash)
        .bind(file_id as i64)
        .fetch_all(self.get_sqlx_pool())
        .await
        .map_err(|e| StorageError::Database(format!("Failed to get file versions: {}", e)))?;

        let mut files = Vec::with_capacity(rows.len());
        for row in rows {
            let account_hash: String = row.try_get("account_hash").unwrap_or_default();
            let device_hash: String = row.try_get("device_hash").unwrap_or_default();
            let group_id: i32 = row.try_get("group_id").unwrap_or(0);
            let watcher_id: i32 = row.try_get("watcher_id").unwrap_or(0);
            let file_id: u64 = row.try_get("file_id").unwrap_or(0);
            let filename: String = row.try_get("filename").unwrap_or_default();
            let file_hash: String = row.try_get("file_hash").unwrap_or_default();
            let file_path: String = row.try_get("file_path").unwrap_or_default();
            let file_size: i64 = row.try_get("file_size").unwrap_or(0);
            let created_ts: i64 = row.try_get("created_ts").unwrap_or(0);
            let updated_ts: i64 = row.try_get("updated_ts").unwrap_or(0);
            let is_deleted: bool = row.try_get("is_deleted").unwrap_or(false);
            let revision: i64 = row.try_get("revision").unwrap_or(0);

            let upload_time = chrono::DateTime::from_timestamp(created_ts, 0)
                .unwrap_or_else(|| chrono::Utc::now());
            let last_updated = chrono::DateTime::from_timestamp(updated_ts, 0)
                .unwrap_or_else(|| chrono::Utc::now());

            files.push(crate::models::file::SyncFile {
                id: format!("{}_{}", file_id, revision),
                user_id: account_hash,
                device_hash,
                group_id,
                watcher_id,
                file_id,
                filename,
                file_hash,
                file_path,
                file_size,
                mime_type: "application/octet-stream".to_string(),
                modified_time: updated_ts,
                is_encrypted: false,
                upload_time,
                last_updated,
                is_deleted,
                revision,
            });
        }

        Ok(files)
    }

    async fn get_file_by_revision(
        &self,
        account_hash: &str,
        file_id: u64,
        revision: i64,
    ) -> Result<crate::models::file::SyncFile> {
        use sqlx::Row;
        debug!(
            "Getting file by revision: file_id={}, revision={}",
            file_id, revision
        );

        let row_opt = sqlx::query(
            r#"SELECT
                    account_hash,
                    device_hash,
                    group_id,
                    watcher_id,
                    file_id,
                    filename,
                    file_hash,
                    file_path,
                    size AS file_size,
                    UNIX_TIMESTAMP(created_time) AS created_ts,
                    UNIX_TIMESTAMP(updated_time) AS updated_ts,
                    is_deleted,
                    revision
               FROM files
               WHERE account_hash = ? AND file_id = ? AND revision = ?
               LIMIT 1"#,
        )
        .bind(account_hash)
        .bind(file_id as i64)
        .bind(revision)
        .fetch_optional(self.get_sqlx_pool())
        .await
        .map_err(|e| StorageError::Database(format!("Failed to get file by revision: {}", e)))?;

        if let Some(row) = row_opt {
            let account_hash: String = row.try_get("account_hash").unwrap_or_default();
            let device_hash: String = row.try_get("device_hash").unwrap_or_default();
            let group_id: i32 = row.try_get("group_id").unwrap_or(0);
            let watcher_id: i32 = row.try_get("watcher_id").unwrap_or(0);
            let file_id: u64 = row.try_get("file_id").unwrap_or(0);
            let filename: String = row.try_get("filename").unwrap_or_default();
            let file_hash: String = row.try_get("file_hash").unwrap_or_default();
            let file_path: String = row.try_get("file_path").unwrap_or_default();
            let file_size: i64 = row.try_get("file_size").unwrap_or(0);
            let created_ts: i64 = row.try_get("created_ts").unwrap_or(0);
            let updated_ts: i64 = row.try_get("updated_ts").unwrap_or(0);
            let is_deleted: bool = row.try_get("is_deleted").unwrap_or(false);
            let revision: i64 = row.try_get("revision").unwrap_or(0);

            let upload_time = chrono::DateTime::from_timestamp(created_ts, 0)
                .unwrap_or_else(|| chrono::Utc::now());
            let last_updated = chrono::DateTime::from_timestamp(updated_ts, 0)
                .unwrap_or_else(|| chrono::Utc::now());

            Ok(crate::models::file::SyncFile {
                id: format!("{}_{}", file_id, revision),
                user_id: account_hash,
                device_hash,
                group_id,
                watcher_id,
                file_id,
                filename,
                file_hash,
                file_path,
                file_size,
                mime_type: "application/octet-stream".to_string(),
                modified_time: updated_ts,
                is_encrypted: false,
                upload_time,
                last_updated,
                is_deleted,
                revision,
            })
        } else {
            Err(StorageError::NotFound(format!(
                "File not found: file_id={}, revision={}",
                file_id, revision
            )))
        }
    }

    async fn store_file(&self, file: &crate::models::file::SyncFile) -> Result<()> {
        debug!(
            "Storing file: file_id={}, revision={}",
            file.file_id, file.revision
        );

        // Carry over previous key_id for this file if present (SyncFile does not contain key_id)
        let prev_key_id: Option<String> = sqlx::query_scalar(
            r#"SELECT key_id FROM files WHERE account_hash = ? AND file_id = ? ORDER BY updated_time DESC LIMIT 1"#
        )
        .bind(&file.user_id)
        .bind(file.file_id)
        .fetch_optional(self.get_sqlx_pool())
        .await
        .map_err(|e| StorageError::Database(format!("Failed to fetch previous key_id: {}", e)))?
        .flatten();
        if let Some(ref kid) = prev_key_id {
            debug!(
                "🔐 Carrying over key_id for new version: {} -> {}",
                file.file_id, kid
            );
        } else {
            debug!(
                "🔐 No previous key_id found for file_id={}, inserting NULL",
                file.file_id
            );
        }

        sqlx::query(
            r#"INSERT INTO files (
                file_id, account_hash, device_hash, file_path, filename, file_hash, size,
                is_deleted, revision, created_time, updated_time, group_id, watcher_id,
                server_group_id, server_watcher_id, key_id
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, FROM_UNIXTIME(?), FROM_UNIXTIME(?), ?, ?, ?, ?, ?)"#
        )
        .bind(file.file_id)
        .bind(&file.user_id)
        .bind(&file.device_hash)
        .bind(&file.file_path)
        .bind(&file.filename)
        .bind(&file.file_hash)
        .bind(file.file_size)
        .bind(file.is_deleted)
        .bind(file.revision)
        .bind(file.upload_time.timestamp())
        .bind(file.last_updated.timestamp())
        .bind(file.group_id)
        .bind(file.watcher_id)
        .bind(file.group_id)
        .bind(file.watcher_id)
        .bind(prev_key_id.as_deref())
        .execute(self.get_sqlx_pool())
        .await
        .map_err(|e| StorageError::Database(format!("Failed to store file: {}", e)))?;

        Ok(())
    }

    async fn get_devices_for_account(&self, account_hash: &str) -> Result<Vec<Device>> {
        debug!("Getting devices for account: {}", account_hash);
        // 위임하여 단일 구현 유지
        MySqlDeviceExt::list_devices(self, account_hash).await
    }

    /// Purge logically deleted files older than ttl_secs; return affected rows
    async fn purge_deleted_files_older_than(&self, ttl_secs: i64) -> Result<u64> {
        <MySqlStorage as MySqlFileExt>::purge_deleted_files_older_than(self, ttl_secs).await
    }

    /// Trim older revisions per (account_hash, file_path) beyond max_revisions; return affected rows
    async fn trim_old_revisions(&self, max_revisions: i32) -> Result<u64> {
        <MySqlStorage as MySqlFileExt>::trim_old_revisions(self, max_revisions).await
    }

    // Stub implementations for missing Storage trait methods
    async fn health_check(&self) -> Result<bool> {
        Ok(true)
    }

    async fn get_metrics(&self) -> Result<StorageMetrics> {
        Ok(StorageMetrics {
            total_queries: 0,
            successful_queries: 0,
            failed_queries: 0,
            average_query_time_ms: 0.0,
            active_connections: 0,
            idle_connections: 0,
            cache_hits: 0,
            cache_misses: 0,
        })
    }

    async fn close(&self) -> Result<()> {
        Ok(())
    }

    async fn batch_create_accounts(&self, _accounts: &[Account]) -> Result<Vec<bool>> {
        Err(StorageError::NotImplemented(
            "batch_create_accounts not implemented".to_string(),
        ))
    }

    async fn list_accounts(
        &self,
        _limit: Option<u32>,
        _offset: Option<u64>,
    ) -> Result<Vec<Account>> {
        Err(StorageError::NotImplemented(
            "list_accounts not implemented".to_string(),
        ))
    }

    async fn cleanup_expired_tokens(&self) -> Result<u64> {
        Ok(0)
    }

    async fn batch_store_files(&self, _files: Vec<FileInfo>) -> Result<Vec<(u64, bool)>> {
        Err(StorageError::NotImplemented(
            "batch_store_files not implemented".to_string(),
        ))
    }

    async fn batch_delete_files(
        &self,
        _account_hash: &str,
        _file_ids: Vec<u64>,
    ) -> Result<Vec<bool>> {
        Err(StorageError::NotImplemented(
            "batch_delete_files not implemented".to_string(),
        ))
    }

    async fn store_file_data_stream(
        &self,
        _file_id: u64,
        _data_stream: Box<dyn futures::Stream<Item = Result<bytes::Bytes>> + Send + Unpin>,
    ) -> Result<()> {
        Err(StorageError::NotImplemented(
            "store_file_data_stream not implemented".to_string(),
        ))
    }

    async fn get_file_data_stream(
        &self,
        _file_id: u64,
    ) -> Result<Option<Box<dyn futures::Stream<Item = Result<bytes::Bytes>> + Send + Unpin>>> {
        Err(StorageError::NotImplemented(
            "get_file_data_stream not implemented".to_string(),
        ))
    }

    async fn update_encryption_key(
        &self,
        _account_hash: &str,
        _encryption_key: &str,
    ) -> Result<()> {
        Err(StorageError::NotImplemented(
            "update_encryption_key not implemented".to_string(),
        ))
    }

    async fn delete_encryption_key(&self, _account_hash: &str) -> Result<()> {
        Err(StorageError::NotImplemented(
            "delete_encryption_key not implemented".to_string(),
        ))
    }
}

// Helper methods for version management
impl MySqlStorage {
    /// Convert MySQL row to SyncFile
    fn row_to_sync_file(&self, _row: ()) -> Result<crate::models::file::SyncFile> {
        unreachable!("replaced by sqlx");
    }

    /// Convert MySQL row to Device
    fn row_to_device(&self, _row: ()) -> Result<Device> {
        unreachable!("replaced by sqlx");
    }

    // Stub implementations for missing Storage trait methods
    async fn health_check(&self) -> Result<bool> {
        Ok(true)
    }

    async fn get_metrics(&self) -> Result<StorageMetrics> {
        Ok(StorageMetrics {
            total_queries: 0,
            successful_queries: 0,
            failed_queries: 0,
            average_query_time_ms: 0.0,
            active_connections: 0,
            idle_connections: 0,
            cache_hits: 0,
            cache_misses: 0,
        })
    }

    async fn close(&self) -> Result<()> {
        Ok(())
    }

    async fn batch_create_accounts(&self, _accounts: &[Account]) -> Result<Vec<bool>> {
        Err(StorageError::NotImplemented(
            "batch_create_accounts not implemented".to_string(),
        ))
    }

    async fn list_accounts(
        &self,
        _limit: Option<u32>,
        _offset: Option<u64>,
    ) -> Result<Vec<Account>> {
        Err(StorageError::NotImplemented(
            "list_accounts not implemented".to_string(),
        ))
    }

    async fn cleanup_expired_tokens(&self) -> Result<u64> {
        Ok(0)
    }

    async fn batch_store_files(&self, _files: Vec<FileInfo>) -> Result<Vec<(u64, bool)>> {
        Err(StorageError::NotImplemented(
            "batch_store_files not implemented".to_string(),
        ))
    }

    async fn batch_delete_files(
        &self,
        _account_hash: &str,
        _file_ids: Vec<u64>,
    ) -> Result<Vec<bool>> {
        Err(StorageError::NotImplemented(
            "batch_delete_files not implemented".to_string(),
        ))
    }

    async fn store_file_data_stream(
        &self,
        _file_id: u64,
        _data_stream: Box<dyn futures::Stream<Item = Result<bytes::Bytes>> + Send + Unpin>,
    ) -> Result<()> {
        Err(StorageError::NotImplemented(
            "store_file_data_stream not implemented".to_string(),
        ))
    }

    async fn get_file_data_stream(
        &self,
        _file_id: u64,
    ) -> Result<Option<Box<dyn futures::Stream<Item = Result<bytes::Bytes>> + Send + Unpin>>> {
        Err(StorageError::NotImplemented(
            "get_file_data_stream not implemented".to_string(),
        ))
    }

    async fn update_encryption_key(
        &self,
        _account_hash: &str,
        _encryption_key: &str,
    ) -> Result<()> {
        Err(StorageError::NotImplemented(
            "update_encryption_key not implemented".to_string(),
        ))
    }

    async fn delete_encryption_key(&self, _account_hash: &str) -> Result<()> {
        Err(StorageError::NotImplemented(
            "delete_encryption_key not implemented".to_string(),
        ))
    }
}
