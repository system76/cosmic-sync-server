use chrono::prelude::*;
// mysql_async fully migrated in this file; using sqlx
use tracing::{debug, error, info, warn};
use serde_json;

use crate::models::watcher::{WatcherGroup, WatcherCondition, ConditionType};
use crate::sync::{WatcherData, WatcherGroupData};
use crate::storage::{Result, StorageError};
use crate::storage::mysql::MySqlStorage;
use crate::utils::time;
use crate::utils::helpers;

/// MySQL 워처 관련 기능 확장 트레이트
#[allow(async_fn_in_trait)]
pub trait MySqlWatcherExt {
    /// 워처 조회
    async fn get_watcher(&self, watcher_id: i32) -> Result<WatcherData>;
    
    /// 워처 그룹 등록
    async fn register_watcher_group(&self, account_hash: &str, device_hash: &str, watcher_group: &WatcherGroup) -> Result<i32>;
    
    /// 워처 그룹 목록 조회
    async fn get_watcher_groups(&self, account_hash: &str) -> Result<Vec<WatcherGroup>>;
    
    /// 특정 워처 그룹 조회
    async fn get_user_watcher_group(&self, account_hash: &str, group_id: i32) -> Result<Option<WatcherGroup>>;
    
    /// 워처 그룹 업데이트
    async fn update_watcher_group(&self, account_hash: &str, watcher_group: &WatcherGroup) -> Result<()>;
    
    /// 워처 그룹 삭제
    async fn delete_watcher_group(&self, account_hash: &str, group_id: i32) -> Result<()>;
    
    /// 계정과 ID로 워처 그룹 조회 (프로토콜 버퍼 형식)
    async fn get_watcher_group_by_account_and_id(&self, account_hash: &str, group_id: i32) -> Result<Option<WatcherGroupData>>;
    
    /// 폴더 경로로 워처 찾기
    async fn find_watcher_by_folder(&self, account_hash: &str, group_id: i32, folder: &str) -> Result<Option<i32>>;
    
    /// 워처 생성 (conditions 포함)
    async fn create_watcher_with_conditions(&self, account_hash: &str, group_id: i32, watcher_data: &crate::sync::WatcherData, timestamp: i64) -> Result<i32>;
    
    /// 그룹 ID와 워처 ID로 워처 정보 조회
    async fn get_watcher_by_group_and_id(&self, account_hash: &str, group_id: i32, watcher_id: i32) -> Result<Option<WatcherData>>;
    
    /// 워처 프리셋 목록 조회
    async fn get_watcher_preset(&self, account_hash: &str) -> Result<Vec<String>>;
    
    /// 워처 프리셋 등록 (프로토콜 버퍼 형식)
    async fn register_watcher_preset_proto(&self, account_hash: &str, device_hash: &str, presets: Vec<String>) -> Result<()>;
    
    /// 워처 프리셋 업데이트 (프로토콜 버퍼 형식)
    async fn update_watcher_preset_proto(&self, account_hash: &str, device_hash: &str, presets: Vec<String>) -> Result<()>;
    
    // === Watcher Conditions Methods ===
    
    /// 워처 조건 생성
    async fn create_watcher_condition(&self, condition: &WatcherCondition) -> Result<i64>;
    
    /// 워처 조건 목록 조회
    async fn get_watcher_conditions(&self, account_hash: &str, watcher_id: i32) -> Result<Vec<WatcherCondition>>;
    
    /// 워처 조건 업데이트
    async fn update_watcher_condition(&self, condition: &WatcherCondition) -> Result<()>;
    
    /// 워처 조건 삭제
    async fn delete_watcher_condition(&self, condition_id: i64) -> Result<()>;
    
    /// 워처의 모든 조건 삭제
    async fn delete_all_watcher_conditions(&self, watcher_id: i32) -> Result<()>;
    
    /// 워처 조건 일괄 저장 (기존 조건 삭제 후 새로 저장)
    async fn save_watcher_conditions(&self, watcher_id: i32, conditions: &[WatcherCondition]) -> Result<()>;
    
    /// 클라이언트 group_id로 서버 group_id 조회
    async fn get_server_group_id(&self, account_hash: &str, client_group_id: i32) -> Result<Option<i32>>;
    
    /// 클라이언트 group_id와 watcher_id로 서버 IDs 조회
    async fn get_server_ids(&self, account_hash: &str, client_group_id: i32, client_watcher_id: i32) -> Result<Option<(i32, i32)>>;
}

impl MySqlWatcherExt for MySqlStorage {
    /// 워처 조회
    async fn get_watcher(&self, watcher_id: i32) -> Result<WatcherData> {
		use sqlx::Row;
		let row_opt = sqlx::query(
			r#"SELECT id, watcher_id, account_hash, folder, is_recursive FROM watchers WHERE id = ?"#,
		)
		.bind(watcher_id)
		.fetch_optional(self.get_sqlx_pool())
		.await
		.map_err(|e| StorageError::Database(format!("Failed to query watcher (sqlx): {}", e)))?;

		if let Some(row) = row_opt {
			let id: i32 = row.try_get("id").map_err(|e| StorageError::Database(format!("Row get id: {}", e)))?;
			let watcher_id_val: i32 = row.try_get("watcher_id").map_err(|e| StorageError::Database(format!("Row get watcher_id: {}", e)))?;
			let account_hash: String = row.try_get("account_hash").map_err(|e| StorageError::Database(format!("Row get account_hash: {}", e)))?;
			let folder: String = row.try_get("folder").map_err(|e| StorageError::Database(format!("Row get folder: {}", e)))?;
			let is_recursive: bool = row.try_get("is_recursive").map_err(|e| StorageError::Database(format!("Row get is_recursive: {}", e)))?;

			let conditions = self.get_watcher_conditions(&account_hash, id).await.unwrap_or_default();
			let mut union_conditions = Vec::new();
			let mut subtracting_conditions = Vec::new();
			for condition in conditions {
				let condition_data = crate::sync::ConditionData { key: condition.key, value: condition.value };
				match condition.condition_type {
					crate::models::watcher::ConditionType::Union => union_conditions.push(condition_data),
					crate::models::watcher::ConditionType::Subtract => subtracting_conditions.push(condition_data),
				}
			}

			let proto_watcher = WatcherData {
				watcher_id: watcher_id_val,
				folder,
				union_conditions,
				subtracting_conditions,
				recursive_path: is_recursive,
				preset: false,
				custom_type: "".to_string(),
				update_mode: "".to_string(),
				is_active: true,
				extra_json: "".to_string(),
			};
			Ok(proto_watcher)
		} else {
			Err(StorageError::NotFound(format!("Watcher with id {} not found", watcher_id)))
		}
    }
    
