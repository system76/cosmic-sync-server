use chrono::prelude::*;
use mysql_async::{prelude::*, Row, Value, TxOpts};
use tracing::{debug, error, info, warn};
use std::path::Path;
use std::sync::Arc;
use sqlx::{mysql::MySqlPool, Executor};
use std::io::{Error, ErrorKind};
use tokio::task;

use prost_types::Timestamp;

use crate::models::file::FileInfo;
use crate::storage::{Result, StorageError};
use crate::storage::mysql::MySqlStorage;

/// MySQL 파일 관련 기능 확장 트레이트
pub trait MySqlFileExt {
    /// 파일 정보 저장
    async fn store_file_info(&self, file_info: FileInfo) -> Result<u64>;
    
    /// 파일 정보 조회
    async fn get_file_info(&self, file_id: u64) -> Result<Option<FileInfo>>;
    
    /// 파일 정보 조회 (삭제된 파일 포함)
    async fn get_file_info_include_deleted(&self, file_id: u64) -> Result<Option<(FileInfo, bool)>>;
    
    /// 경로로 파일 정보 조회
    async fn get_file_info_by_path(&self, account_hash: &str, file_path: &str, group_id: i32) -> Result<Option<FileInfo>>;
    
    /// 해시로 파일 검색
    async fn get_file_by_hash(&self, account_hash: &str, file_hash: &str) -> Result<Option<FileInfo>>;
    
    /// 경로와 파일명으로 파일 검색
    async fn find_file_by_path_and_name(&self, account_hash: &str, file_path: &str, filename: &str, revision: i64) -> Result<Option<FileInfo>>;
    
    /// 경로와 파일명과 그룹 ID로 파일 검색
    async fn find_file_by_criteria(&self, account_hash: &str, group_id: i32, watcher_id: i32, file_path: &str, filename: &str) -> Result<Option<FileInfo>>;
    
    /// 파일 삭제
    async fn delete_file(&self, account_hash: &str, file_id: u64) -> Result<()>;
    
    /// 파일 목록 조회
    async fn list_files(&self, account_hash: &str, group_id: i32, upload_time_from: Option<i64>) -> Result<Vec<FileInfo>>;
    
    /// 파일 목록 조회 (특정 디바이스 해시 제외)
    async fn list_files_except_device(&self, account_hash: &str, group_id: i32, exclude_device_hash: &str, upload_time_from: Option<i64>) -> Result<Vec<FileInfo>>;
    
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
}

impl MySqlFileExt for MySqlStorage {
    /// 파일 정보 저장
    async fn store_file_info(&self, file_info: FileInfo) -> Result<u64> {
        let pool = self.get_pool();
        
        let mut conn = match pool.get_conn().await {
            Ok(conn) => conn,
            Err(e) => {
                error!("❌ 데이터베이스 연결 실패: {}", e);
                return Err(StorageError::Database(format!("데이터베이스 연결 실패: {}", e)));
            }
        };
        
        let now = Utc::now().timestamp();
        let updated_time = file_info.updated_time.seconds;
        
        // Convert timestamps to MySQL datetime format
        let now_datetime = match std::panic::catch_unwind(|| {
            crate::storage::mysql::MySqlStorage::timestamp_to_datetime(now)
        }) {
            Ok(dt) => dt,
            Err(_) => {
                error!("❌ now 타임스탬프 변환 중 panic 발생");
                return Err(StorageError::Database("타임스탬프 변환 오류".to_string()));
            }
        };
        
        let updated_datetime = match std::panic::catch_unwind(|| {
            crate::storage::mysql::MySqlStorage::timestamp_to_datetime(updated_time)
        }) {
            Ok(dt) => dt,
            Err(_) => {
                error!("❌ updated 타임스탬프 변환 중 panic 발생");
                return Err(StorageError::Database("타임스탬프 변환 오류".to_string()));
            }
        };
        
        // 트랜잭션 시작
        let mut tx = match conn.start_transaction(TxOpts::default()).await {
            Ok(tx) => tx,
            Err(e) => {
                error!("❌ 트랜잭션 시작 실패: {}", e);
                return Err(StorageError::Database(format!("트랜잭션 시작 실패: {}", e)));
            }
        };
        // 먼저 file_id로 기존 파일이 있는지 확인
        let existing_by_file_id: Option<(u64, i64)> = match tx.exec_first(
            "SELECT file_id, revision FROM files WHERE file_id = ? AND is_deleted = FALSE LIMIT 1",
            (file_info.file_id,)
        ).await {
            Ok(result) => result,
            Err(e) => {
                error!("❌ file_id로 파일 확인 실패: {}", e);
                return Err(StorageError::Database(format!("file_id로 파일 확인 실패: {}", e)));
            }
        };
        
        if let Some((existing_file_id, current_revision)) = existing_by_file_id {
            // 동일한 file_id가 이미 존재하는 경우 - 파일 정보만 업데이트
            
            // 기존 파일 정보 업데이트
            match tx.exec_drop(
                r"UPDATE files SET 
                    file_hash = ?, device_hash = ?, updated_time = ?, size = ?,
                    revision = revision + 1
                  WHERE file_id = ?",
                (
                    &file_info.file_hash,
                    &file_info.device_hash,
                    updated_datetime,
                    file_info.size,
                    file_info.file_id
                )
            ).await {
                Ok(_) => {},
                Err(e) => {
                    error!("❌ 파일 정보 업데이트 실패: {}", e);
                    return Err(StorageError::Database(format!("파일 정보 업데이트 실패: {}", e)));
                }
            }
            
            // 트랜잭션 커밋
            match tx.commit().await {
                Ok(_) => {},
                Err(e) => {
                    error!("❌ 트랜잭션 커밋 실패: {}", e);
                    return Err(StorageError::Database(format!("트랜잭션 커밋 실패: {}", e)));
                }
            }
            
            return Ok(file_info.file_id);
        }
        
        debug!("🔍 최대 revision 조회 중...");
        // 같은 경로와 파일명을 가진 모든 파일(삭제된 파일 포함)의 최대 revision 조회
        let max_revision: Option<i64> = match tx.exec_first(
            "SELECT COALESCE(MAX(revision), 0) FROM files WHERE account_hash = ? AND file_path = ? AND filename = ? AND server_group_id = ?",
            (&file_info.account_hash, &file_info.file_path, &file_info.filename, file_info.group_id)
        ).await {
            Ok(result) => {
                debug!("✅ 최대 revision 조회 성공: result={:?}", result);
                result
            },
            Err(e) => {
                error!("❌ 최대 revision 조회 실패: {}", e);
                return Err(StorageError::Database(format!("최대 revision 조회 실패: {}", e)));
            }
        };
        
        let new_revision = max_revision.unwrap_or(0) + 1;
        
        debug!("📊 revision 계산: max_revision={:?}, new_revision={}", 
              max_revision, new_revision);
        
        debug!("🔍 활성 파일 확인 중...");
        // 동일한 파일 경로와 이름으로 삭제되지 않은 파일이 있는지 확인
        let existing_active_file: Option<(u64,)> = match tx.exec_first(
            "SELECT file_id FROM files WHERE account_hash = ? AND file_path = ? AND filename = ? AND server_group_id = ? AND is_deleted = FALSE LIMIT 1",
            (&file_info.account_hash, &file_info.file_path, &file_info.filename, file_info.group_id)
        ).await {
            Ok(result) => {
                debug!("✅ 활성 파일 확인 성공: result={:?}", result);
                result
            },
            Err(e) => {
                error!("❌ 활성 파일 존재 여부 확인 실패: {}", e);
                return Err(StorageError::Database(format!("활성 파일 존재 여부 확인 실패: {}", e)));
            }
        };
        
        if let Some((existing_file_id,)) = existing_active_file {
            // 기존 활성 파일이 있으면 삭제 상태로 표시
            debug!("🗑️ 기존 활성 파일 삭제 표시: existing_file_id={}", existing_file_id);
            
            match tx.exec_drop(
                "UPDATE files SET is_deleted = TRUE WHERE file_id = ?",
                (existing_file_id,)
            ).await {
                Ok(_) => debug!("✅ 기존 파일 삭제 표시 성공"),
                Err(e) => {
                    error!("❌ 기존 파일 삭제 표시 실패: {}", e);
                    return Err(StorageError::Database(format!("기존 파일 삭제 표시 실패: {}", e)));
                }
            }
        }
        
        // 새 파일 삽입 (계산된 revision 사용)
        info!("💾 새 파일 INSERT 경로: file_id={}, revision={}, filename={}", 
             file_info.file_id, new_revision, file_info.filename);
        
        let params: Vec<mysql_async::Value> = vec![
            file_info.file_id.into(),
            file_info.account_hash.clone().into(),
            file_info.device_hash.clone().into(),
            file_info.file_path.clone().into(),
            file_info.filename.clone().into(),
            file_info.file_hash.clone().into(),
            file_info.size.into(),
            new_revision.into(),              // 계산된 revision 사용
            now_datetime.into(),              // datetime format
            updated_datetime.into(),          // datetime format
            file_info.group_id.into(),        // 클라이언트 ID (호환성을 위해 유지)
            file_info.watcher_id.into(),      // 클라이언트 ID (호환성을 위해 유지)
            file_info.group_id.into(),        // 서버 ID (실제로는 이미 서버 ID임)
            file_info.watcher_id.into()       // 서버 ID (실제로는 이미 서버 ID임)
        ];
        
        match tx.exec_drop(
            r"INSERT INTO files (
                file_id, account_hash, device_hash, file_path, filename, file_hash, size,
                is_deleted, revision, created_time, updated_time, group_id, watcher_id,
                server_group_id, server_watcher_id
            ) VALUES (?, ?, ?, ?, ?, ?, ?, FALSE, ?, ?, ?, ?, ?, ?, ?)",
            params
        ).await {
            Ok(_) => debug!("✅ 새 파일 정보 삽입 성공"),
            Err(e) => {
                error!("❌ 새 파일 정보 삽입 실패: {}", e);
                return Err(StorageError::Database(format!("새 파일 정보 삽입 실패: {}", e)));
            }
        }
        
