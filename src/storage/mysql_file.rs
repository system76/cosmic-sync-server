use chrono::prelude::*;
// removed mysql_async imports after sqlx migration
use tracing::{debug, error, info, warn};
// removed unused std::path::Path
// use prost_types::Timestamp via fully qualified paths where needed

use crate::models::file::FileInfo;
use crate::storage::mysql::MySqlStorage;
use crate::storage::{Result, StorageError};

/// MySQL 파일 관련 기능 확장 트레이트
pub trait MySqlFileExt {
    /// 파일 정보 저장
    async fn store_file_info(&self, file_info: FileInfo) -> Result<u64>;

    /// 파일 정보 조회
    async fn get_file_info(&self, file_id: u64) -> Result<Option<FileInfo>>;

    /// 파일 정보 조회 (삭제된 파일 포함)
    async fn get_file_info_include_deleted(&self, file_id: u64)
        -> Result<Option<(FileInfo, bool)>>;

    /// 경로로 파일 정보 조회
    async fn get_file_info_by_path(
        &self,
        account_hash: &str,
        file_path: &str,
        group_id: i32,
    ) -> Result<Option<FileInfo>>;

    /// 해시로 파일 검색
    async fn get_file_by_hash(
        &self,
        account_hash: &str,
        file_hash: &str,
    ) -> Result<Option<FileInfo>>;

    /// 경로와 파일명으로 파일 검색
    async fn find_file_by_path_and_name(
        &self,
        account_hash: &str,
        file_path: &str,
        filename: &str,
        revision: i64,
    ) -> Result<Option<FileInfo>>;

    /// 경로와 파일명과 그룹 ID로 파일 검색
    async fn find_file_by_criteria(
        &self,
        account_hash: &str,
        group_id: i32,
        watcher_id: i32,
        file_path: &str,
        filename: &str,
    ) -> Result<Option<FileInfo>>;

    /// 파일 삭제
    async fn delete_file(&self, account_hash: &str, file_id: u64) -> Result<()>;

    /// 파일 목록 조회
    async fn list_files(
        &self,
        account_hash: &str,
        group_id: i32,
        upload_time_from: Option<i64>,
    ) -> Result<Vec<FileInfo>>;

    /// 파일 목록 조회 (특정 디바이스 해시 제외)
    async fn list_files_except_device(
        &self,
        account_hash: &str,
        group_id: i32,
        exclude_device_hash: &str,
        upload_time_from: Option<i64>,
    ) -> Result<Vec<FileInfo>>;

    /// 파일 데이터 저장
    async fn store_file_data(&self, file_id: u64, data_bytes: Vec<u8>) -> Result<()>;

    /// 파일 데이터 조회
    async fn get_file_data(&self, file_id: u64) -> Result<Option<Vec<u8>>>;

    /// 암호화 키 조회
    async fn get_encryption_key(&self, account_hash: &str) -> Result<Option<String>>;

    /// 암호화 키 저장
    async fn store_encryption_key(&self, account_hash: &str, encryption_key: &str) -> Result<()>;

    /// 파일 ID로 존재 여부와 삭제 상태 확인
    async fn check_file_exists(&self, file_id: u64) -> Result<(bool, bool)>;

    /// TTL 기반 물리 삭제: is_deleted=1 이고 updated_time이 NOW()-ttl 초 이전인 레코드 제거
    async fn purge_deleted_files_older_than(&self, ttl_secs: i64) -> Result<u64>;

    /// 파일 경로 기준 리비전 상한: 각 (account_hash,file_path,server_group_id)별 최신 max_revisions만 유지하고 나머지는 is_deleted=1 처리
    async fn trim_old_revisions(&self, max_revisions: i32) -> Result<u64>;
}

impl MySqlFileExt for MySqlStorage {
    /// 파일 정보 저장
    async fn store_file_info(&self, file_info: FileInfo) -> Result<u64> {
        let now = Utc::now().timestamp();
        let updated_time = file_info.updated_time.seconds;

        // 트랜잭션 시작 (sqlx)
        let mut tx = self
            .get_sqlx_pool()
            .begin()
            .await
            .map_err(|e| StorageError::Database(format!("트랜잭션 시작 실패(sqlx): {}", e)))?;
        // 먼저 file_id로 기존 파일이 있는지 확인
        let existing_by_file_id: Option<(u64, i64)> = sqlx::query_as(
            r#"SELECT file_id, revision FROM files WHERE file_id = ? AND is_deleted = FALSE LIMIT 1"#
        )
        .bind(file_info.file_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| { error!("❌ file_id로 파일 확인 실패(sqlx): {}", e); StorageError::Database(format!("file_id로 파일 확인 실패: {}", e)) })?;

        if let Some((_existing_file_id, _current_revision)) = existing_by_file_id {
            // 동일한 file_id가 이미 존재하는 경우 - 파일 정보만 업데이트

            // 기존 파일 정보 업데이트
            if let Some(ref kid) = file_info.key_id {
                debug!(
                    "🔐 Updating key_id for existing file_id: {} -> {}",
                    file_info.file_id, kid
                );
            } else {
                debug!(
                    "🔐 Skipping key_id update (None) for existing file_id: {}",
                    file_info.file_id
                );
            }
            sqlx::query(
                r#"UPDATE files SET 
                    file_hash = ?, device_hash = ?, updated_time = FROM_UNIXTIME(?), size = ?,
                    revision = revision + 1,
                    key_id = COALESCE(?, key_id)
                  WHERE file_id = ?"#,
            )
            .bind(&file_info.file_hash)
            .bind(&file_info.device_hash)
            .bind(updated_time)
            .bind(file_info.size as i64)
            .bind(file_info.key_id.as_deref())
            .bind(file_info.file_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                error!("❌ 파일 정보 업데이트 실패(sqlx): {}", e);
                StorageError::Database(format!("파일 정보 업데이트 실패: {}", e))
            })?;

            // 트랜잭션 커밋
            tx.commit().await.map_err(|e| {
                error!("❌ 트랜잭션 커밋 실패(sqlx): {}", e);
                StorageError::Database(format!("트랜잭션 커밋 실패: {}", e))
            })?;

            return Ok(file_info.file_id);
        }