    /// 워처 그룹 등록
    async fn register_watcher_group(&self, account_hash: &str, device_hash: &str, watcher_group: &WatcherGroup) -> Result<i32> {
        // use sqlx::Acquire; // not needed
        // 날짜 형식은 DB에서 UNIX_TIMESTAMP로 처리하므로 문자열 변환 불필요
        let mut tx = self.get_sqlx_pool().begin().await
            .map_err(|e| { error!("Failed to start transaction: {}", e); StorageError::Database(format!("Failed to start transaction: {}", e)) })?;

        // 기존 그룹이 있는지 확인하고 시간 비교 (초 단위 비교)
        let existing_updated_ts: Option<i64> = sqlx::query_scalar(
            r#"SELECT UNIX_TIMESTAMP(updated_at) FROM watcher_groups WHERE account_hash = ? AND group_id = ?"#
        )
        .bind(account_hash)
        .bind(watcher_group.group_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| { error!("Failed to check existing watcher group: {}", e); StorageError::Database(format!("Failed to check existing watcher group: {}", e)) })?;

        if let Some(server_ts) = existing_updated_ts {
            if server_ts >= watcher_group.updated_at.timestamp() {
                info!("Server watcher group is newer or equal, skipping update");
                tx.commit().await.map_err(|e| StorageError::Database(format!("Failed to commit transaction: {}", e)))?;
                return Ok(watcher_group.group_id);
            }
        }

        // 기존 사용자 데이터 모두 삭제 (사용자별 하나의 설정만 허용)
        sqlx::query(r#"DELETE FROM watchers WHERE account_hash = ?"#)
            .bind(account_hash)
            .execute(&mut *tx)
            .await
            .map_err(|e| { error!("Failed to delete watchers: {}", e); StorageError::Database(format!("Failed to delete watchers: {}", e)) })?;

        sqlx::query(r#"DELETE FROM watcher_groups WHERE account_hash = ?"#)
            .bind(account_hash)
            .execute(&mut *tx)
            .await
            .map_err(|e| { error!("Failed to delete watcher_groups: {}", e); StorageError::Database(format!("Failed to delete watcher_groups: {}", e)) })?;

        let res = sqlx::query(r#"INSERT INTO watcher_groups (
                group_id, account_hash, device_hash, title, 
                created_at, updated_at, is_active
              ) VALUES (?, ?, ?, ?, FROM_UNIXTIME(?), FROM_UNIXTIME(?), ?)"#)
            .bind(watcher_group.group_id)
            .bind(account_hash)
            .bind(device_hash)
            .bind(&watcher_group.title)
            .bind(watcher_group.created_at.timestamp())
            .bind(watcher_group.updated_at.timestamp())
            .bind(watcher_group.is_active)
            .execute(&mut *tx)
            .await
            .map_err(|e| { error!("Failed to insert watcher group: {}", e); StorageError::Database(format!("Failed to insert watcher group: {}", e)) })?;

        let _last_id = res.last_insert_id();

        tx.commit().await.map_err(|e| StorageError::Database(format!("Failed to commit transaction: {}", e)))?;
        Ok(watcher_group.group_id)
    }
    
    /// 워처 그룹 목록 조회
    async fn get_watcher_groups(&self, account_hash: &str) -> Result<Vec<WatcherGroup>> {
        use sqlx::Row;
        let rows = sqlx::query(
            r#"SELECT id, group_id, account_hash, title,
                       UNIX_TIMESTAMP(created_at) AS created_ts,
                       UNIX_TIMESTAMP(updated_at) AS updated_ts,
                       is_active
                FROM watcher_groups
                WHERE account_hash = ?
                ORDER BY id"#,
        )
        .bind(account_hash)
        .fetch_all(self.get_sqlx_pool())
        .await
        .map_err(|e| StorageError::Database(format!("Failed to query watcher groups (sqlx): {}", e)))?;

        let mut result = Vec::with_capacity(rows.len());
        for row in rows {
            let id: i32 = row.try_get("id").map_err(|e| StorageError::Database(format!("Row get id: {}", e)))?;
            let group_id_val: i32 = row.try_get("group_id").map_err(|e| StorageError::Database(format!("Row get group_id: {}", e)))?;
            let acc_hash: String = row.try_get("account_hash").map_err(|e| StorageError::Database(format!("Row get account_hash: {}", e)))?;
            let title: String = row.try_get("title").map_err(|e| StorageError::Database(format!("Row get title: {}", e)))?;
            let created_ts: Option<i64> = row.try_get("created_ts").unwrap_or(None);
            let updated_ts: Option<i64> = row.try_get("updated_ts").unwrap_or(None);
            let is_active: bool = row.try_get("is_active").map_err(|e| StorageError::Database(format!("Row get is_active: {}", e)))?;

            let created_at = created_ts
                .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0))
                .unwrap_or_else(|| Utc::now());
            let updated_at = updated_ts
                .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0))
                .unwrap_or_else(|| Utc::now());

            let watcher_rows = sqlx::query(r#"SELECT id FROM watchers WHERE group_id = ? AND account_hash = ?"#)
                .bind(id)
                .bind(account_hash)
                .fetch_all(self.get_sqlx_pool())
                .await
                .map_err(|e| StorageError::Database(format!("Failed to query group watchers (sqlx): {}", e)))?;
            let mut watcher_ids: Vec<i32> = Vec::with_capacity(watcher_rows.len());
            for wr in watcher_rows {
                let wid: i32 = wr.try_get("id").map_err(|e| StorageError::Database(format!("Row get id: {}", e)))?;
                watcher_ids.push(wid);
            }

            let group = WatcherGroup {
                id,
                group_id: group_id_val,
                account_hash: acc_hash,
                title,
                created_at,
                updated_at,
                is_active,
                watcher_ids,
            };
            result.push(group);
        }