        debug!("🔄 트랜잭션 커밋 중...");
        // 트랜잭션 커밋
        match tx.commit().await {
            Ok(_) => {
                info!("🎉 파일 정보 저장 완료: file_id={}, revision={}, filename={}", 
                     file_info.file_id, new_revision, file_info.filename);
            },
            Err(e) => {
                error!("❌ 트랜잭션 커밋 실패: {}", e);
                return Err(StorageError::Database(format!("트랜잭션 커밋 실패: {}", e)));
            }
        }
        
        Ok(file_info.file_id)
    }
    
    /// 파일 정보 조회
    async fn get_file_info(&self, file_id: u64) -> Result<Option<FileInfo>> {
        let pool = self.get_pool();
        let mut conn = pool.get_conn().await.map_err(|e| {
            StorageError::Database(format!("데이터베이스 연결 실패: {}", e))
        })?;
        
        debug!("파일 정보 조회: file_id={}", file_id);
        
        // file_id로 파일 정보 조회 (삭제되지 않은 파일만)
        let row: Option<Row> = conn.exec_first(
            r"SELECT 
                file_id, account_hash, device_hash, file_path, filename, file_hash,
                DATE_FORMAT(created_time, '%Y-%m-%d %H:%i:%s') as created_time_str,
                DATE_FORMAT(updated_time, '%Y-%m-%d %H:%i:%s') as updated_time_str,
                group_id, watcher_id, is_deleted, revision, size
              FROM files 
              WHERE file_id = ? AND is_deleted = FALSE",
            (file_id,)
        ).await.map_err(|e| StorageError::Database(format!("파일 정보 조회 실패: {}", e)))?;
        
        if let Some(row) = row {
            // Row 객체에서 필요한 필드 추출
            let file_id: u64 = row.get(0).unwrap_or(0);
            let account_hash: String = row.get(1).unwrap_or_else(|| String::from(""));
            let device_hash: String = row.get(2).unwrap_or_else(|| String::from(""));
            let file_path: String = row.get(3).unwrap_or_else(|| String::from(""));
            let filename: String = row.get(4).unwrap_or_else(|| String::from(""));
            let file_hash: String = row.get(5).unwrap_or_else(|| String::from(""));
            let created_time_str: String = row.get(6).unwrap_or_else(|| String::from("1970-01-01 00:00:00"));
            let updated_time_str: String = row.get(7).unwrap_or_else(|| String::from("1970-01-01 00:00:00"));
            let group_id: i32 = row.get(8).unwrap_or(0);
            let watcher_id: i32 = row.get(9).unwrap_or(0);
            let _is_deleted: bool = row.get(10).unwrap_or(false);
            let revision: i64 = row.get(11).unwrap_or(0);
            let size: u64 = row.get(12).unwrap_or(0);
            
            // datetime을 Unix timestamp로 변환
            let updated_time = crate::storage::mysql::MySqlStorage::datetime_to_timestamp(&updated_time_str)
                .unwrap_or(0);
            
            // 타임스탬프 생성
            let timestamp = prost_types::Timestamp {
                seconds: updated_time,
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
                is_encrypted: false, // 기본값 사용
                file_path,
                updated_time: timestamp,
                revision,
                account_hash,
                size,
            };
            
            debug!("파일 정보 조회 성공: file_id={}", file_id);
            Ok(Some(file_info))
        } else {
            debug!("파일 정보 없음 또는 이미 삭제됨: file_id={}", file_id);
            Ok(None)
        }
    }
    
    /// 파일 정보 조회 (삭제된 파일 포함)
    async fn get_file_info_include_deleted(&self, file_id: u64) -> Result<Option<(FileInfo, bool)>> {
        let pool = self.get_pool();
        let mut conn = pool.get_conn().await.map_err(|e| {
            StorageError::Database(format!("데이터베이스 연결 실패: {}", e))
        })?;
        
        debug!("파일 정보 조회 (삭제 포함): file_id={}", file_id);
        
        // file_id로 파일 정보 조회 (삭제된 파일 포함)
        let row: Option<Row> = conn.exec_first(
            r"SELECT 
                file_id, account_hash, device_hash, file_path, filename, file_hash,
                DATE_FORMAT(created_time, '%Y-%m-%d %H:%i:%s') as created_time_str,
                DATE_FORMAT(updated_time, '%Y-%m-%d %H:%i:%s') as updated_time_str,
                group_id, watcher_id, is_deleted, revision, size
              FROM files 
              WHERE file_id = ?",
            (file_id,)
        ).await.map_err(|e| StorageError::Database(format!("파일 정보 조회 실패: {}", e)))?;
        
        if let Some(row) = row {
            // Row 객체에서 필요한 필드 추출
            let file_id: u64 = row.get(0).unwrap_or(0);
            let account_hash: String = row.get(1).unwrap_or_else(|| String::from(""));
            let device_hash: String = row.get(2).unwrap_or_else(|| String::from(""));
            let file_path: String = row.get(3).unwrap_or_else(|| String::from(""));
            let filename: String = row.get(4).unwrap_or_else(|| String::from(""));
            let file_hash: String = row.get(5).unwrap_or_else(|| String::from(""));
            let created_time_str: String = row.get(6).unwrap_or_else(|| String::from("1970-01-01 00:00:00"));
            let updated_time_str: String = row.get(7).unwrap_or_else(|| String::from("1970-01-01 00:00:00"));
            let group_id: i32 = row.get(8).unwrap_or(0);
            let watcher_id: i32 = row.get(9).unwrap_or(0);
            let is_deleted: bool = row.get(10).unwrap_or(false);
            let revision: i64 = row.get(11).unwrap_or(0);
            let size: u64 = row.get(12).unwrap_or(0);
            
            // datetime을 Unix timestamp로 변환
            let updated_time = crate::storage::mysql::MySqlStorage::datetime_to_timestamp(&updated_time_str)
                .unwrap_or(0);
            
            // 타임스탬프 생성
            let timestamp = prost_types::Timestamp {
                seconds: updated_time,
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
                is_encrypted: false, // 기본값 사용
                file_path,
                updated_time: timestamp,
                revision,
                account_hash,
                size,
            };
            
            debug!("파일 정보 조회 성공 (삭제 포함): file_id={}, is_deleted={}", file_id, is_deleted);
            Ok(Some((file_info, is_deleted)))
        } else {
            debug!("파일 정보 없음 (ID 자체가 없음): file_id={}", file_id);
            Ok(None)
        }
    }
    
    /// 경로로 파일 정보 조회
    async fn get_file_info_by_path(&self, account_hash: &str, file_path: &str, group_id: i32) -> Result<Option<FileInfo>> {
        let pool = self.get_pool();
        let mut conn = pool.get_conn().await.map_err(|e| {
            StorageError::Database(format!("데이터베이스 연결 실패: {}", e))
        })?;
        
        debug!("경로로 파일 정보 조회: account_hash={}, file_path={}, group_id={}", account_hash, file_path, group_id);
        
        // 경로에서 파일명 추출
        let path = Path::new(file_path);
        let filename = path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        
        let file_directory = path.parent()
            .and_then(|dir| dir.to_str())
            .unwrap_or("");
        
        debug!("파일명: {}, 디렉토리: {}", filename, file_directory);
        
        // 계정 해시, 경로, 그룹 ID로 파일 정보 조회
        let row: Option<Row> = conn.exec_first(
            r"SELECT 
                file_id, account_hash, device_hash, file_path, filename, file_hash,
                DATE_FORMAT(created_time, '%Y-%m-%d %H:%i:%s') as created_time_str,
                DATE_FORMAT(updated_time, '%Y-%m-%d %H:%i:%s') as updated_time_str,
                group_id, watcher_id, revision, size
              FROM files 
              WHERE account_hash = ? AND file_path = ? AND server_group_id = ? AND is_deleted = FALSE
              ORDER BY revision DESC LIMIT 1",
            (account_hash, file_path, group_id)
        ).await.map_err(|e| StorageError::Database(format!("파일 정보 조회 실패: {}", e)))?;
        
        if let Some(row) = row {
            // Row 객체에서 필요한 필드 추출
            let file_id: u64 = row.get(0).unwrap_or(0);
            let account_hash: String = row.get(1).unwrap_or_else(|| String::from(""));
            let device_hash: String = row.get(2).unwrap_or_else(|| String::from(""));
            let file_path: String = row.get(3).unwrap_or_else(|| String::from(""));
            let filename: String = row.get(4).unwrap_or_else(|| String::from(""));
            let file_hash: String = row.get(5).unwrap_or_else(|| String::from(""));
            let created_time_str: String = row.get(6).unwrap_or_else(|| String::from("1970-01-01 00:00:00"));
            let updated_time_str: String = row.get(7).unwrap_or_else(|| String::from("1970-01-01 00:00:00"));
            let group_id: i32 = row.get(8).unwrap_or(0);
            let watcher_id: i32 = row.get(9).unwrap_or(0);
            let revision: i64 = row.get(10).unwrap_or(0);
            let size: u64 = row.get(11).unwrap_or(0);
            
            // datetime을 Unix timestamp로 변환
            let updated_time = crate::storage::mysql::MySqlStorage::datetime_to_timestamp(&updated_time_str)
                .unwrap_or(0);
            
            // 타임스탬프 생성
            let timestamp = prost_types::Timestamp {
                seconds: updated_time,
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
                is_encrypted: false, // 기본값 사용
                file_path,
                updated_time: timestamp,
                revision,
                account_hash,
                size,
            };
            
            debug!("경로로 파일 정보 조회 성공: file_id={}", file_id);
            Ok(Some(file_info))
        } else {
            debug!("경로에 해당하는 파일 정보 없음: file_path={}", file_path);
            Ok(None)
        }
    }
    
    /// 해시로 파일 검색
    async fn get_file_by_hash(&self, account_hash: &str, file_hash: &str) -> Result<Option<FileInfo>> {
        let pool = self.get_pool();
        let mut conn = pool.get_conn().await.map_err(|e| {
            StorageError::Database(format!("데이터베이스 연결 실패: {}", e))
        })?;
        
        debug!("해시로 파일 검색: account_hash={}, file_hash={}", account_hash, file_hash);
        
        // 계정 해시와 파일 해시로 파일 정보 조회
        let row: Option<Row> = conn.exec_first(
            r"SELECT 
                file_id, account_hash, device_hash, file_path, filename, file_hash,
                DATE_FORMAT(created_time, '%Y-%m-%d %H:%i:%s') as created_time_str,
                DATE_FORMAT(updated_time, '%Y-%m-%d %H:%i:%s') as updated_time_str,
                group_id, watcher_id, revision, size
              FROM files 
              WHERE account_hash = ? AND file_hash = ? AND is_deleted = FALSE
              ORDER BY revision DESC LIMIT 1",
            (account_hash, file_hash)
        ).await.map_err(|e| StorageError::Database(format!("파일 검색 실패: {}", e)))?;
        
        if let Some(row) = row {
            // Row 객체에서 필요한 필드 추출
            let file_id: u64 = row.get(0).unwrap_or(0);
            let account_hash: String = row.get(1).unwrap_or_else(|| String::from(""));
            let device_hash: String = row.get(2).unwrap_or_else(|| String::from(""));
            let file_path: String = row.get(3).unwrap_or_else(|| String::from(""));
            let filename: String = row.get(4).unwrap_or_else(|| String::from(""));
            let file_hash: String = row.get(5).unwrap_or_else(|| String::from(""));
            let created_time_str: String = row.get(6).unwrap_or_else(|| String::from("1970-01-01 00:00:00"));
            let updated_time_str: String = row.get(7).unwrap_or_else(|| String::from("1970-01-01 00:00:00"));
            let group_id: i32 = row.get(8).unwrap_or(0);
            let watcher_id: i32 = row.get(9).unwrap_or(0);
            let revision: i64 = row.get(10).unwrap_or(0);
            let size: u64 = row.get(11).unwrap_or(0);
            
            // datetime을 Unix timestamp로 변환
            let updated_time = crate::storage::mysql::MySqlStorage::datetime_to_timestamp(&updated_time_str)
                .unwrap_or(0);
            
            // 타임스탬프 생성
            let timestamp = prost_types::Timestamp {
                seconds: updated_time,
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
                is_encrypted: false, // 기본값 사용
                file_path,
                updated_time: timestamp,
                revision,
                account_hash,
                size,
            };
            
            debug!("해시로 파일 검색 성공: file_id={}", file_id);
            Ok(Some(file_info))
        } else {
            debug!("해시에 해당하는 파일 정보 없음: file_hash={}", file_hash);
            Ok(None)
        }
    }
    
    /// 경로와 파일명으로 파일 검색
    async fn find_file_by_path_and_name(&self, account_hash: &str, file_path: &str, filename: &str, revision: i64) -> Result<Option<FileInfo>> {
        let pool = self.get_pool();
        let mut conn = pool.get_conn().await.map_err(|e| {
            StorageError::Database(format!("데이터베이스 연결 실패: {}", e))
        })?;
        
        debug!("경로와 파일명으로 파일 검색: account_hash={}, file_path={}, filename={}, revision={}", 
              account_hash, file_path, filename, revision);
        
        // 경로 분석: 이미 파일명이 포함되어 있는지 확인
        let (search_path, search_filename) = if file_path.ends_with(&format!("/{}", filename)) || file_path.ends_with(filename) {
            // 파일 경로에 이미 파일명이 포함된 경우
            debug!("파일 경로에 이미 파일명이 포함됨: {}", file_path);
            
            // 파일명 추출: 마지막 / 이후의 내용 또는 전체 경로
            let last_slash_pos = file_path.rfind('/');
            
            match last_slash_pos {
                Some(pos) => {
                    // 마지막 / 이전까지가 경로, 그 이후가 파일명
                    let path = &file_path[0..pos];
                    let fname = &file_path[pos+1..];
                    debug!("추출된 경로: {}, 파일명: {}", path, fname);
                    (path.to_string(), fname.to_string())
                },
                None => {
                    // /가 없는 경우 전체가 파일명
                    debug!("경로 없음, 파일명만 있음: {}", file_path);
                    ("".to_string(), file_path.to_string())
                }
            }
        } else {
            // 경로와 파일명이 분리되어 있는 경우
            debug!("경로와 파일명 분리: 경로={}, 파일명={}", file_path, filename);
            (file_path.to_string(), filename.to_string())
        };
        
        debug!("검색 경로: {}, 파일명: {}", search_path, search_filename);
        
        // 정확한 경로와 파일명으로 검색
        let mut query = String::from(
            r"SELECT 
                file_id, account_hash, device_hash, file_path, filename, file_hash,
                DATE_FORMAT(created_time, '%Y-%m-%d %H:%i:%s') as created_time_str,
                DATE_FORMAT(updated_time, '%Y-%m-%d %H:%i:%s') as updated_time_str,
                group_id, watcher_id, revision, size
              FROM files 
              WHERE account_hash = ? AND file_path = ? AND filename = ? AND is_deleted = FALSE"
        );
        
        let mut params: Vec<Value> = vec![
            account_hash.into(),
            search_path.clone().into(),
            search_filename.clone().into(),
        ];
        
        if revision > 0 {
            query.push_str(" AND revision = ?");
            params.push(revision.into());
        }
        
        query.push_str(" ORDER BY revision DESC LIMIT 1");
        
        // 정확한 경로로 검색 먼저 시도
        let mut row: Option<Row> = conn.exec_first(query.clone(), params.clone())
            .await.map_err(|e| StorageError::Database(format!("파일 검색 실패(정확한 검색): {}", e)))?;
            
        // 파일을 찾지 못했다면, file_path에 전체 경로(파일명 포함)가 있는 경우를 대비해 추가 검색
        if row.is_none() {
            debug!("정확한 경로로 검색 실패, 전체 경로 검색 시도: {}", file_path);
            
            // 전체 경로로 검색 쿼리
            let mut alt_query = String::from(
                r"SELECT 
                    file_id, account_hash, device_hash, file_path, filename, file_hash,
                    DATE_FORMAT(created_time, '%Y-%m-%d %H:%i:%s') as created_time_str,
                    DATE_FORMAT(updated_time, '%Y-%m-%d %H:%i:%s') as updated_time_str,
                    group_id, watcher_id, revision, size
                  FROM files 
                  WHERE account_hash = ? AND 
                        (file_path = ? OR CONCAT(file_path, '/', filename) = ?) AND is_deleted = FALSE"
            );
            
            let mut alt_params: Vec<Value> = vec![
                account_hash.into(),
                file_path.clone().into(), // 전체 경로가 file_path에 저장된 경우
                file_path.clone().into(), // 경로+파일명이 합쳐진 전체 경로와 비교
            ];
            
            if revision > 0 {
                alt_query.push_str(" AND revision = ?");
                alt_params.push(revision.into());
            }
            
            alt_query.push_str(" ORDER BY revision DESC LIMIT 1");
            
            row = conn.exec_first(alt_query, alt_params)
                .await.map_err(|e| StorageError::Database(format!("파일 검색 실패(대체 검색): {}", e)))?;
        }
        
        // 계정 해시, 경로, 파일명으로 파일 정보 조회
        if let Some(row) = row {
            debug!("✅ 파일 찾음!");
            // Row 객체에서 필요한 필드 추출
            let file_id: u64 = row.get(0).unwrap_or(0);
            let acc_hash: String = row.get(1).unwrap_or_else(|| String::from(""));
            let device_hash: String = row.get(2).unwrap_or_else(|| String::from(""));
            let file_path: String = row.get(3).unwrap_or_else(|| String::from(""));
            let filename: String = row.get(4).unwrap_or_else(|| String::from(""));
            let file_hash: String = row.get(5).unwrap_or_else(|| String::from(""));
            let created_time_str: String = row.get(6).unwrap_or_else(|| String::from("1970-01-01 00:00:00"));
            let updated_time_str: String = row.get(7).unwrap_or_else(|| String::from("1970-01-01 00:00:00"));
            let group_id: i32 = row.get(8).unwrap_or(0);
            let watcher_id: i32 = row.get(9).unwrap_or(0);
            let revision: i64 = row.get(10).unwrap_or(0);
            let size: u64 = row.get(11).unwrap_or(0);
            
            debug!("✅ 파일 정보: file_id={}, filename={}, watcher_id={}, revision={}", 
                   file_id, filename, watcher_id, revision);
            
            // datetime을 Unix timestamp로 변환
            let updated_time = crate::storage::mysql::MySqlStorage::datetime_to_timestamp(&updated_time_str)
                .unwrap_or(0);
            
            // 타임스탬프 생성
            let timestamp = prost_types::Timestamp {
                seconds: updated_time,
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
            };
            
            debug!("경로와 파일명으로 파일 검색 성공: file_id={}, updated_time={}", file_id, updated_time_str);
            Ok(Some(file_info))
        } else {
            warn!("❌ 파일 검색 실패 - 조건에 맞는 파일을 찾을 수 없음:");
            warn!("   account_hash: {}", account_hash);
            warn!("   file_path: {}", file_path);
            warn!("   filename: {}", filename);
            warn!("   search_path: {}", search_path);
            warn!("   search_filename: {}", search_filename);
            warn!("   revision: {}", revision);
            Ok(None)
        }
    }
    
    /// 파일 삭제 (metadata and content)
    async fn delete_file(&self, account_hash: &str, file_id: u64) -> Result<()> {
        let pool = self.get_pool();
        let mut conn = pool.get_conn().await.map_err(|e| {
            StorageError::Database(format!("데이터베이스 연결 실패: {}", e))
        })?;
        
        info!("파일 삭제: account_hash={}, file_id={}", account_hash, file_id);
        
        // 트랜잭션 시작
        let mut tx = conn.start_transaction(TxOpts::default()).await
            .map_err(|e| StorageError::Database(format!("트랜잭션 시작 실패: {}", e)))?;
        
        // 파일이 존재하고 사용자에게 속하는지 확인
        let file_exists: Option<(u64, i64, String, String, String, i32, i32)> = tx.exec_first(
            "SELECT file_id, revision, file_path, filename, device_hash, group_id, watcher_id 
             FROM files WHERE file_id = ? AND account_hash = ?",
            (file_id, account_hash)
        ).await.map_err(|e| StorageError::Database(format!("파일 존재 여부 확인 실패: {}", e)))?;
        
        if file_exists.is_none() {
            debug!("삭제할 파일이 없거나 해당 사용자의 파일이 아님: file_id={}, account_hash={}", file_id, account_hash);
            return Err(StorageError::NotFound(format!("파일을 찾을 수 없음: {}", file_id)));
        }
        
        let (_, current_revision, file_path, filename, device_hash, group_id, watcher_id) = file_exists.unwrap();
        let new_revision = current_revision + 1;
        
        debug!("파일 삭제 처리: file_id={}, file_path={}, filename={}, current_revision={}, new_revision={}", 
               file_id, file_path, filename, current_revision, new_revision);
        
        let now = Utc::now().timestamp();
        let now_datetime = crate::storage::mysql::MySqlStorage::timestamp_to_datetime(now);
        
        // 1. 기존 파일 레코드를 is_deleted=1로 업데이트
        tx.exec_drop(
            "UPDATE files SET is_deleted = 1 WHERE file_id = ?",
            (file_id,)
        ).await.map_err(|e| StorageError::Database(format!("기존 파일 삭제 표시 실패: {}", e)))?;
        
        // 2. 같은 파일 경로와 이름을 가진 이전 revision들도 모두 is_deleted=1로 업데이트
        tx.exec_drop(
            "UPDATE files SET is_deleted = 1 
             WHERE account_hash = ? AND file_path = ? AND filename = ? AND group_id = ?",
            (account_hash, &file_path, &filename, group_id)
        ).await.map_err(|e| StorageError::Database(format!("이전 버전 파일 삭제 표시 실패: {}", e)))?;
        
        // 3. 삭제 이력 추가
        debug!("삭제 이력 추가: file_path={}, filename={}", file_path, filename);
        
        // 새로운 file_id 생성 (랜덤값)
        let new_file_id = rand::random::<u64>();
        
        // file_id 필드를 명시적으로 지정하여 INSERT
        tx.exec_drop(
            "INSERT INTO files 
            (file_id, account_hash, device_hash, file_path, filename, file_hash, size) 
            VALUES (?, ?, ?, ?, ?, ?, ?)",
            (
                new_file_id,
                account_hash,
                device_hash.clone(),
                file_path.clone(),
                filename.clone(),
                file_path.clone(),
                0
            )
        ).await.map_err(|e| StorageError::Database(format!("삭제 이력 추가 실패 (1단계): {}", e)))?;
        
        // 나머지 필드 업데이트
        tx.exec_drop(
            "UPDATE files SET 
             is_deleted = 1, 
             revision = ?,
             created_time = ?,
             updated_time = ?,
             group_id = ?,
             watcher_id = ?
             WHERE file_id = ?",
            (
                new_revision,
                now_datetime.clone(),
                now_datetime.clone(),
                group_id,
                watcher_id,
                new_file_id
            )
        ).await.map_err(|e| StorageError::Database(format!("삭제 이력 추가 실패 (2단계): {}", e)))?;
        
        // 트랜잭션 커밋
        tx.commit().await.map_err(|e| StorageError::Database(format!("트랜잭션 커밋 실패: {}", e)))?;
        
        info!("파일 삭제 완료: file_id={}, new_revision={}, 삭제 이력 file_id={}", 
              file_id, new_revision, new_file_id);
        Ok(())
    }
    
    /// 파일 목록 조회
    async fn list_files(&self, account_hash: &str, group_id: i32, upload_time_from: Option<i64>) -> Result<Vec<FileInfo>> {
        let pool = self.get_pool();
        let mut conn = pool.get_conn().await.map_err(|e| {
            StorageError::Database(format!("데이터베이스 연결 실패: {}", e))
        })?;
        
        info!("파일 목록 조회: account_hash={}, group_id={}", account_hash, group_id);
        
        // SQL 쿼리 기본 부분
        let mut query = String::from(
            r"SELECT 
                file_id, account_hash, device_hash, file_path, filename, file_hash,
                DATE_FORMAT(created_time, '%Y-%m-%d %H:%i:%s') as created_time_str,
                DATE_FORMAT(updated_time, '%Y-%m-%d %H:%i:%s') as updated_time_str,
                group_id, watcher_id, revision, size
              FROM files 
              WHERE account_hash = ? AND server_group_id = ? AND is_deleted = FALSE"
        );
        
        // 조건부 추가: 특정 시간 이후의 파일만 조회
        let mut params: Vec<Value> = vec![
            account_hash.into(),
            group_id.into(),
        ];
        
        if let Some(time_from) = upload_time_from {
            query.push_str(" AND updated_time >= ?");
            params.push(time_from.into());
        }
        
        // 정렬 조건 추가
        query.push_str(" ORDER BY updated_time DESC");
        
        debug!("SQL 쿼리: {}", query);
        
        // 쿼리 실행 및 결과 처리
        let rows: Vec<Row> = conn.exec(query, params)
            .await
            .map_err(|e| {
                error!("파일 목록 조회 중 SQL 오류: {}", e);
                StorageError::Database(format!("파일 목록 조회 중 SQL 오류: {}", e))
            })?;
        
        // 결과를 FileInfo 객체로 변환
        let mut files = Vec::with_capacity(rows.len());
        for row in rows {
            // 각 필드값 추출
            let file_id: u64 = row.get(0).unwrap_or(0);
            let filename: String = row.get(1).unwrap_or_else(|| String::from(""));
            let file_hash: String = row.get(2).unwrap_or_else(|| String::from(""));
            let device_hash: String = row.get(3).unwrap_or_else(|| String::from(""));
            let group_id: i32 = row.get(4).unwrap_or(0);
            let watcher_id: i32 = row.get(5).unwrap_or(0);
            let file_path: String = row.get(6).unwrap_or_else(|| String::from(""));
            let created_time_str: String = row.get(7).unwrap_or_else(|| String::from("1970-01-01 00:00:00"));
            let updated_time_str: String = row.get(8).unwrap_or_else(|| String::from("1970-01-01 00:00:00"));
            let revision: i64 = row.get(9).unwrap_or(1);
            let size: u64 = row.get(10).unwrap_or(0);
            
            // datetime을 Unix timestamp로 변환
            let updated_time = crate::storage::mysql::MySqlStorage::datetime_to_timestamp(&updated_time_str)
                .unwrap_or(0);
            
            // Timestamp 생성
            let timestamp = prost_types::Timestamp {
                seconds: updated_time,
                nanos: 0,
            };
            
            // FileInfo 객체 생성 및 벡터에 추가
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
                account_hash: account_hash.to_string(),
                size,
            };
            
            files.push(file_info);
        }
        
        info!("파일 {} 개를 찾았습니다: account_hash={}, group_id={}", files.len(), account_hash, group_id);
        Ok(files)
    }
    
    /// 파일 데이터 저장
    async fn store_file_data(&self, file_id: u64, data_bytes: Vec<u8>) -> Result<()> {
        info!("🔄 MySQL 파일 데이터 저장 시작: file_id={}, data_size={} bytes", 
             file_id, data_bytes.len());
        
        let pool = self.get_pool();
        debug!("📡 데이터베이스 연결 풀에서 연결 요청 중...");
        
        let mut conn = match pool.get_conn().await {
            Ok(conn) => {
                debug!("✅ 데이터베이스 연결 성공");
                conn
            },
            Err(e) => {
                error!("❌ 데이터베이스 연결 실패: {}", e);
                return Err(StorageError::Database(format!("Failed to get connection: {}", e)));
            }
        };
        
        // 현재 시간 (초)
        let now = chrono::Utc::now().timestamp();
        debug!("⏰ 타임스탬프: {}", now);
        
        debug!("🔍 기존 파일 데이터 확인 중...");
        // 기존 데이터 있는지 확인
        let exists: Option<(u64,)> = match conn.exec_first(
            "SELECT file_id FROM file_data WHERE file_id = ?",
            (file_id,)
        ).await {
            Ok(result) => {
                debug!("✅ 기존 데이터 확인 쿼리 성공: result={:?}", result);
                result
            },
            Err(e) => {
                error!("❌ 기존 데이터 확인 실패: {}", e);
                return Err(StorageError::Database(e.to_string()));
            }
        };
        
        if exists.is_some() {
            // 업데이트
            info!("🔄 기존 파일 데이터 업데이트: file_id={}", file_id);
            match conn.exec_drop(
                "UPDATE file_data SET data = ?, updated_at = ? WHERE file_id = ?",
                (data_bytes, now, file_id)
            ).await {
                Ok(_) => {
                    info!("✅ 파일 데이터 업데이트 성공: file_id={}", file_id);
                },
                Err(e) => {
                    error!("❌ 파일 데이터 업데이트 실패: {}", e);
                    return Err(StorageError::Database(e.to_string()));
                }
            }
        } else {
            // 새로 삽입
            info!("💾 새 파일 데이터 삽입: file_id={}", file_id);
            match conn.exec_drop(
                "INSERT INTO file_data (file_id, data, created_at, updated_at) VALUES (?, ?, ?, ?)",
                (file_id, data_bytes, now, now)
            ).await {
                Ok(_) => {
                    info!("✅ 파일 데이터 삽입 성공: file_id={}", file_id);
                },
                Err(e) => {
                    error!("❌ 파일 데이터 삽입 실패: {}", e);
                    return Err(StorageError::Database(e.to_string()));
                }
            }
        }
        
        info!("🎉 파일 데이터 저장 완료: file_id={}", file_id);
        Ok(())
    }
    
    /// 파일 데이터 조회
    async fn get_file_data(&self, file_id: u64) -> Result<Option<Vec<u8>>> {
        let pool = self.get_pool();
        let mut conn = pool.get_conn().await.map_err(|e| {
            StorageError::Database(format!("Failed to get connection: {}", e))
        })?;
        
        let result: Option<Vec<u8>> = conn.exec_first(
            "SELECT data FROM file_data WHERE file_id = ?",
            (file_id,)
        ).await.map_err(|e| StorageError::Database(e.to_string()))?;
        
        Ok(result)
    }
    
    /// 암호화 키 조회
    async fn get_encryption_key(&self, account_hash: &str) -> Result<Option<String>> {
        let pool = self.get_pool();
        let mut conn = pool.get_conn().await.map_err(|e| {
            StorageError::Database(format!("Failed to get connection: {}", e))
        })?;
        
        let result: Option<String> = conn.exec_first(
            "SELECT encryption_key FROM encryption_keys WHERE account_hash = ?",
            (account_hash,)
        ).await.map_err(|e| StorageError::Database(e.to_string()))?;
        
        Ok(result)
    }
    
    /// 암호화 키 저장
    async fn store_encryption_key(&self, account_hash: &str, encryption_key: &str) -> Result<()> {
        let pool = self.get_pool();
        let mut conn = pool.get_conn().await.map_err(|e| {
            StorageError::Database(format!("Failed to get connection: {}", e))
        })?;
        
        // 현재 시간 (초)
        let now = chrono::Utc::now().timestamp();
        
        // 기존 키 있는지 확인
        let exists: Option<(String,)> = conn.exec_first(
            "SELECT account_hash FROM encryption_keys WHERE account_hash = ?",
            (account_hash,)
        ).await.map_err(|e| StorageError::Database(e.to_string()))?;
        
        if exists.is_some() {
            // 업데이트
            conn.exec_drop(
                "UPDATE encryption_keys SET encryption_key = ?, updated_at = ? WHERE account_hash = ?",
                (encryption_key, now, account_hash)
            ).await.map_err(|e| StorageError::Database(e.to_string()))?;
        } else {
            // 새로 삽입
            conn.exec_drop(
                "INSERT INTO encryption_keys (account_hash, encryption_key, created_at, updated_at) VALUES (?, ?, ?, ?)",
                (account_hash, encryption_key, now, now)
            ).await.map_err(|e| StorageError::Database(e.to_string()))?;
        }
        
        Ok(())
    }
    
    /// 파일 목록 조회 (특정 디바이스 해시 제외)
    async fn list_files_except_device(&self, account_hash: &str, group_id: i32, exclude_device_hash: &str, upload_time_from: Option<i64>) -> Result<Vec<FileInfo>> {
        let pool = self.get_pool();
        let mut conn = pool.get_conn().await.map_err(|e| {
            StorageError::Database(format!("데이터베이스 연결 실패: {}", e))
        })?;
        
        info!("파일 목록 조회 (디바이스 제외): account_hash={}, group_id={}, exclude_device={}", 
              account_hash, group_id, exclude_device_hash);
        
        // SQL 쿼리 기본 부분 - 특정 device_hash로 업로드된 파일은 제외
        let mut query = String::from(
            r"SELECT 
                file_id, filename, file_hash, device_hash,
                group_id, watcher_id, file_path, 
                DATE_FORMAT(created_time, '%Y-%m-%d %H:%i:%s') as created_time_str,
                DATE_FORMAT(updated_time, '%Y-%m-%d %H:%i:%s') as updated_time_str,
                revision, size
              FROM files 
              WHERE account_hash = ? AND server_group_id = ? AND device_hash != ? AND is_deleted = FALSE"
        );
        
        // 조건부 추가: 특정 시간 이후의 파일만 조회
        let mut params: Vec<Value> = vec![
            account_hash.into(),
            group_id.into(),
            exclude_device_hash.into(),
        ];
        
        if let Some(time_from) = upload_time_from {
            query.push_str(" AND updated_time >= ?");
            params.push(time_from.into());
        }
        
        // 정렬 조건 추가
        query.push_str(" ORDER BY updated_time DESC");
        
        debug!("SQL 쿼리: {}", query);
        
        // 쿼리 실행 및 결과 처리
        let rows: Vec<Row> = conn.exec(query, params)
            .await
            .map_err(|e| {
                error!("파일 목록 조회 중 SQL 오류: {}", e);
                StorageError::Database(format!("파일 목록 조회 중 SQL 오류: {}", e))
            })?;
        
        // 결과를 FileInfo 객체로 변환
        let mut files = Vec::with_capacity(rows.len());
        for row in rows {
            // 각 필드값 추출
            let file_id: u64 = row.get(0).unwrap_or(0);
            let filename: String = row.get(1).unwrap_or_else(|| String::from(""));
            let file_hash: String = row.get(2).unwrap_or_else(|| String::from(""));
            let device_hash: String = row.get(3).unwrap_or_else(|| String::from(""));
            let group_id: i32 = row.get(4).unwrap_or(0);
            let watcher_id: i32 = row.get(5).unwrap_or(0);
            let file_path: String = row.get(6).unwrap_or_else(|| String::from(""));
            let created_time_str: String = row.get(7).unwrap_or_else(|| String::from("1970-01-01 00:00:00"));
            let updated_time_str: String = row.get(8).unwrap_or_else(|| String::from("1970-01-01 00:00:00"));
            let revision: i64 = row.get(9).unwrap_or(1);
            let size: u64 = row.get(10).unwrap_or(0);
            
            // datetime을 Unix timestamp로 변환
            let updated_time = crate::storage::mysql::MySqlStorage::datetime_to_timestamp(&updated_time_str)
                .unwrap_or(0);
            
            // Timestamp 생성
            let timestamp = prost_types::Timestamp {
                seconds: updated_time,
                nanos: 0,
            };
            
            // FileInfo 객체 생성 및 벡터에 추가
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
                account_hash: account_hash.to_string(),
                size,
            };
            
            files.push(file_info);
        }
        
        info!("파일 {} 개를 찾았습니다 (디바이스 제외): account_hash={}, group_id={}, exclude_device={}", 
              files.len(), account_hash, group_id, exclude_device_hash);
        Ok(files)
    }

    /// 경로와 파일명과 그룹 ID로 파일 검색
    async fn find_file_by_criteria(&self, account_hash: &str, group_id: i32, watcher_id: i32, file_path: &str, filename: &str) -> Result<Option<FileInfo>> {
        let pool = self.get_pool();
        let mut conn = pool.get_conn().await.map_err(|e| {
            StorageError::Database(format!("데이터베이스 연결 실패: {}", e))
        })?;
        
        debug!("🔍 find_file_by_criteria 호출됨:");
        debug!("   account_hash: {}", account_hash);
        debug!("   group_id: {}", group_id);
        debug!("   watcher_id: {}", watcher_id);
        debug!("   file_path: {}", file_path);
        debug!("   filename: {}", filename);
        
        // 경로 분석: 이미 파일명이 포함되어 있는지 확인
        let (search_path, search_filename) = if file_path.ends_with(&format!("/{}", filename)) || file_path.ends_with(filename) {
            // 파일 경로에 이미 파일명이 포함된 경우
            debug!("파일 경로에 이미 파일명이 포함됨: {}", file_path);
            
            // 파일명 추출: 마지막 / 이후의 내용 또는 전체 경로
            let last_slash_pos = file_path.rfind('/');
            
            match last_slash_pos {
                Some(pos) => {
                    // 마지막 / 이전까지가 경로, 그 이후가 파일명
                    let path = &file_path[0..pos];
                    let fname = &file_path[pos+1..];
                    debug!("추출된 경로: {}, 파일명: {}", path, fname);
                    (path.to_string(), fname.to_string())
                },
                None => {
                    // /가 없는 경우 전체가 파일명
                    debug!("경로 없음, 파일명만 있음: {}", file_path);
                    ("".to_string(), file_path.to_string())
                }
            }
        } else {
            // 경로와 파일명이 분리되어 있는 경우
            debug!("경로와 파일명 분리: 경로={}, 파일명={}", file_path, filename);
            (file_path.to_string(), filename.to_string())
        };
        
        debug!("🔍 최종 검색 조건: path='{}', filename='{}'", search_path, search_filename);
        
        // 데이터베이스에서 파일 검색 - 두 가지 패턴으로 검색
        // 1) 분리된 경로와 파일명으로 검색 (search_path + search_filename)
        // 2) 전체 경로가 file_path에 저장된 경우도 검색 (원본 file_path + filename)
        let params: Vec<mysql_async::Value> = vec![
            account_hash.into(),
            group_id.into(),
            watcher_id.into(),
            search_path.clone().into(),
            search_filename.clone().into(),
            file_path.into(),
            filename.into()
        ];
        
        let row: Option<Row> = conn.exec_first(
            r"SELECT 
                file_id, account_hash, device_hash, file_path, filename, file_hash,
                DATE_FORMAT(created_time, '%Y-%m-%d %H:%i:%s') as created_time_str,
                DATE_FORMAT(updated_time, '%Y-%m-%d %H:%i:%s') as updated_time_str,
                group_id, watcher_id, revision, size
              FROM files 
              WHERE account_hash = ? AND server_group_id = ? AND server_watcher_id = ? AND is_deleted = FALSE
                AND (
                  (file_path = ? AND filename = ?) OR 
                  (file_path = ? AND filename = ?)
                )
              ORDER BY revision DESC 
              LIMIT 1",
            params
        ).await.map_err(|e| {
            error!("❌ 파일 검색 쿼리 실행 실패: {}", e);
            StorageError::Database(format!("파일 검색 쿼리 실행 실패: {}", e))
        })?;
        
        if let Some(row) = row {
            debug!("✅ 파일 찾음!");
            // Row 객체에서 필요한 필드 추출
            let file_id: u64 = row.get(0).unwrap_or(0);
            let acc_hash: String = row.get(1).unwrap_or_else(|| String::from(""));
            let device_hash: String = row.get(2).unwrap_or_else(|| String::from(""));
            let file_path: String = row.get(3).unwrap_or_else(|| String::from(""));
            let filename: String = row.get(4).unwrap_or_else(|| String::from(""));
            let file_hash: String = row.get(5).unwrap_or_else(|| String::from(""));
            let created_time_str: String = row.get(6).unwrap_or_else(|| String::from("1970-01-01 00:00:00"));
            let updated_time_str: String = row.get(7).unwrap_or_else(|| String::from("1970-01-01 00:00:00"));
            let group_id: i32 = row.get(8).unwrap_or(0);
            let watcher_id: i32 = row.get(9).unwrap_or(0);
            let revision: i64 = row.get(10).unwrap_or(0);
            let size: u64 = row.get(11).unwrap_or(0);
            
            info!("✅ find_file_by_criteria 결과: file_id={}, filename={}, watcher_id={}, revision={}", 
                   file_id, filename, watcher_id, revision);
            
            // datetime을 Unix timestamp로 변환
            let updated_time = crate::storage::mysql::MySqlStorage::datetime_to_timestamp(&updated_time_str)
                .unwrap_or(0);
            
            // 타임스탬프 생성
            let timestamp = prost_types::Timestamp {
                seconds: updated_time,
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
            };
            
            info!("✅ find_file_by_criteria 완료: file_id={}, revision={}, updated_time={}", 
                 file_id, revision, updated_time_str);
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
        
        let pool = self.get_pool();
        let mut conn = pool.get_conn().await.map_err(|e| {
            StorageError::Database(format!("데이터베이스 연결 실패: {}", e))
        })?;
        
        // 더 상세한 정보를 위해 파일 전체 정보 조회
        let query = "SELECT file_id, filename, file_path, is_deleted, device_hash, account_hash FROM files WHERE file_id = ?";
        info!("🔍 실행할 SQL: {}", query);
        info!("🔍 파라미터: file_id={}", file_id);
        
        let row: Option<(u64, String, String, u8, String, String)> = conn.exec_first(
            query,
            (file_id,)
        ).await.map_err(|e| {
            error!("❌ 파일 존재 조회 SQL 오류: {}", e);
            StorageError::Database(format!("파일 존재 조회 SQL 오류: {}", e))
        })?;
        
        match row {
            Some((db_file_id, filename, file_path, is_deleted_raw, device_hash, account_hash)) => {
                let is_deleted_bool = is_deleted_raw == 1;
                info!("✅ 파일 DB 조회 결과:");
                info!("   file_id: {}", db_file_id);
                info!("   filename: {}", filename);
                info!("   file_path: {}", file_path);
                info!("   is_deleted (raw): {} (u8)", is_deleted_raw);
                info!("   is_deleted (converted): {} (bool)", is_deleted_bool);
                info!("   device_hash: {}", device_hash);
                info!("   account_hash: {}", account_hash);
                
                Ok((true, is_deleted_bool))
            },
            None => {
                warn!("⚠️ 파일이 데이터베이스에 존재하지 않음: file_id={}", file_id);
                Ok((false, false))
            }
        }
    }
}