        debug!("🔍 최대 revision 조회 중...");
        // 같은 경로와 파일명을 가진 모든 파일(삭제된 파일 포함)의 최대 revision 조회
        let max_revision: Option<i64> = sqlx::query_scalar(
            r#"SELECT COALESCE(MAX(revision), 0) FROM files WHERE account_hash = ? AND file_path = ? AND filename = ? AND server_group_id = ?"#
        )
        .bind(&file_info.account_hash)
        .bind(&file_info.file_path)
        .bind(&file_info.filename)
        .bind(file_info.group_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| { error!("❌ 최대 revision 조회 실패(sqlx): {}", e); StorageError::Database(format!("최대 revision 조회 실패: {}", e)) })?;

        let new_revision = max_revision.unwrap_or(0) + 1;
        debug!(
            "📄 새 파일 정보 저장 준비: file_id={}, revision={} (key_id: {:?})",
            file_info.file_id, new_revision, file_info.key_id
        );

        // Path encryption and index computation omitted as it's handled elsewhere

        debug!("🔍 활성 파일 확인 중...");
        // 동일한 파일 경로와 이름으로 삭제되지 않은 파일이 있는지 확인
        let existing_active_file: Option<u64> = sqlx::query_scalar(
            r#"SELECT file_id FROM files WHERE account_hash = ? AND file_path = ? AND filename = ? AND server_group_id = ? AND is_deleted = FALSE LIMIT 1"#
        )
        .bind(&file_info.account_hash)
        .bind(&file_info.file_path)
        .bind(&file_info.filename)
        .bind(file_info.group_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| { error!("❌ 활성 파일 존재 여부 확인 실패(sqlx): {}", e); StorageError::Database(format!("활성 파일 존재 여부 확인 실패: {}", e)) })?;

        if let Some(existing_file_id) = existing_active_file {
            // 기존 활성 파일이 있으면 삭제 상태로 표시
            debug!(
                "🗑️ 기존 활성 파일 삭제 표시: existing_file_id={}",
                existing_file_id
            );

            sqlx::query(r#"UPDATE files SET is_deleted = TRUE WHERE file_id = ?"#)
                .bind(existing_file_id)
                .execute(&mut *tx)
                .await
                .map_err(|e| {
                    error!("❌ 기존 파일 삭제 표시 실패(sqlx): {}", e);
                    StorageError::Database(format!("기존 파일 삭제 표시 실패: {}", e))
                })?;
        }

        // 새 파일 삽입 (계산된 revision 사용)
        info!(
            "💾 새 파일 INSERT 경로: file_id={}, revision={}, filename= {}",
            file_info.file_id, new_revision, file_info.filename
        );

        // Determine server key; if present and 32 bytes, encrypt path/name and compute indices deterministically
        let cfg = crate::server::app_state::AppState::get_config();
        let (file_path_bytes, filename_bytes, eq_index, token_path) = if let Some(kv) =
            cfg.server_encode_key.as_ref()
        {
            if kv.len() == 32 {
                let key: &[u8; 32] = kv.as_slice().try_into().expect("len checked");
                let aad = format!(
                    "{}:{}:{}",
                    file_info.account_hash, file_info.group_id, file_info.watcher_id
                );
                let ct_path = crate::utils::crypto::aead_encrypt(
                    key,
                    file_info.file_path.as_bytes(),
                    aad.as_bytes(),
                );
                let ct_name = crate::utils::crypto::aead_encrypt(
                    key,
                    file_info.filename.as_bytes(),
                    aad.as_bytes(),
                );
                let salt =
                    crate::utils::crypto::derive_salt(key, "meta-index", &file_info.account_hash);
                let eq_index = crate::utils::crypto::make_eq_index(&salt, &file_info.file_path);
                let token_path = crate::utils::crypto::make_token_path(&salt, &file_info.file_path);
                (ct_path, ct_name, eq_index, token_path)
            } else {
                let fpb = file_info.file_path.as_bytes().to_vec();
                let fnb = file_info.filename.as_bytes().to_vec();
                let eq = crate::utils::crypto::make_eq_index(
                    file_info.account_hash.as_bytes(),
                    &file_info.file_path,
                );
                let tp = crate::utils::crypto::make_token_path(
                    file_info.account_hash.as_bytes(),
                    &file_info.file_path,
                );
                (fpb, fnb, eq, tp)
            }
        } else {
            let fpb = file_info.file_path.as_bytes().to_vec();
            let fnb = file_info.filename.as_bytes().to_vec();
            let eq = crate::utils::crypto::make_eq_index(
                file_info.account_hash.as_bytes(),
                &file_info.file_path,
            );
            let tp = crate::utils::crypto::make_token_path(
                file_info.account_hash.as_bytes(),
                &file_info.file_path,
            );
            (fpb, fnb, eq, tp)
        };

        sqlx::query(
            r#"INSERT INTO files (
                file_id, account_hash, device_hash, file_path, filename, file_hash, size,
                is_deleted, revision, created_time, updated_time, group_id, watcher_id,
                server_group_id, server_watcher_id, eq_index, token_path, key_id
            ) VALUES (?, ?, ?, ?, ?, ?, ?, FALSE, ?, FROM_UNIXTIME(?), FROM_UNIXTIME(?), ?, ?, ?, ?, ?, ?, ?)"#
        )
        .bind(file_info.file_id as i64)
        .bind(&file_info.account_hash)
        .bind(&file_info.device_hash)
        .bind(&file_path_bytes)
        .bind(&filename_bytes)
        .bind(&file_info.file_hash)
        .bind(file_info.size as i64)
        .bind(new_revision)
        .bind(now)
        .bind(updated_time)
        .bind(file_info.group_id)
        .bind(file_info.watcher_id)
        .bind(file_info.group_id)
        .bind(file_info.watcher_id)
        .bind(&eq_index)
        .bind(&token_path)
        .bind(file_info.key_id.as_deref())
        .execute(&mut *tx)
        .await
        .map_err(|e| { error!("❌ 새 파일 정보 삽입 실패(sqlx): {}", e); StorageError::Database(format!("새 파일 정보 삽입 실패: {}", e)) })?;

        debug!("🔄 트랜잭션 커밋 중...");
        // 트랜잭션 커밋
        tx.commit().await.map_err(|e| {
            error!("❌ 트랜잭션 커밋 실패(sqlx): {}", e);
            StorageError::Database(format!("트랜잭션 커밋 실패: {}", e))
        })?;