        Ok(result)
    }
    
    /// 특정 워처 그룹 조회
    async fn get_user_watcher_group(&self, account_hash: &str, group_id: i32) -> Result<Option<WatcherGroup>> {
        debug!("get_user_watcher_group called with account_hash={}, group_id={}", account_hash, group_id);

        use sqlx::Row;
        let row_opt = sqlx::query(
            r#"SELECT id, group_id, account_hash, title,
                       UNIX_TIMESTAMP(created_at) AS created_ts,
                       UNIX_TIMESTAMP(updated_at) AS updated_ts,
                       is_active
                FROM watcher_groups
                WHERE account_hash = ? AND group_id = ?"#,
        )
        .bind(account_hash)
        .bind(group_id)
        .fetch_optional(self.get_sqlx_pool())
        .await
        .map_err(|e| StorageError::Database(format!("Failed to query watcher group (sqlx): {}", e)))?;

        if let Some(row) = row_opt {
            let id: i32 = row.try_get("id").map_err(|e| StorageError::Database(format!("Row get id: {}", e)))?;
            let group_id_val: i32 = row.try_get("group_id").map_err(|e| StorageError::Database(format!("Row get group_id: {}", e)))?;
            let acc_hash: String = row.try_get("account_hash").map_err(|e| StorageError::Database(format!("Row get account_hash: {}", e)))?;
            let title: String = row.try_get("title").map_err(|e| StorageError::Database(format!("Row get title: {}", e)))?;
            let created_ts: Option<i64> = row.try_get("created_ts").unwrap_or(None);
            let updated_ts: Option<i64> = row.try_get("updated_ts").unwrap_or(None);
            let is_active: bool = row.try_get("is_active").map_err(|e| StorageError::Database(format!("Row get is_active: {}", e)))?;

            let created_at = created_ts
                .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0))
                .unwrap_or_else(|| Utc::now());
            let updated_at = updated_ts
                .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0))
                .unwrap_or_else(|| Utc::now());

            let watcher_rows = sqlx::query(r#"SELECT id FROM watchers WHERE group_id = ? AND account_hash = ?"#)
                .bind(id)
                .bind(account_hash)
                .fetch_all(self.get_sqlx_pool())
                .await
                .map_err(|e| StorageError::Database(format!("Failed to query group watchers (sqlx): {}", e)))?;
            let mut watcher_ids: Vec<i32> = Vec::with_capacity(watcher_rows.len());
            for wr in watcher_rows {
                let wid: i32 = wr.try_get("id").map_err(|e| StorageError::Database(format!("Row get id: {}", e)))?;
                watcher_ids.push(wid);
            }

            let group = WatcherGroup {
                id,
                group_id: group_id_val,
                account_hash: acc_hash,
                title,
                created_at,
                updated_at,
                is_active,
                watcher_ids,
            };

            Ok(Some(group))
        } else {
            Ok(None)
        }
    }
    
    /// 워처 그룹 업데이트
    async fn update_watcher_group(&self, account_hash: &str, watcher_group: &WatcherGroup) -> Result<()> {
        // 기존 그룹의 업데이트 시간 확인 (sqlx)
        debug!("Checking existing watcher group timestamp before update");
        let existing_updated_ts: Option<i64> = sqlx::query_scalar(
            r#"SELECT UNIX_TIMESTAMP(updated_at) FROM watcher_groups WHERE id = ? AND account_hash = ?"#
        )
        .bind(watcher_group.id)
        .bind(account_hash)
        .fetch_optional(self.get_sqlx_pool())
        .await
        .map_err(|e| StorageError::Database(format!("Failed to check existing watcher group timestamp: {}", e)))?;

        if let Some(server_ts) = existing_updated_ts {
            if server_ts >= watcher_group.updated_at.timestamp() {
                info!("Server watcher group is newer, skipping update");
                return Ok(());
            }
        }

        let mut tx = self.get_sqlx_pool().begin().await
            .map_err(|e| StorageError::Database(format!("Failed to start transaction: {}", e)))?;

        sqlx::query(r#"UPDATE watcher_groups SET 
                title = ?, 
                updated_at = FROM_UNIXTIME(?), 
                is_active = ?
              WHERE id = ? AND account_hash = ?"#)
            .bind(&watcher_group.title)
            .bind(watcher_group.updated_at.timestamp())
            .bind(watcher_group.is_active)
            .bind(watcher_group.id)
            .bind(account_hash)
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::Database(format!("Failed to update watcher group: {}", e)))?;

        tx.commit().await.map_err(|e| StorageError::Database(format!("Failed to commit transaction: {}", e)))?;
        Ok(())
    }
    
    /// 워처 그룹 삭제 (연관 워처/조건 포함 정리)
    async fn delete_watcher_group(&self, account_hash: &str, group_id: i32) -> Result<()> {
        // 트랜잭션 시작
        let mut tx = self.get_sqlx_pool().begin().await
            .map_err(|e| StorageError::Database(format!("Failed to start transaction: {}", e)))?;

        // 서버 그룹 ID 조회 (watchers는 서버 그룹 ID를 FK로 사용)
        let server_group_id_opt: Option<i32> = sqlx::query_scalar(
            r#"SELECT id FROM watcher_groups WHERE account_hash = ? AND group_id = ?"#
        )
        .bind(account_hash)
        .bind(group_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| StorageError::Database(format!("Failed to select watcher_group id: {}", e)))?;

        let server_group_id = match server_group_id_opt {
            Some(id) => id,
            None => {
                // 그룹이 이미 없으면 작업 없음
                tx.commit().await.map_err(|e| StorageError::Database(format!("Failed to commit transaction: {}", e)))?;
                return Ok(());
            }
        };

        // 관련 조건 삭제 (FK ON DELETE CASCADE가 있어도 안전하게 선제 정리)
        sqlx::query(r#"DELETE FROM watcher_conditions 
              WHERE watcher_id IN (
                SELECT id FROM watchers WHERE account_hash = ? AND group_id = ?
              )"#)
            .bind(account_hash)
            .bind(server_group_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::Database(format!("Failed to delete watcher conditions: {}", e)))?;

        // 관련 워처 삭제
        sqlx::query(r#"DELETE FROM watchers WHERE account_hash = ? AND group_id = ?"#)
            .bind(account_hash)
            .bind(server_group_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::Database(format!("Failed to delete watchers: {}", e)))?;

        // 그룹 삭제 (서버 그룹 ID 기준)
        sqlx::query(r#"DELETE FROM watcher_groups WHERE id = ? AND account_hash = ?"#)
            .bind(server_group_id)
            .bind(account_hash)
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::Database(format!("Failed to delete watcher group: {}", e)))?;
        
        tx.commit().await.map_err(|e| StorageError::Database(format!("Failed to commit transaction: {}", e)))?;
        Ok(())
    }
    
    /// 계정과 ID로 워처 그룹 조회 (프로토콜 버퍼 형식)
    async fn get_watcher_group_by_account_and_id(&self, account_hash: &str, group_id: i32) -> Result<Option<WatcherGroupData>> {
        // Use sqlx for this path (transition to sqlx) without compile-time macros
        use sqlx::Row;
        let row_opt = sqlx::query(
            r#"SELECT id, group_id, title, UNIX_TIMESTAMP(updated_at) AS updated_at_ts
               FROM watcher_groups WHERE account_hash = ? AND group_id = ?"#,
        )
        .bind(account_hash)
        .bind(group_id)
        .fetch_optional(self.get_sqlx_pool())
        .await
        .map_err(|e| StorageError::Database(format!("Failed to query watcher group (sqlx): {}", e)))?;

        if let Some(row) = row_opt {
            let id: i32 = row.try_get("id").map_err(|e| StorageError::Database(format!("Row get id: {}", e)))?;
            let group_id_val: i32 = row.try_get("group_id").map_err(|e| StorageError::Database(format!("Row get group_id: {}", e)))?;
            let title: String = row.try_get("title").map_err(|e| StorageError::Database(format!("Row get title: {}", e)))?;
            let updated_at_ts: Option<i64> = row.try_get("updated_at_ts").unwrap_or(None);
            let updated_at = updated_at_ts
                .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0))
                .unwrap_or_else(|| Utc::now());

            // Fetch watcher ids with sqlx
            let rows = sqlx::query(r#"SELECT id FROM watchers WHERE group_id = ? AND account_hash = ?"#)
                .bind(id)
                .bind(account_hash)
                .fetch_all(self.get_sqlx_pool())
                .await
                .map_err(|e| StorageError::Database(format!("Failed to query group watchers (sqlx): {}", e)))?;
            let mut watcher_ids: Vec<i32> = Vec::with_capacity(rows.len());
            for r in rows {
                let wid: i32 = r
                    .try_get("id")
                    .map_err(|e| StorageError::Database(format!("Row get id: {}", e)))?;
                watcher_ids.push(wid);
            }

            // Fetch each watcher via existing method (still mysql_async underneath)
            let mut watchers = Vec::with_capacity(watcher_ids.len());
            for watcher_id in watcher_ids {
                if let Ok(w) = self.get_watcher(watcher_id).await {
                    watchers.push(w);
                } else {
                    // continue on error
                }
            }

            let timestamp = time::datetime_to_timestamp(&updated_at);
            let group_data = WatcherGroupData {
                group_id: group_id_val,
                title,
                watchers,
                last_updated: Some(timestamp),
            };
            Ok(Some(group_data))
        } else {
            Ok(None)
        }
    }
    
    /// 워처 프리셋 목록 조회
    async fn get_watcher_preset(&self, account_hash: &str) -> Result<Vec<String>> {
        // 계정 해시로 프리셋 조회 (sqlx)
        let preset_json: Option<String> = sqlx::query_scalar(
            r#"SELECT preset_json FROM watcher_presets WHERE account_hash = ? ORDER BY updated_at DESC LIMIT 1"#
        )
        .bind(account_hash)
        .fetch_optional(self.get_sqlx_pool())
        .await
        .map_err(|e| StorageError::Database(format!("Failed to query watcher preset: {}", e)))?;
        
        match preset_json {
            Some(json) => {
                // JSON 문자열을 Vec<String>으로 역직렬화
                let presets: Vec<String> = serde_json::from_str(&json)
                    .map_err(|e| StorageError::General(format!("Failed to deserialize presets: {}", e)))?;
                
                Ok(presets)
            },
            None => Ok(Vec::new()) // 프리셋이 없으면 빈 벡터 반환
        }
    }
    
    /// 워처 프리셋 등록 (프로토콜 버퍼 형식)
    async fn register_watcher_preset_proto(&self, account_hash: &str, _device_hash: &str, presets: Vec<String>) -> Result<()> {
        info!("🔄 Registering watcher presets: account={}, presets_count={}", account_hash, presets.len());
        
        // use sqlx::Acquire; // not needed
        
        // 현재 시간 (초)
        let now = chrono::Utc::now().timestamp();
        
        // 계정이 존재하지 않으면 자동 생성 (Foreign Key 오류 방지)
        match sqlx::query(r#"INSERT IGNORE INTO accounts (account_hash, created_at, updated_at) 
              VALUES (?, ?, ?)"#)
            .bind(account_hash)
            .bind(now)
            .bind(now)
            .execute(self.get_sqlx_pool()).await {
            Ok(_) => {
                debug!("✅ Account ensured in database: {}", account_hash);
            },
            Err(e) => {
                warn!("⚠️ Failed to ensure account exists (continuing anyway): {}", e);
            }
        }
        
        // UPSERT 방식으로 프리셋 저장 (충돌 방지)
        let preset_json = serde_json::to_string(&presets)
            .map_err(|e| StorageError::General(format!("Failed to serialize presets: {}", e)))?;
        
        // ON DUPLICATE KEY UPDATE를 사용한 UPSERT
        match sqlx::query(r#"INSERT INTO watcher_presets (
                account_hash, preset_json, created_at, updated_at
              ) VALUES (?, ?, ?, ?)
              ON DUPLICATE KEY UPDATE 
                preset_json = VALUES(preset_json),
                updated_at = VALUES(updated_at)"#)
            .bind(account_hash)
            .bind(&preset_json)
            .bind(now)
            .bind(now)
            .execute(self.get_sqlx_pool()).await {
            Ok(_) => {
                info!("✅ Watcher presets registered successfully: account={}", account_hash);
                Ok(())
            },
            Err(e) => {
                error!("❌ Failed to register watcher presets: {}", e);
                Err(StorageError::Database(format!("Failed to register watcher presets: {}", e)))
            }
        }
    }
    
    /// 워처 프리셋 업데이트 (프로토콜 버퍼 형식)
    async fn update_watcher_preset_proto(&self, account_hash: &str, device_hash: &str, presets: Vec<String>) -> Result<()> {
        // 실질적으로 register_watcher_preset_proto와 동일한 동작
        self.register_watcher_preset_proto(account_hash, device_hash, presets).await
    }

    /// 폴더 경로로 워처 찾기
    async fn find_watcher_by_folder(&self, account_hash: &str, group_id: i32, folder: &str) -> Result<Option<i32>> {
        // Normalize folder path to preserve tilde (~) prefix for home directory
        let normalized_folder = helpers::normalize_path_preserve_tilde(folder);
        debug!("Finding watcher by folder: account={}, group_id={}, original_folder={}, normalized_folder={}", 
               account_hash, group_id, folder, normalized_folder);
        
		let row_opt = sqlx::query(r#"SELECT id FROM watchers WHERE account_hash = ? AND group_id = ? AND folder = ?"#)
			.bind(account_hash)
			.bind(group_id)
			.bind(&normalized_folder)
			.fetch_optional(self.get_sqlx_pool())
			.await
			.map_err(|e| {
				error!("Failed to query watcher by folder (sqlx): {}", e);
				StorageError::Database(format!("Failed to query watcher: {}", e))
			})?;

		if let Some(row) = row_opt {
			let id: i32 = sqlx::Row::try_get(&row, "id").map_err(|e| StorageError::Database(format!("Row get id: {}", e)))?;
			debug!("Found watcher with ID: {} for normalized folder: {}", id, normalized_folder);
			Ok(Some(id))
		} else {
			debug!("No watcher found for normalized folder: {}", normalized_folder);
			Ok(None)
		}
    }

    // removed: create_watcher without conditions (use create_watcher_with_conditions instead)

    /// 워처 생성 (conditions 포함)
    async fn create_watcher_with_conditions(&self, account_hash: &str, group_id: i32, watcher_data: &crate::sync::WatcherData, timestamp: i64) -> Result<i32> {
        // Normalize folder path to preserve tilde (~) prefix for home directory
        let normalized_folder = crate::utils::helpers::normalize_path_preserve_tilde(&watcher_data.folder);
        debug!("Creating new watcher with conditions: account={}, group_id={}, original_folder={}, normalized_folder={}, is_recursive={}", 
               account_hash, group_id, &watcher_data.folder, normalized_folder, watcher_data.recursive_path);
        
        // use sqlx::Acquire; // not needed

        // 워처 등록
        let folder_name = normalized_folder.split('/').last().unwrap_or("Watcher").to_string();
        let title = format!("Watcher for {}", folder_name);

        debug!("Inserting watcher with title: {}, watcher_id: {}", title, watcher_data.watcher_id);
        
        // 트랜잭션 시작 전에 watcher_group이 존재할 때까지 기다림 (race condition 해결)
        debug!("WATCHER_CREATE_DEBUG: Looking for watcher_group: group_id={}, account_hash={}", group_id, account_hash);
        
        let mut db_group_id: Option<i32> = None;
        for attempt in 1..=15 { // 더 많은 재시도 허용
            let group_result: Option<i32> = sqlx::query_scalar(
                r#"SELECT id FROM watcher_groups WHERE group_id = ? AND account_hash = ?"#
            )
            .bind(group_id)
            .bind(account_hash)
            .fetch_optional(self.get_sqlx_pool())
            .await
            .map_err(|e| { error!("Failed to execute watcher_groups query: {}", e); StorageError::Database(format!("Failed to get DB group ID: {}", e)) })?;
            
            if let Some(id) = group_result {
                debug!("Found watcher_group on attempt {}: id={}", attempt, id);
                db_group_id = Some(id);
                break;
            } else {
                warn!("Watcher group not found on attempt {}/15 for client group_id={}, waiting...", attempt, group_id);
                if attempt < 15 {
                    // 트랜잭션 밖에서 더 긴 시간 대기 가능
                    let sleep_ms = std::cmp::min(300 * attempt as u64, 2000); // 최대 2초
                    tokio::time::sleep(tokio::time::Duration::from_millis(sleep_ms)).await;
                }
            }
        }
        
        let db_group_id = match db_group_id {
            Some(id) => {
                info!("Found existing DB group ID: {}", id);
                id
            },
            None => {
                error!("Watcher group not found for client group_id: {} after 15 attempts. Groups must be created via register_watcher_group first.", group_id);
                return Err(StorageError::Database(format!("Watcher group with client group_id {} not found after waiting", group_id)));
            }
        };

        // 이제 watcher_group이 확실히 존재하므로 트랜잭션 시작
        let mut tx = self.get_sqlx_pool().begin().await
            .map_err(|e| { error!("Failed to start transaction: {}", e); StorageError::Database(format!("Failed to start transaction: {}", e)) })?;

        debug!("Proceeding with watcher creation for group ID: {}", db_group_id);

        // 기존 watcher가 있는지 확인하고 타임스탬프 비교 (local_group_id 포함)
        debug!("Checking for existing watcher with watcher_id: {}, account_hash: {}, local_group_id: {}", watcher_data.watcher_id, account_hash, group_id);
        let existing_watcher: Option<i64> = sqlx::query_scalar(
            r#"SELECT updated_at FROM watchers WHERE watcher_id = ? AND account_hash = ? AND local_group_id = ?"#
        )
        .bind(watcher_data.watcher_id)
        .bind(account_hash)
        .bind(group_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| { error!("Failed to check existing watcher: {}", e); StorageError::Database(format!("Failed to check existing watcher: {}", e)) })?;
        
        if let Some(existing_updated_at) = existing_watcher {
            let existing_datetime = chrono::DateTime::from_timestamp(existing_updated_at, 0).unwrap_or_else(|| chrono::Utc::now());
            let client_datetime = chrono::DateTime::from_timestamp(timestamp, 0).unwrap_or_else(|| chrono::Utc::now());
            
            // 서버의 기존 watcher가 클라이언트보다 새로우면 업데이트 스킵
            if existing_datetime >= client_datetime {
                info!("Server watcher is newer (server: {}, client: {}), skipping watcher creation", 
                      existing_datetime, client_datetime);
                
                // 기존 watcher ID 반환 (local_group_id 포함)
                let existing_id: Option<i32> = sqlx::query_scalar(
                    r#"SELECT id FROM watchers WHERE watcher_id = ? AND account_hash = ? AND local_group_id = ?"#
                )
                .bind(watcher_data.watcher_id)
                .bind(account_hash)
                .bind(group_id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(|e| { error!("Failed to get existing watcher ID: {}", e); StorageError::Database(format!("Failed to get existing watcher ID: {}", e)) })?;
                
                if let Some(existing_id) = existing_id {
                    debug!("Committing transaction (no changes made)");
                    tx.commit().await.map_err(|e| {
                        error!("Failed to commit transaction: {}", e);
                        StorageError::Database(format!("Failed to commit transaction: {}", e))
                    })?;
                    
                    debug!("Skipped watcher creation, returning existing ID: {}", existing_id);
                    return Ok(existing_id);
                }
            } else {
                info!("Client watcher is newer (server: {}, client: {}), proceeding with watcher update", 
                      existing_datetime, client_datetime);
                
                // 기존 watcher와 conditions 삭제 (local_group_id 포함)
                debug!("Deleting existing watcher and conditions with local_group_id: {}", group_id);
                
                // 기존 파일들을 orphaned 상태로 마킹 (워처 변경 시 파일 데이터 보호)
                sqlx::query(r#"UPDATE files SET watcher_id = 0 WHERE watcher_id = (SELECT id FROM watchers WHERE watcher_id = ? AND account_hash = ? AND local_group_id = ?)"#)
                    .bind(watcher_data.watcher_id)
                    .bind(account_hash)
                    .bind(group_id)
                    .execute(&mut *tx)
                    .await
                    .map_err(|e| { error!("Failed to mark files as orphaned: {}", e); StorageError::Database(format!("Failed to mark files as orphaned: {}", e)) })?;
                
                sqlx::query(r#"DELETE FROM watcher_conditions WHERE watcher_id = (SELECT id FROM watchers WHERE watcher_id = ? AND account_hash = ? AND local_group_id = ?)"#)
                    .bind(watcher_data.watcher_id)
                    .bind(account_hash)
                    .bind(group_id)
                    .execute(&mut *tx)
                    .await
                    .map_err(|e| { error!("Failed to delete existing watcher conditions: {}", e); StorageError::Database(format!("Failed to delete existing watcher conditions: {}", e)) })?;
                
                sqlx::query(r#"DELETE FROM watchers WHERE watcher_id = ? AND account_hash = ? AND local_group_id = ?"#)
                    .bind(watcher_data.watcher_id)
                    .bind(account_hash)
                    .bind(group_id)
                    .execute(&mut *tx)
                    .await
                    .map_err(|e| { error!("Failed to delete existing watcher: {}", e); StorageError::Database(format!("Failed to delete existing watcher: {}", e)) })?;
            }
        }

        // 워처 삽입 시도
        let result = sqlx::query(r#"INSERT INTO watchers (
                watcher_id, account_hash, group_id, local_group_id, folder, title,
                is_recursive, created_at, updated_at, 
                is_active, extra_json
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#)
            .bind(watcher_data.watcher_id)
            .bind(account_hash)
            .bind(db_group_id)
            .bind(group_id)
            .bind(&normalized_folder)
            .bind(&title)
            .bind(watcher_data.recursive_path)
            .bind(timestamp)
            .bind(timestamp)
            .bind(true)
            .bind(&watcher_data.extra_json)
            .execute(&mut *tx)
            .await;

        // 삽입에 실패한 경우 롤백 후 오류 반환
        if let Err(e) = result {
            error!("Failed to insert watcher: {}", e);
            if let Err(rollback_err) = tx.rollback().await {
                error!("Failed to rollback transaction: {}", rollback_err);
            }
            return Err(StorageError::Database(format!("Failed to insert watcher: {}", e)));
        }
        // 생성된 ID 조회 (executor result)
        let last_id_u64 = match &result {
            Ok(res) => res.last_insert_id(),
            Err(_) => 0,
        } as u64;
        let new_id: i32 = if last_id_u64 > i32::MAX as u64 { i32::MAX } else { last_id_u64 as i32 };

        if new_id == 0 {
            error!("Failed to create watcher: Invalid ID (0)");
            if let Err(rollback_err) = tx.rollback().await {
                error!("Failed to rollback transaction: {}", rollback_err);
            }
            return Err(StorageError::Database("Failed to create watcher: Invalid ID".to_string()));
        }

        // group_watchers 테이블 사용하지 않음 - watchers 테이블의 group_id로 직접 관리

        // conditions 저장
        use crate::models::watcher::{WatcherCondition, ConditionType};
        
        // union_conditions 저장
        for condition_data in &watcher_data.union_conditions {
            debug!("Saving union condition: {}={:?}", condition_data.key, condition_data.value);
            let condition = WatcherCondition {
                id: None,
                account_hash: account_hash.to_string(), // account_hash 추가
                watcher_id: new_id,                     // 서버 DB ID
                local_watcher_id: watcher_data.watcher_id, // 클라이언트 측 watcher ID
                local_group_id: group_id,               // 클라이언트 측 group ID
                condition_type: ConditionType::Union,
                key: condition_data.key.clone(),
                value: condition_data.value.clone(), // ConditionData.value는 이미 Vec<String>
                operator: "equals".to_string(), // 기본 연산자
                created_at: chrono::DateTime::from_timestamp(timestamp, 0).unwrap_or_else(|| chrono::Utc::now()),
                updated_at: chrono::DateTime::from_timestamp(timestamp, 0).unwrap_or_else(|| chrono::Utc::now()),
            };
            
            // value를 JSON 배열로 직렬화
            let value_json = serde_json::to_string(&condition.value).map_err(|e| {
                StorageError::Database(format!("Failed to serialize condition values: {}", e))
            })?;

            let result = sqlx::query(r#"INSERT INTO watcher_conditions (
                    account_hash, watcher_id, local_watcher_id, local_group_id, condition_type, `key`, value, operator, created_at, updated_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#)
                .bind(&condition.account_hash)
                .bind(condition.watcher_id)
                .bind(condition.local_watcher_id)
                .bind(condition.local_group_id)
                .bind(condition.condition_type.to_string())
                .bind(&condition.key)
                .bind(&value_json)
                .bind(&condition.operator)
                .bind(condition.created_at.timestamp())
                .bind(condition.updated_at.timestamp())
                .execute(&mut *tx)
                .await;

            if let Err(e) = result {
                error!("Failed to save union condition: {}", e);
                if let Err(rollback_err) = tx.rollback().await {
                    error!("Failed to rollback transaction: {}", rollback_err);
                }
                return Err(StorageError::Database(format!("Failed to save union condition: {}", e)));
            }
        }
        
        // subtracting_conditions 저장
        for condition_data in &watcher_data.subtracting_conditions {
            debug!("Saving subtracting condition: {}={:?}", condition_data.key, condition_data.value);
            let condition = WatcherCondition {
                id: None,
                account_hash: account_hash.to_string(), // account_hash 추가
                watcher_id: new_id,                     // 서버 DB ID
                local_watcher_id: watcher_data.watcher_id, // 클라이언트 측 watcher ID
                local_group_id: group_id,               // 클라이언트 측 group ID
                condition_type: ConditionType::Subtract,
                key: condition_data.key.clone(),
                value: condition_data.value.clone(), // ConditionData.value는 이미 Vec<String>
                operator: "equals".to_string(), // 기본 연산자
                created_at: chrono::DateTime::from_timestamp(timestamp, 0).unwrap_or_else(|| chrono::Utc::now()),
                updated_at: chrono::DateTime::from_timestamp(timestamp, 0).unwrap_or_else(|| chrono::Utc::now()),
            };
            
            // value를 JSON 배열로 직렬화
            let value_json = serde_json::to_string(&condition.value).map_err(|e| {
                StorageError::Database(format!("Failed to serialize condition values: {}", e))
            })?;

            let result = sqlx::query(r#"INSERT INTO watcher_conditions (
                    account_hash, watcher_id, local_watcher_id, local_group_id, condition_type, `key`, value, operator, created_at, updated_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#)
                .bind(&condition.account_hash)
                .bind(condition.watcher_id)
                .bind(condition.local_watcher_id)
                .bind(condition.local_group_id)
                .bind(condition.condition_type.to_string())
                .bind(&condition.key)
                .bind(&value_json)
                .bind(&condition.operator)
                .bind(condition.created_at.timestamp())
                .bind(condition.updated_at.timestamp())
                .execute(&mut *tx)
                .await;

            if let Err(e) = result {
                error!("Failed to save subtracting condition: {}", e);
                if let Err(rollback_err) = tx.rollback().await {
                    error!("Failed to rollback transaction: {}", rollback_err);
                }
                return Err(StorageError::Database(format!("Failed to save subtracting condition: {}", e)));
            }
        }

        debug!("Committing transaction for watcher creation");
        // 트랜잭션 커밋
        if let Err(e) = tx.commit().await {
            error!("Failed to commit transaction: {}", e);
            return Err(StorageError::Database(format!("Failed to commit transaction: {}", e)));
        }

        debug!("Created new watcher ID {} for normalized folder {} in group {}", new_id, normalized_folder, group_id);
        Ok(new_id)
    }
    
    /// 그룹 ID와 워처 ID로 워처 정보 조회
    async fn get_watcher_by_group_and_id(&self, account_hash: &str, group_id: i32, watcher_id: i32) -> Result<Option<WatcherData>> {
        debug!("Getting watcher by group and ID: account={}, group_id={}, watcher_id={}", 
               account_hash, group_id, watcher_id);
        
		use sqlx::Row;
		let row_opt = sqlx::query(
			r#"SELECT id, watcher_id, folder, is_recursive
		       FROM watchers 
		       WHERE account_hash = ? AND local_group_id = ? AND watcher_id = ?"#
		)
		.bind(account_hash)
		.bind(group_id)
		.bind(watcher_id)
		.fetch_optional(self.get_sqlx_pool())
		.await
		.map_err(|e| {
			error!("Failed to query watcher (sqlx): {}", e);
			StorageError::Database(format!("Failed to query watcher: {}", e))
		})?;

		if let Some(row) = row_opt {
			let id: i32 = row.try_get("id").map_err(|e| StorageError::Database(format!("Row get id: {}", e)))?;
			let watcher_id_val: i32 = row.try_get("watcher_id").map_err(|e| StorageError::Database(format!("Row get watcher_id: {}", e)))?;
			let folder: String = row.try_get("folder").map_err(|e| StorageError::Database(format!("Row get folder: {}", e)))?;
			let is_recursive: bool = row.try_get("is_recursive").map_err(|e| StorageError::Database(format!("Row get is_recursive: {}", e)))?;

			let conditions = self.get_watcher_conditions(account_hash, id).await.unwrap_or_default();
			let mut union_conditions = Vec::new();
			let mut subtracting_conditions = Vec::new();
			for condition in conditions {
				let condition_data = crate::sync::ConditionData { key: condition.key, value: condition.value };
				match condition.condition_type {
					crate::models::watcher::ConditionType::Union => union_conditions.push(condition_data),
					crate::models::watcher::ConditionType::Subtract => subtracting_conditions.push(condition_data),
				}
			}
			let proto_watcher = WatcherData {
				watcher_id: watcher_id_val,
				folder,
				union_conditions,
				subtracting_conditions,
				recursive_path: is_recursive,
				preset: false,
				custom_type: "".to_string(),
				update_mode: "".to_string(),
				is_active: true,
				extra_json: "".to_string(),
			};
			debug!("Found watcher: folder={}, recursive={}", proto_watcher.folder, proto_watcher.recursive_path);
			Ok(Some(proto_watcher))
		} else {
			debug!("Watcher not found: group_id={}, watcher_id={}", group_id, watcher_id);
			Ok(None)
		}
    }

    // === Watcher Conditions Methods ===
    
    /// 워처 조건 생성
    async fn create_watcher_condition(&self, condition: &WatcherCondition) -> Result<i64> {
		// use sqlx::Acquire; // not needed
		let actual_local_group_id = if condition.local_group_id == 0 {
			let watcher_id_opt: Option<i32> = sqlx::query_scalar(r#"SELECT watcher_id FROM watchers WHERE id = ?"#)
				.bind(condition.watcher_id)
				.fetch_optional(self.get_sqlx_pool())
				.await
				.map_err(|e| StorageError::Database(format!("Failed to get watcher_id for watcher (sqlx): {}", e)))?;
			match watcher_id_opt { Some(w) => w, None => { error!("Watcher with ID {} not found", condition.watcher_id); return Err(StorageError::NotFound(format!("Watcher with ID {} not found", condition.watcher_id))); } }
		} else { condition.local_group_id };

		let value_json = serde_json::to_string(&condition.value)
			.map_err(|e| StorageError::Database(format!("Failed to serialize condition values: {}", e)))?;

		let mut tx = self.get_sqlx_pool().begin().await
			.map_err(|e| StorageError::Database(format!("Failed to start transaction (sqlx): {}", e)))?;

		let res = sqlx::query(
			r#"INSERT INTO watcher_conditions (
					account_hash, watcher_id, local_watcher_id, local_group_id,
					condition_type, `key`, value, operator, created_at, updated_at
				) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#
		)
		.bind(&condition.account_hash)
		.bind(condition.watcher_id)
		.bind(condition.local_watcher_id)
		.bind(actual_local_group_id)
		.bind(condition.condition_type.to_string())
		.bind(&condition.key)
		.bind(&value_json)
		.bind(&condition.operator)
		.bind(condition.created_at.timestamp())
		.bind(condition.updated_at.timestamp())
		.execute(&mut *tx)
		.await
		.map_err(|e| { StorageError::Database(format!("Failed to insert watcher condition (sqlx): {}", e)) })?;

		let new_id = res.last_insert_id() as i64;
		if new_id == 0 { let _ = tx.rollback().await; return Err(StorageError::Database("Failed to create watcher condition: Invalid ID".to_string())); }
		tx.commit().await.map_err(|e| StorageError::Database(format!("Failed to commit transaction (sqlx): {}", e)))?;
		debug!("Created new watcher condition ID {}", new_id);
		Ok(new_id)
    }
    
    /// 워처 조건 목록 조회
    async fn get_watcher_conditions(&self, account_hash: &str, watcher_id: i32) -> Result<Vec<WatcherCondition>> {
		use sqlx::Row;
		let rows = sqlx::query(
			r#"SELECT id, account_hash, watcher_id, local_watcher_id, local_group_id,
		              condition_type, `key`, value, operator, created_at, updated_at
		       FROM watcher_conditions
		       WHERE account_hash = ? AND watcher_id = ?
		       ORDER BY id"#
		)
		.bind(account_hash)
		.bind(watcher_id)
		.fetch_all(self.get_sqlx_pool())
		.await
		.map_err(|e| StorageError::Database(format!("Failed to query watcher conditions (sqlx): {}", e)))?;

		let mut result = Vec::with_capacity(rows.len());
		for row in rows {
			let id: i64 = row.try_get("id").map_err(|e| StorageError::Database(format!("Row get id: {}", e)))?;
			let db_account_hash: String = row.try_get("account_hash").map_err(|e| StorageError::Database(format!("Row get account_hash: {}", e)))?;
			let db_watcher_id: i32 = row.try_get("watcher_id").map_err(|e| StorageError::Database(format!("Row get watcher_id: {}", e)))?;
			let local_watcher_id: i32 = row.try_get("local_watcher_id").unwrap_or(0);
			let local_group_id: i32 = row.try_get("local_group_id").unwrap_or(0);
			let condition_type_str: String = row.try_get("condition_type").map_err(|e| StorageError::Database(format!("Row get condition_type: {}", e)))?;
			let key: String = row.try_get("key").map_err(|e| StorageError::Database(format!("Row get key: {}", e)))?;
			let value_json: String = row.try_get("value").map_err(|e| StorageError::Database(format!("Row get value: {}", e)))?;
			let operator: String = row.try_get("operator").map_err(|e| StorageError::Database(format!("Row get operator: {}", e)))?;
			let created_at: i64 = row.try_get("created_at").unwrap_or(0);
			let updated_at: i64 = row.try_get("updated_at").unwrap_or(0);

			let condition_type = condition_type_str.parse::<ConditionType>()
				.map_err(|e| StorageError::Database(format!("Invalid condition type: {}", e)))?;
			let value: Vec<String> = serde_json::from_str(&value_json)
				.map_err(|e| StorageError::Database(format!("Failed to deserialize condition values: {}", e)))?;
			let created_at_dt = chrono::DateTime::from_timestamp(created_at, 0).unwrap_or_else(|| chrono::Utc::now());
			let updated_at_dt = chrono::DateTime::from_timestamp(updated_at, 0).unwrap_or_else(|| chrono::Utc::now());

			result.push(WatcherCondition {
				id: Some(id),
				account_hash: db_account_hash,
				watcher_id: db_watcher_id,
				local_watcher_id,
				local_group_id,
				condition_type,
				key,
				value,
				operator,
				created_at: created_at_dt,
				updated_at: updated_at_dt,
			});
		}
		Ok(result)
    }
    
    /// 워처 조건 업데이트
    async fn update_watcher_condition(&self, condition: &WatcherCondition) -> Result<()> {
        // use sqlx::Acquire; // not needed
        let condition_id = condition.id.ok_or_else(|| {
            StorageError::ValidationError("Condition ID is required for update".to_string())
        })?;

        // 트랜잭션 시작(sqlx)
        let mut tx = self.get_sqlx_pool().begin().await
            .map_err(|e| StorageError::Database(format!("Failed to start transaction: {}", e)))?;

        // value를 JSON 배열로 직렬화
        let value_json = serde_json::to_string(&condition.value).map_err(|e| {
            StorageError::Database(format!("Failed to serialize condition values: {}", e))
        })?;

        // 워처 조건 업데이트(sqlx)
        let result = sqlx::query(r#"UPDATE watcher_conditions SET
                local_watcher_id = ?,
                local_group_id = ?,
                condition_type = ?,
                `key` = ?,
                value = ?,
                operator = ?,
                updated_at = ?
              WHERE id = ?"#)
            .bind(condition.local_watcher_id)
            .bind(condition.local_group_id)
            .bind(condition.condition_type.to_string())
            .bind(&condition.key)
            .bind(&value_json)
            .bind(&condition.operator)
            .bind(condition.updated_at.timestamp())
            .bind(condition_id)
            .execute(&mut *tx)
            .await;

        // 업데이트에 실패한 경우 롤백 후 오류 반환
        if let Err(e) = result {
            error!("Failed to update watcher condition: {}", e);
            if let Err(rollback_err) = tx.rollback().await {
                error!("Failed to rollback transaction: {}", rollback_err);
            }
            return Err(StorageError::Database(format!("Failed to update watcher condition: {}", e)));
        }

        // 트랜잭션 커밋
        if let Err(e) = tx.commit().await {
            error!("Failed to commit transaction: {}", e);
            return Err(StorageError::Database(format!("Failed to commit transaction: {}", e)));
        }

        Ok(())
    }
    
    /// 워처 조건 삭제
    async fn delete_watcher_condition(&self, condition_id: i64) -> Result<()> {
		sqlx::query(r#"DELETE FROM watcher_conditions WHERE id = ?"#)
			.bind(condition_id)
			.execute(self.get_sqlx_pool())
			.await
			.map_err(|e| {
				error!("Failed to delete watcher condition (sqlx): {}", e);
				StorageError::Database(format!("Failed to delete watcher condition: {}", e))
			})?;
		Ok(())
    }
    
    /// 워처의 모든 조건 삭제
    async fn delete_all_watcher_conditions(&self, watcher_id: i32) -> Result<()> {
		sqlx::query(r#"DELETE FROM watcher_conditions WHERE watcher_id = ?"#)
			.bind(watcher_id)
			.execute(self.get_sqlx_pool())
			.await
			.map_err(|e| {
				error!("Failed to delete all watcher conditions (sqlx): {}", e);
				StorageError::Database(format!("Failed to delete all watcher conditions: {}", e))
			})?;
		Ok(())
    }
    
    /// 워처 조건 일괄 저장 (기존 조건 삭제 후 새로 저장)
    async fn save_watcher_conditions(&self, watcher_id: i32, conditions: &[WatcherCondition]) -> Result<()> {
		let mut tx = self.get_sqlx_pool().begin().await
			.map_err(|e| StorageError::Database(format!("Failed to start transaction (sqlx): {}", e)))?;

		sqlx::query(r#"DELETE FROM watcher_conditions WHERE watcher_id = ?"#)
			.bind(watcher_id)
			.execute(&mut *tx)
			.await
			.map_err(|e| StorageError::Database(format!("Failed to delete existing conditions (sqlx): {}", e)))?;

		for condition in conditions {
			let value_json = serde_json::to_string(&condition.value)
				.map_err(|e| StorageError::Database(format!("Failed to serialize condition values: {}", e)))?;

			sqlx::query(
				r#"INSERT INTO watcher_conditions (
						account_hash, watcher_id, local_group_id, condition_type,
						`key`, value, operator, created_at, updated_at
					) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)"#
			)
			.bind(&condition.account_hash)
			.bind(condition.watcher_id)
			.bind(condition.local_group_id)
			.bind(condition.condition_type.to_string())
			.bind(&condition.key)
			.bind(&value_json)
			.bind(&condition.operator)
			.bind(condition.created_at.timestamp())
			.bind(condition.updated_at.timestamp())
			.execute(&mut *tx)
			.await
			.map_err(|e| StorageError::Database(format!("Failed to insert watcher condition (sqlx): {}", e)))?;
		}

		tx.commit().await.map_err(|e| StorageError::Database(format!("Failed to commit transaction (sqlx): {}", e)))?;
		Ok(())
      }
    
    /// 클라이언트 group_id로 서버 group_id 조회
    async fn get_server_group_id(&self, account_hash: &str, client_group_id: i32) -> Result<Option<i32>> {
		debug!("Getting server group ID for client_group_id={}, account_hash={}", client_group_id, account_hash);
		let id_opt: Option<i32> = sqlx::query_scalar(r#"SELECT id FROM watcher_groups WHERE account_hash = ? AND group_id = ?"#)
			.bind(account_hash)
			.bind(client_group_id)
			.fetch_optional(self.get_sqlx_pool())
			.await
			.map_err(|e| { error!("Failed to get server group ID (sqlx): {}", e); StorageError::Database(format!("Failed to get server group ID: {}", e)) })?;
		Ok(id_opt)
    }
    
    /// 클라이언트 group_id와 watcher_id로 서버 IDs 조회
    async fn get_server_ids(&self, account_hash: &str, client_group_id: i32, client_watcher_id: i32) -> Result<Option<(i32, i32)>> {
        debug!("Getting server IDs for client_group_id={}, client_watcher_id={}, account_hash={}", 
               client_group_id, client_watcher_id, account_hash);

        let group_id_opt: Option<i32> = sqlx::query_scalar(
            r#"SELECT id FROM watcher_groups WHERE account_hash = ? AND group_id = ?"#)
            .bind(account_hash)
            .bind(client_group_id)
            .fetch_optional(self.get_sqlx_pool())
            .await
            .map_err(|e| { error!("Failed to get server group ID (sqlx): {}", e); StorageError::Database(format!("Failed to get server group ID: {}", e)) })?;

        if let Some(group_id) = group_id_opt {
            let watcher_id_opt: Option<i32> = sqlx::query_scalar(
                r#"SELECT id FROM watchers WHERE account_hash = ? AND local_group_id = ? AND watcher_id = ?"#)
                .bind(account_hash)
                .bind(client_group_id)
                .bind(client_watcher_id)
                .fetch_optional(self.get_sqlx_pool())
                .await
                .map_err(|e| { error!("Failed to get server watcher ID (sqlx): {}", e); StorageError::Database(format!("Failed to get server watcher ID: {}", e)) })?;

            if let Some(watcher_id) = watcher_id_opt {
                debug!("Found server IDs: group_id={}, watcher_id={}", group_id, watcher_id);
                Ok(Some((group_id, watcher_id)))
            } else {
                debug!("Watcher not found for client_watcher_id={}", client_watcher_id);
                Ok(None)
            }
        } else {
            debug!("Group not found for client_group_id={}", client_group_id);
            Ok(None)
        }
    }
}