        Ok(file_info.file_id)
    }

    /// 파일 정보 조회
    async fn get_file_info(&self, file_id: u64) -> Result<Option<FileInfo>> {
        use sqlx::Row;
        debug!("파일 정보 조회: file_id={}", file_id);

        let row_opt = sqlx::query(
            r#"SELECT 
                    file_id, account_hash, device_hash, file_path, filename, file_hash,
                    UNIX_TIMESTAMP(created_time) AS created_ts,
                    UNIX_TIMESTAMP(updated_time) AS updated_ts,
                    group_id, watcher_id, is_deleted, revision, size, key_id
               FROM files 
               WHERE file_id = ? AND is_deleted = FALSE"#,
        )
        .bind(file_id)
        .fetch_optional(self.get_sqlx_pool())
        .await
        .map_err(|e| StorageError::Database(format!("파일 정보 조회 실패(sqlx): {}", e)))?;

        if let Some(row) = row_opt {
            let file_id: u64 = row.try_get("file_id").unwrap_or(0);
            let account_hash: String = row.try_get("account_hash").unwrap_or_default();
            let device_hash: String = row.try_get("device_hash").unwrap_or_default();
            let file_path_b: Vec<u8> = row.try_get("file_path").unwrap_or_default();
            let filename_b: Vec<u8> = row.try_get("filename").unwrap_or_default();
            let file_hash: String = row.try_get("file_hash").unwrap_or_default();
            let updated_ts: Option<i64> = row.try_get("updated_ts").unwrap_or(None);
            let group_id: i32 = row.try_get("group_id").unwrap_or(0);
            let watcher_id: i32 = row.try_get("watcher_id").unwrap_or(0);
            let revision: i64 = row.try_get("revision").unwrap_or(0);
            let size: u64 = row.try_get("size").unwrap_or(0);
            let key_id_opt: Option<String> = row.try_get("key_id").ok();

            let timestamp = prost_types::Timestamp {
                seconds: updated_ts.unwrap_or(0),
                nanos: 0,
            };

            let file_info = FileInfo {
                file_id,
                filename: self.decrypt_text(&account_hash, group_id, watcher_id, filename_b),
                file_hash,
                device_hash,
                group_id,
                watcher_id,
                is_encrypted: false,
                file_path: self.decrypt_text(&account_hash, group_id, watcher_id, file_path_b),
                updated_time: timestamp,
                revision,
                account_hash,
                size,
                key_id: key_id_opt,
            };
            debug!("파일 정보 조회 성공: file_id={}", file_id);
            Ok(Some(file_info))
        } else {
            debug!("파일 정보 없음 또는 이미 삭제됨: file_id={}", file_id);
            Ok(None)
        }
    }

    /// 파일 정보 조회 (삭제된 파일 포함)
    async fn get_file_info_include_deleted(
        &self,
        file_id: u64,
    ) -> Result<Option<(FileInfo, bool)>> {
        use sqlx::Row;
        debug!("파일 정보 조회 (삭제 포함): file_id={}", file_id);

        let row_opt = sqlx::query(
            r#"SELECT 
                    file_id, account_hash, device_hash, file_path, filename, file_hash,
                    UNIX_TIMESTAMP(created_time) AS created_ts,
                    UNIX_TIMESTAMP(updated_time) AS updated_ts,
                    group_id, watcher_id, is_deleted, revision, size, key_id
               FROM files 
               WHERE file_id = ?"#,
        )
        .bind(file_id)
        .fetch_optional(self.get_sqlx_pool())
        .await
        .map_err(|e| StorageError::Database(format!("파일 정보 조회 실패(sqlx): {}", e)))?;

        if let Some(row) = row_opt {
            let file_id: u64 = row.try_get("file_id").unwrap_or(0);
            let account_hash: String = row.try_get("account_hash").unwrap_or_default();
            let device_hash: String = row.try_get("device_hash").unwrap_or_default();
            let file_path_b: Vec<u8> = row.try_get("file_path").unwrap_or_default();
            let filename_b: Vec<u8> = row.try_get("filename").unwrap_or_default();
            let file_hash: String = row.try_get("file_hash").unwrap_or_default();
            let updated_ts: Option<i64> = row.try_get("updated_ts").unwrap_or(None);
            let group_id: i32 = row.try_get("group_id").unwrap_or(0);
            let watcher_id: i32 = row.try_get("watcher_id").unwrap_or(0);
            let is_deleted: bool = row.try_get("is_deleted").unwrap_or(false);
            let revision: i64 = row.try_get("revision").unwrap_or(0);
            let size: u64 = row.try_get("size").unwrap_or(0);
            let key_id_opt: Option<String> = row.try_get("key_id").ok();

            let timestamp = prost_types::Timestamp {
                seconds: updated_ts.unwrap_or(0),
                nanos: 0,
            };

            let file_info = FileInfo {
                file_id,
                filename: self.decrypt_text(&account_hash, group_id, watcher_id, filename_b),
                file_hash,
                device_hash,
                group_id,
                watcher_id,
                is_encrypted: false,
                file_path: self.decrypt_text(&account_hash, group_id, watcher_id, file_path_b),
                updated_time: timestamp,
                revision,
                account_hash,
                size,
                key_id: key_id_opt,
            };
            debug!(
                "파일 정보 조회 성공 (삭제 포함): file_id={}, is_deleted={}",
                file_id, is_deleted
            );
            Ok(Some((file_info, is_deleted)))
        } else {
            debug!("파일 정보 없음 (ID 자체가 없음): file_id={}", file_id);
            Ok(None)
        }
    }

    /// 경로로 파일 정보 조회
    async fn get_file_info_by_path(
        &self,
        account_hash: &str,
        file_path: &str,
        group_id: i32,
    ) -> Result<Option<FileInfo>> {
        use sqlx::Row;
        debug!(
            "경로로 파일 정보 조회: account_hash={}, file_path={}, group_id={}",
            account_hash, file_path, group_id
        );

        // lookup by eq_index (deterministic)
        let cfg = crate::server::app_state::AppState::get_config();
        let eq_index = if let Some(kv) = cfg.server_encode_key.as_ref() {
            if kv.len() == 32 {
                let key: &[u8; 32] = kv.as_slice().try_into().expect("len checked");
                let salt = crate::utils::crypto::derive_salt(key, "meta-index", account_hash);
                crate::utils::crypto::make_eq_index(&salt, file_path)
            } else {
                crate::utils::crypto::make_eq_index(account_hash.as_bytes(), file_path)
            }
        } else {
            crate::utils::crypto::make_eq_index(account_hash.as_bytes(), file_path)
        };

        let row_opt = sqlx::query(
            r#"SELECT 
                    file_id, account_hash, device_hash, file_path, filename, file_hash,
                    UNIX_TIMESTAMP(created_time) AS created_ts,
                    UNIX_TIMESTAMP(updated_time) AS updated_ts,
                    group_id, watcher_id, revision, size, key_id
               FROM files 
               WHERE account_hash = ? AND eq_index = ? AND server_group_id = ? AND is_deleted = FALSE
               ORDER BY revision DESC LIMIT 1"#
        )
        .bind(account_hash)
        .bind(&eq_index)
        .bind(group_id)
        .fetch_optional(self.get_sqlx_pool())
        .await
        .map_err(|e| StorageError::Database(format!("파일 정보 조회 실패(sqlx): {}", e)))?;

        if let Some(row) = row_opt {
            let file_id: u64 = row.try_get("file_id").unwrap_or(0);
            let account_hash: String = row.try_get("account_hash").unwrap_or_default();
            let device_hash: String = row.try_get("device_hash").unwrap_or_default();
            let file_path_b: Vec<u8> = row.try_get("file_path").unwrap_or_default();
            let filename_b: Vec<u8> = row.try_get("filename").unwrap_or_default();
            let file_hash: String = row.try_get("file_hash").unwrap_or_default();
            let updated_ts: Option<i64> = row.try_get("updated_ts").unwrap_or(None);
            let group_id: i32 = row.try_get("group_id").unwrap_or(0);
            let watcher_id: i32 = row.try_get("watcher_id").unwrap_or(0);
            let revision: i64 = row.try_get("revision").unwrap_or(0);
            let size: u64 = row.try_get("size").unwrap_or(0);
            let key_id_opt: Option<String> = row.try_get("key_id").ok();

            let timestamp = prost_types::Timestamp {
                seconds: updated_ts.unwrap_or(0),
                nanos: 0,
            };

            let file_info = FileInfo {
                file_id,
                filename: self.decrypt_text(&account_hash, group_id, watcher_id, filename_b),
                file_hash,
                device_hash,
                group_id,
                watcher_id,
                is_encrypted: false,
                file_path: self.decrypt_text(&account_hash, group_id, watcher_id, file_path_b),
                updated_time: timestamp,
                revision,
                account_hash,
                size,
                key_id: key_id_opt,
            };
            debug!("경로로 파일 정보 조회 성공: file_id={}", file_id);
            Ok(Some(file_info))
        } else {
            debug!("경로에 해당하는 파일 정보 없음: file_path={}", file_path);
            Ok(None)
        }
    }

    /// 해시로 파일 검색
    async fn get_file_by_hash(
        &self,
        account_hash: &str,
        file_hash: &str,
    ) -> Result<Option<FileInfo>> {
        use sqlx::Row;
        debug!(
            "해시로 파일 검색: account_hash={}, file_hash={}",
            account_hash, file_hash
        );

        let row_opt = sqlx::query(
            r#"SELECT 
                    file_id, account_hash, device_hash, file_path, filename, file_hash,
                    UNIX_TIMESTAMP(created_time) AS created_ts,
                    UNIX_TIMESTAMP(updated_time) AS updated_ts,
                    group_id, watcher_id, revision, size, key_id
               FROM files 
               WHERE account_hash = ? AND file_hash = ? AND is_deleted = FALSE
               ORDER BY revision DESC LIMIT 1"#,
        )
        .bind(account_hash)
        .bind(file_hash)
        .fetch_optional(self.get_sqlx_pool())
        .await
        .map_err(|e| StorageError::Database(format!("파일 검색 실패(sqlx): {}", e)))?;

        if let Some(row) = row_opt {
            let file_id: u64 = row.try_get("file_id").unwrap_or(0);
            let account_hash: String = row.try_get("account_hash").unwrap_or_default();
            let device_hash: String = row.try_get("device_hash").unwrap_or_default();
            let file_path_b: Vec<u8> = row.try_get("file_path").unwrap_or_default();
            let filename_b: Vec<u8> = row.try_get("filename").unwrap_or_default();
            let file_hash: String = row.try_get("file_hash").unwrap_or_default();
            let updated_ts: Option<i64> = row.try_get("updated_ts").unwrap_or(None);
            let group_id: i32 = row.try_get("group_id").unwrap_or(0);
            let watcher_id: i32 = row.try_get("watcher_id").unwrap_or(0);
            let revision: i64 = row.try_get("revision").unwrap_or(0);
            let size: u64 = row.try_get("size").unwrap_or(0);
            let key_id_opt: Option<String> = row.try_get("key_id").ok();

            let timestamp = prost_types::Timestamp {
                seconds: updated_ts.unwrap_or(0),
                nanos: 0,
            };

            let file_info = FileInfo {
                file_id,
                filename: self.decrypt_text(&account_hash, group_id, watcher_id, filename_b),
                file_hash,
                device_hash,
                group_id,
                watcher_id,
                is_encrypted: false,
                file_path: self.decrypt_text(&account_hash, group_id, watcher_id, file_path_b),
                updated_time: timestamp,
                revision,
                account_hash,
                size,
                key_id: key_id_opt,
            };
            debug!("해시로 파일 검색 성공: file_id={}", file_id);
            Ok(Some(file_info))
        } else {
            debug!("해시에 해당하는 파일 정보 없음: file_hash={}", file_hash);
            Ok(None)
        }
    }

    /// 경로와 파일명으로 파일 검색
    async fn find_file_by_path_and_name(
        &self,
        account_hash: &str,
        file_path: &str,
        filename: &str,
        revision: i64,
    ) -> Result<Option<FileInfo>> {
        use sqlx::Row;
        debug!(
            "경로와 파일명으로 파일 검색: account_hash={}, file_path={}, filename={}, revision={}",
            account_hash, file_path, filename, revision
        );

        // equality by eq_index
        let cfg = crate::server::app_state::AppState::get_config();
        let eq_index = if let Some(kv) = cfg.server_encode_key.as_ref() {
            if kv.len() == 32 {
                let key: &[u8; 32] = kv.as_slice().try_into().expect("len checked");
                let salt = crate::utils::crypto::derive_salt(key, "meta-index", account_hash);
                crate::utils::crypto::make_eq_index(&salt, file_path)
            } else {
                crate::utils::crypto::make_eq_index(account_hash.as_bytes(), file_path)
            }
        } else {
            crate::utils::crypto::make_eq_index(account_hash.as_bytes(), file_path)
        };
        let row_opt = if revision > 0 {
            sqlx::query(
                r#"SELECT 
                        file_id, account_hash, device_hash, file_path, filename, file_hash,
                        UNIX_TIMESTAMP(created_time) as created_ts,
                        UNIX_TIMESTAMP(updated_time) as updated_ts,
                        group_id, watcher_id, revision, size, key_id
                   FROM files 
                   WHERE account_hash = ? AND eq_index = ? AND is_deleted = FALSE AND revision = ?
                   ORDER BY revision DESC LIMIT 1"#,
            )
            .bind(account_hash)
            .bind(&eq_index)
            .bind(revision)
            .fetch_optional(self.get_sqlx_pool())
            .await
            .map_err(|e| {
                StorageError::Database(format!("파일 검색 실패(정확한 검색, sqlx): {}", e))
            })?
        } else {
            sqlx::query(
                r#"SELECT 
                        file_id, account_hash, device_hash, file_path, filename, file_hash,
                        UNIX_TIMESTAMP(created_time) as created_ts,
                        UNIX_TIMESTAMP(updated_time) as updated_ts,
                        group_id, watcher_id, revision, size, key_id
                   FROM files 
                   WHERE account_hash = ? AND eq_index = ? AND is_deleted = FALSE
                   ORDER BY revision DESC LIMIT 1"#,
            )
            .bind(account_hash)
            .bind(&eq_index)
            .fetch_optional(self.get_sqlx_pool())
            .await
            .map_err(|e| {
                StorageError::Database(format!("파일 검색 실패(정확한 검색, sqlx): {}", e))
            })?
        };

        if let Some(row) = row_opt {
            let file_id: u64 = row.try_get("file_id").unwrap_or(0);
            let acc_hash: String = row.try_get("account_hash").unwrap_or_default();
            let device_hash: String = row.try_get("device_hash").unwrap_or_default();
            let file_path_b: Vec<u8> = row.try_get("file_path").unwrap_or_default();
            let filename_b: Vec<u8> = row.try_get("filename").unwrap_or_default();
            let file_hash: String = row.try_get("file_hash").unwrap_or_default();
            let updated_ts: Option<i64> = row.try_get("updated_ts").unwrap_or(None);
            let group_id: i32 = row.try_get("group_id").unwrap_or(0);
            let watcher_id: i32 = row.try_get("watcher_id").unwrap_or(0);
            let revision: i64 = row.try_get("revision").unwrap_or(0);
            let size: u64 = row.try_get("size").unwrap_or(0);
            let key_id_opt: Option<String> = row.try_get("key_id").ok();

            let file_path = self.decrypt_text(&acc_hash, group_id, watcher_id, file_path_b);
            let filename = self.decrypt_text(&acc_hash, group_id, watcher_id, filename_b);

            let timestamp = prost_types::Timestamp {
                seconds: updated_ts.unwrap_or(0),
                nanos: 0,
            };

            let file_info = FileInfo {
                file_id,
                filename,
                file_hash,
                device_hash,
                group_id,
                watcher_id,
                is_encrypted: false,
                file_path,
                updated_time: timestamp,
                revision,
                account_hash: acc_hash,
                size,
                key_id: key_id_opt,
            };
            debug!("경로/파일명으로 파일 검색 성공: file_id={}", file_id);
            Ok(Some(file_info))
        } else {
            debug!("경로/파일명 검색 결과 없음");
            Ok(None)
        }
    }

    /// 파일 삭제 (metadata and content)
    async fn delete_file(&self, account_hash: &str, file_id: u64) -> Result<()> {
        info!(
            "파일 삭제: account_hash={}, file_id={}",
            account_hash, file_id
        );
        let mut tx = self
            .get_sqlx_pool()
            .begin()
            .await
            .map_err(|e| StorageError::Database(format!("트랜잭션 시작 실패(sqlx): {}", e)))?;

        // 파일이 존재하고 사용자에게 속하는지 확인
        let file_exists: Option<(u64, i64, String, String, String, i32, i32)> = sqlx::query_as(
            r#"SELECT file_id, revision, file_path, filename, device_hash, group_id, watcher_id
               FROM files WHERE file_id = ? AND account_hash = ?"#,
        )
        .bind(file_id)
        .bind(account_hash)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| StorageError::Database(format!("파일 존재 여부 확인 실패(sqlx): {}", e)))?;

        if file_exists.is_none() {
            debug!(
                "삭제할 파일이 없거나 해당 사용자의 파일이 아님: file_id={}, account_hash={}",
                file_id, account_hash
            );
            return Err(StorageError::NotFound(format!(
                "파일을 찾을 수 없음: {}",
                file_id
            )));
        }

        let (_, current_revision, file_path, filename, device_hash, group_id, watcher_id) =
            if let Some(rec) = file_exists {
                rec
            } else {
                return Err(StorageError::NotFound(format!(
                    "파일을 찾을 수 없음: {}",
                    file_id
                )));
            };
        let new_revision = current_revision + 1;

        debug!("파일 삭제 처리: file_id={}, file_path={}, filename={}, current_revision={}, new_revision={}", 
               file_id, file_path, filename, current_revision, new_revision);

        let now = Utc::now().timestamp();

        // 1. 기존 파일 레코드를 is_deleted=1로 업데이트
        sqlx::query(r#"UPDATE files SET is_deleted = 1 WHERE file_id = ?"#)
            .bind(file_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                StorageError::Database(format!("기존 파일 삭제 표시 실패(sqlx): {}", e))
            })?;

        // 2. 같은 파일 경로와 이름을 가진 이전 revision들도 모두 is_deleted=1로 업데이트
        sqlx::query(
            r#"UPDATE files SET is_deleted = 1 
             WHERE account_hash = ? AND file_path = ? AND filename = ? AND group_id = ?"#,
        )
        .bind(account_hash)
        .bind(&file_path)
        .bind(&filename)
        .bind(group_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            StorageError::Database(format!("이전 버전 파일 삭제 표시 실패(sqlx): {}", e))
        })?;

        // 3. 삭제 이력 추가
        debug!(
            "삭제 이력 추가: file_path={}, filename={}",
            file_path, filename
        );

        // 새로운 file_id 생성 (랜덤값)
        let new_file_id = rand::random::<u64>();

        // file_id 필드를 명시적으로 지정하여 INSERT
        sqlx::query(
            r#"INSERT INTO files 
            (file_id, account_hash, device_hash, file_path, filename, file_hash, size) 
            VALUES (?, ?, ?, ?, ?, ?, ?)"#,
        )
        .bind(new_file_id)
        .bind(account_hash)
        .bind(&device_hash)
        .bind(&file_path)
        .bind(&filename)
        .bind(&file_path)
        .bind(0i64)
        .execute(&mut *tx)
        .await
        .map_err(|e| StorageError::Database(format!("삭제 이력 추가 실패 (1단계, sqlx): {}", e)))?;

        // 나머지 필드 업데이트
        sqlx::query(
            r#"UPDATE files SET 
             is_deleted = 1, 
             revision = ?,
             created_time = FROM_UNIXTIME(?),
             updated_time = FROM_UNIXTIME(?),
             group_id = ?,
             watcher_id = ?
             WHERE file_id = ?"#,
        )
        .bind(new_revision)
        .bind(now)
        .bind(now)
        .bind(group_id)
        .bind(watcher_id)
        .bind(new_file_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| StorageError::Database(format!("삭제 이력 추가 실패 (2단계, sqlx): {}", e)))?;

        // 트랜잭션 커밋
        tx.commit()
            .await
            .map_err(|e| StorageError::Database(format!("트랜잭션 커밋 실패(sqlx): {}", e)))?;

        info!(
            "파일 삭제 완료: file_id={}, new_revision={}, 삭제 이력 file_id={}",
            file_id, new_revision, new_file_id
        );
        Ok(())
    }

    /// 파일 목록 조회
    async fn list_files(
        &self,
        account_hash: &str,
        group_id: i32,
        upload_time_from: Option<i64>,
    ) -> Result<Vec<FileInfo>> {
        use sqlx::Row;
        info!(
            "파일 목록 조회: account_hash={}, group_id={}",
            account_hash, group_id
        );

        let rows = sqlx::query(
            r#"SELECT 
                    file_id, account_hash, device_hash, file_path, filename, file_hash,
                    UNIX_TIMESTAMP(created_time) AS created_ts,
                    UNIX_TIMESTAMP(updated_time) AS updated_ts,
                    group_id, watcher_id, revision, size, key_id
               FROM files 
               WHERE account_hash = ? AND server_group_id = ? AND is_deleted = FALSE
               ORDER BY updated_time DESC"#,
        )
        .bind(account_hash)
        .bind(group_id)
        .fetch_all(self.get_sqlx_pool())
        .await
        .map_err(|e| StorageError::Database(format!("파일 목록 조회 실패(sqlx): {}", e)))?;

        let mut files = Vec::with_capacity(rows.len());
        for row in rows.into_iter() {
            let file_id: u64 = row.try_get("file_id").unwrap_or(0);
            let acc_hash: String = row.try_get("account_hash").unwrap_or_default();
            let device_hash: String = row.try_get("device_hash").unwrap_or_default();
            let file_path_b: Vec<u8> = row.try_get("file_path").unwrap_or_default();
            let filename_b: Vec<u8> = row.try_get("filename").unwrap_or_default();
            let file_hash: String = row.try_get("file_hash").unwrap_or_default();
            let updated_ts: Option<i64> = row.try_get("updated_ts").unwrap_or(None);
            let group_id: i32 = row.try_get("group_id").unwrap_or(0);
            let watcher_id: i32 = row.try_get("watcher_id").unwrap_or(0);
            let revision: i64 = row.try_get("revision").unwrap_or(0);
            let size: u64 = row.try_get("size").unwrap_or(0);
            let key_id_opt: Option<String> = row.try_get("key_id").ok();

            let timestamp = prost_types::Timestamp {
                seconds: updated_ts.unwrap_or(0),
                nanos: 0,
            };
            files.push(FileInfo {
                file_id,
                filename: self.decrypt_text(&acc_hash, group_id, watcher_id, filename_b),
                file_hash,
                device_hash,
                group_id,
                watcher_id,
                is_encrypted: false,
                file_path: self.decrypt_text(&acc_hash, group_id, watcher_id, file_path_b),
                updated_time: timestamp,
                revision,
                account_hash: acc_hash,
                size,
                key_id: key_id_opt,
            });
        }
        info!(
            "파일 {} 개를 찾았습니다: account_hash={}, group_id={}",
            files.len(),
            account_hash,
            group_id
        );
        Ok(files)
    }

    /// 파일 목록 조회 (특정 디바이스 해시 제외)
    async fn list_files_except_device(
        &self,
        account_hash: &str,
        group_id: i32,
        exclude_device_hash: &str,
        upload_time_from: Option<i64>,
    ) -> Result<Vec<FileInfo>> {
        use sqlx::Row;
        info!(
            "파일 목록 조회(디바이스 제외): account_hash={}, group_id={}, exclude_device={}",
            account_hash, group_id, exclude_device_hash
        );

        let rows = sqlx::query(
            r#"SELECT 
                    file_id, account_hash, device_hash, file_path, filename, file_hash,
                    UNIX_TIMESTAMP(created_time) AS created_ts,
                    UNIX_TIMESTAMP(updated_time) AS updated_ts,
                    group_id, watcher_id, revision, size, key_id
               FROM files 
               WHERE account_hash = ? AND server_group_id = ? AND device_hash <> ? AND is_deleted = FALSE
               ORDER BY updated_time DESC"#
        )
        .bind(account_hash)
        .bind(group_id)
        .bind(exclude_device_hash)
        .fetch_all(self.get_sqlx_pool())
        .await
        .map_err(|e| StorageError::Database(format!("파일 목록 조회 실패(sqlx): {}", e)))?;

        let mut files = Vec::with_capacity(rows.len());
        for row in rows.into_iter() {
            let file_id: u64 = row.try_get("file_id").unwrap_or(0);
            let acc_hash: String = row.try_get("account_hash").unwrap_or_default();
            let device_hash: String = row.try_get("device_hash").unwrap_or_default();
            let file_path_b: Vec<u8> = row.try_get("file_path").unwrap_or_default();
            let filename_b: Vec<u8> = row.try_get("filename").unwrap_or_default();
            let file_hash: String = row.try_get("file_hash").unwrap_or_default();
            let updated_ts: Option<i64> = row.try_get("updated_ts").unwrap_or(None);
            let group_id: i32 = row.try_get("group_id").unwrap_or(0);
            let watcher_id: i32 = row.try_get("watcher_id").unwrap_or(0);
            let revision: i64 = row.try_get("revision").unwrap_or(0);
            let size: u64 = row.try_get("size").unwrap_or(0);
            let key_id_opt: Option<String> = row.try_get("key_id").ok();

            let timestamp = prost_types::Timestamp {
                seconds: updated_ts.unwrap_or(0),
                nanos: 0,
            };
            files.push(FileInfo {
                file_id,
                filename: self.decrypt_text(&acc_hash, group_id, watcher_id, filename_b),
                file_hash,
                device_hash,
                group_id,
                watcher_id,
                is_encrypted: false,
                file_path: self.decrypt_text(&acc_hash, group_id, watcher_id, file_path_b),
                updated_time: timestamp,
                revision,
                account_hash: acc_hash,
                size,
                key_id: key_id_opt,
            });
        }
        Ok(files)
    }

    /// 파일 데이터 저장
    async fn store_file_data(&self, file_id: u64, data_bytes: Vec<u8>) -> Result<()> {
        info!(
            "🔄 MySQL 파일 데이터 저장 시작: file_id={}, data_size={} bytes",
            file_id,
            data_bytes.len()
        );

        debug!("📡 데이터베이스 연결(sqlx) 준비 중...");

        // 현재 시간 (초)
        let now = chrono::Utc::now().timestamp();
        debug!("⏰ 타임스탬프: {}", now);

        debug!("🔍 기존 파일 데이터 확인 중...");
        // 기존 데이터 있는지 확인
        let exists: Option<u64> =
            sqlx::query_scalar(r#"SELECT file_id FROM file_data WHERE file_id = ?"#)
                .bind(file_id)
                .fetch_optional(self.get_sqlx_pool())
                .await
                .map_err(|e| {
                    error!("❌ 기존 데이터 확인 실패(sqlx): {}", e);
                    StorageError::Database(e.to_string())
                })?;

        if exists.is_some() {
            // 업데이트
            info!("🔄 기존 파일 데이터 업데이트: file_id={}", file_id);
            sqlx::query(r#"UPDATE file_data SET data = ?, updated_at = ? WHERE file_id = ?"#)
                .bind(data_bytes)
                .bind(now)
                .bind(file_id)
                .execute(self.get_sqlx_pool())
                .await
                .map_err(|e| {
                    error!("❌ 파일 데이터 업데이트 실패(sqlx): {}", e);
                    StorageError::Database(e.to_string())
                })?;
        } else {
            // 새로 삽입
            info!("💾 새 파일 데이터 삽입: file_id={}", file_id);
            sqlx::query(r#"INSERT INTO file_data (file_id, data, created_at, updated_at) VALUES (?, ?, ?, ?)"#)
                .bind(file_id)
                .bind(data_bytes)
                .bind(now)
                .bind(now)
                .execute(self.get_sqlx_pool())
                .await
                .map_err(|e| { error!("❌ 파일 데이터 삽입 실패(sqlx): {}", e); StorageError::Database(e.to_string()) })?;
        }

        info!("🎉 파일 데이터 저장 완료: file_id={}", file_id);
        Ok(())
    }

    /// 파일 데이터 조회
    async fn get_file_data(&self, file_id: u64) -> Result<Option<Vec<u8>>> {
        let result: Option<Vec<u8>> =
            sqlx::query_scalar(r#"SELECT data FROM file_data WHERE file_id = ?"#)
                .bind(file_id as i64)
                .fetch_optional(self.get_sqlx_pool())
                .await
                .map_err(|e| StorageError::Database(e.to_string()))?;
        Ok(result)
    }

    /// 암호화 키 조회
    async fn get_encryption_key(&self, account_hash: &str) -> Result<Option<String>> {
        let result: Option<String> = sqlx::query_scalar(
            r#"SELECT encryption_key FROM encryption_keys WHERE account_hash = ?"#,
        )
        .bind(account_hash)
        .fetch_optional(self.get_sqlx_pool())
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;
        Ok(result)
    }

    /// 암호화 키 저장
    async fn store_encryption_key(&self, account_hash: &str, encryption_key: &str) -> Result<()> {
        // 현재 시간 (초)
        let now = chrono::Utc::now().timestamp();
        let exists: Option<String> = sqlx::query_scalar(
            r#"SELECT account_hash FROM encryption_keys WHERE account_hash = ?"#,
        )
        .bind(account_hash)
        .fetch_optional(self.get_sqlx_pool())
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        if exists.is_some() {
            sqlx::query(r#"UPDATE encryption_keys SET encryption_key = ?, updated_at = ? WHERE account_hash = ?"#)
                .bind(encryption_key)
                .bind(now)
                .bind(account_hash)
                .execute(self.get_sqlx_pool())
                .await
                .map_err(|e| StorageError::Database(e.to_string()))?;
        } else {
            sqlx::query(r#"INSERT INTO encryption_keys (account_hash, encryption_key, created_at, updated_at) VALUES (?, ?, ?, ?)"#)
                .bind(account_hash)
                .bind(encryption_key)
                .bind(now)
                .bind(now)
                .execute(self.get_sqlx_pool())
                .await
                .map_err(|e| StorageError::Database(e.to_string()))?;
        }
        Ok(())
    }

    /// 경로와 파일명과 그룹 ID로 파일 검색
    async fn find_file_by_criteria(
        &self,
        account_hash: &str,
        group_id: i32,
        watcher_id: i32,
        file_path: &str,
        filename: &str,
    ) -> Result<Option<FileInfo>> {
        use sqlx::Row;
        debug!("🔍 find_file_by_criteria 호출됨:");
        debug!("   account_hash: {}", account_hash);
        debug!("   group_id: {}", group_id);
        debug!("   watcher_id: {}", watcher_id);
        debug!("   file_path: {}", file_path);
        debug!("   filename: {}", filename);

        // 경로 분석: 이미 파일명이 포함되어 있는지 확인
        let (search_path, search_filename) =
            if file_path.ends_with(&format!("/{}", filename)) || file_path.ends_with(filename) {
                // 파일 경로에 이미 파일명이 포함된 경우
                debug!("파일 경로에 이미 파일명이 포함됨: {}", file_path);

                // 파일명 추출: 마지막 / 이후의 내용 또는 전체 경로
                let last_slash_pos = file_path.rfind('/');

                match last_slash_pos {
                    Some(pos) => {
                        // 마지막 / 이전까지가 경로, 그 이후가 파일명
                        let path = &file_path[0..pos];
                        let fname = &file_path[pos + 1..];
                        debug!("추출된 경로: {}, 파일명: {}", path, fname);
                        (path.to_string(), fname.to_string())
                    }
                    None => {
                        // /가 없는 경우 전체가 파일명
                        debug!("경로 없음, 파일명만 있음: {}", file_path);
                        ("".to_string(), file_path.to_string())
                    }
                }
            } else {
                // 경로와 파일명이 분리되어 있는 경우
                debug!(
                    "경로와 파일명 분리: 경로={}, 파일명={}",
                    file_path, filename
                );
                (file_path.to_string(), filename.to_string())
            };

        debug!(
            "🔍 최종 검색 조건: path='{}', filename='{}'",
            search_path, search_filename
        );

        // sqlx로 두 패턴 검색 수행
        let row = sqlx::query(
            r#"SELECT 
                    file_id, account_hash, device_hash, file_path, filename, file_hash,
                    UNIX_TIMESTAMP(created_time) as created_ts,
                    UNIX_TIMESTAMP(updated_time) as updated_ts,
                    group_id, watcher_id, revision, size, key_id
               FROM files 
               WHERE account_hash = ? AND server_group_id = ? AND server_watcher_id = ? AND is_deleted = FALSE
                 AND (
                   (file_path = ? AND filename = ?) OR 
                   (file_path = ? AND filename = ?)
                 )
               ORDER BY revision DESC 
               LIMIT 1"#
        )
        .bind(account_hash)
        .bind(group_id)
        .bind(watcher_id)
        .bind(&search_path)
        .bind(&search_filename)
        .bind(file_path)
        .bind(filename)
        .fetch_optional(self.get_sqlx_pool())
        .await
        .map_err(|e| { error!("❌ 파일 검색 쿼리 실행 실패(sqlx): {}", e); StorageError::Database(format!("파일 검색 쿼리 실행 실패: {}", e)) })?;

        if let Some(row) = row {
            debug!("✅ 파일 찾음!");
            // Row 객체에서 필요한 필드 추출
            let file_id: u64 = row.try_get("file_id").unwrap_or(0);
            let acc_hash: String = row.try_get("account_hash").unwrap_or_default();
            let device_hash: String = row.try_get("device_hash").unwrap_or_default();
            let file_path: String = row.try_get("file_path").unwrap_or_default();
            let filename: String = row.try_get("filename").unwrap_or_default();
            let file_hash: String = row.try_get("file_hash").unwrap_or_default();
            let updated_ts: Option<i64> = row.try_get("updated_ts").unwrap_or(None);
            let group_id: i32 = row.try_get("group_id").unwrap_or(0);
            let watcher_id: i32 = row.try_get("watcher_id").unwrap_or(0);
            let revision: i64 = row.try_get("revision").unwrap_or(0);
            let size: u64 = row.try_get("size").unwrap_or(0);
            let key_id_opt: Option<String> = row.try_get("key_id").ok();

            info!("✅ find_file_by_criteria 결과: file_id={}, filename={}, watcher_id={}, revision={}", 
                   file_id, filename, watcher_id, revision);

            // datetime을 Unix timestamp로 변환
            let timestamp = prost_types::Timestamp {
                seconds: updated_ts.unwrap_or(0),
                nanos: 0,
            };

            // FileInfo 객체 생성
            let file_info = FileInfo {
                file_id,
                filename,
                file_hash,
                device_hash,
                group_id,
                watcher_id,
                is_encrypted: false,
                file_path,
                updated_time: timestamp,
                revision,
                account_hash: acc_hash,
                size,
                key_id: key_id_opt,
            };

            info!(
                "✅ find_file_by_criteria 완료: file_id={}, revision={}",
                file_id, revision
            );
            Ok(Some(file_info))
        } else {
            warn!("❌ 파일 검색 실패 - 조건에 맞는 파일을 찾을 수 없음:");
            warn!("   account_hash: {}", account_hash);
            warn!("   file_path: {}", file_path);
            warn!("   filename: {}", filename);
            warn!("   search_path: {}", search_path);
            warn!("   search_filename: {}", search_filename);
            Ok(None)
        }
    }

    /// 파일 ID로 존재 여부와 삭제 상태 확인
    async fn check_file_exists(&self, file_id: u64) -> Result<(bool, bool)> {
        info!("🔍 check_file_exists 호출: file_id={}", file_id);
        let is_deleted_opt: Option<i8> =
            sqlx::query_scalar(r#"SELECT is_deleted FROM files WHERE file_id = ?"#)
                .bind(file_id as i64)
                .fetch_optional(self.get_sqlx_pool())
                .await
                .map_err(|e| {
                    error!("❌ 파일 존재 조회 SQL 오류(sqlx): {}", e);
                    StorageError::Database(format!("파일 존재 조회 SQL 오류: {}", e))
                })?;

        match is_deleted_opt {
            Some(raw) => Ok((true, raw != 0)),
            None => {
                warn!(
                    "⚠️ 파일이 데이터베이스에 존재하지 않음: file_id={}",
                    file_id
                );
                Ok((false, false))
            }
        }
    }

    /// TTL 기반 물리 삭제: is_deleted=1 이고 updated_time이 NOW()-ttl 초 이전인 레코드 제거
    async fn purge_deleted_files_older_than(&self, ttl_secs: i64) -> Result<u64> {
        let affected = sqlx::query(r#"DELETE FROM files WHERE is_deleted = 1 AND updated_time < FROM_UNIXTIME(UNIX_TIMESTAMP() - ?)"#)
            .bind(ttl_secs)
            .execute(self.get_sqlx_pool())
            .await
            .map_err(|e| StorageError::Database(format!("Failed to purge deleted files: {}", e)))?
            .rows_affected();
        Ok(affected as u64)
    }

    /// 파일 경로 기준 리비전 상한: 각 (account_hash,file_path,server_group_id)별 최신 max_revisions만 유지하고 나머지는 is_deleted=1 처리
    async fn trim_old_revisions(&self, max_revisions: i32) -> Result<u64> {
        // MySQL 5.7 호환: 서브쿼리로 최신 N개를 제외하고 나머지를 is_deleted=1로 마킹
        // 논리 삭제만 수행 (물리 삭제는 TTL로 정리)
        let sql = r#"
            UPDATE files f
            JOIN (
                SELECT t.account_hash, t.file_path, t.server_group_id, t.file_id, t.revision
                FROM files t
                WHERE t.is_deleted = 0
                AND (
                    SELECT COUNT(*) FROM files i
                    WHERE i.account_hash = t.account_hash
                      AND i.file_path = t.file_path
                      AND i.server_group_id = t.server_group_id
                      AND i.is_deleted = 0
                      AND i.updated_time >= t.updated_time
                ) > ?
            ) AS x
            ON f.account_hash = x.account_hash
               AND f.file_path = x.file_path
               AND f.server_group_id = x.server_group_id
               AND f.file_id = x.file_id
               AND f.revision = x.revision
            SET f.is_deleted = 1
        "#;
        let affected = sqlx::query(sql)
            .bind(max_revisions)
            .execute(self.get_sqlx_pool())
            .await
            .map_err(|e| StorageError::Database(format!("Failed to trim old revisions: {}", e)))?
            .rows_affected();
        Ok(affected as u64)
    }
}
