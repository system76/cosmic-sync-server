use chrono::prelude::*;
use mysql_async::{prelude::*, TxOpts};
use tracing::{debug, error, info, warn};
use serde_json;

use crate::models::watcher::{WatcherGroup, WatcherPreset, WatcherCondition, ConditionType};
use crate::sync::{DeviceInfo, WatcherData, WatcherGroupData};
use crate::storage::{Result, StorageError};
use crate::storage::mysql::MySqlStorage;
use crate::utils::time;
use crate::utils::helpers;

/// MySQL 워처 관련 기능 확장 트레이트
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
    
    /// 워처 생성
    async fn create_watcher(&self, account_hash: &str, group_id: i32, folder: &str, is_recursive: bool, timestamp: i64) -> Result<i32>;
    
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
        let pool = self.get_pool();
        let mut conn = pool.get_conn().await.map_err(|e| {
            StorageError::Database(format!("Failed to get connection: {}", e))
        })?;

        // 직접 watchers 테이블에서 조회
        let watcher: Option<(i32, i32, String, i32, i32, String, Option<String>, i32, bool)> = conn.exec_first(
            r"SELECT 
                id, watcher_id, account_hash, group_id, local_group_id, folder, pattern, interval_seconds, is_recursive
            FROM watchers 
            WHERE id = ?",
            (watcher_id,)
        ).await.map_err(|e| {
            StorageError::Database(e.to_string())
        })?;

        match watcher {
            Some((id, watcher_id_val, account_hash, _group_id, _local_group_id, folder, _pattern, _interval, is_recursive)) => {
                // 워처 조건들 조회
                let conditions = self.get_watcher_conditions(&account_hash, id).await.unwrap_or_default();
                
                // union과 subtract 조건 분리
                let mut union_conditions = Vec::new();
                let mut subtracting_conditions = Vec::new();
                
                for condition in conditions {
                    let condition_data = crate::sync::ConditionData {
                        key: condition.key,
                        value: condition.value,
                    };
                    
                    match condition.condition_type {
                        crate::models::watcher::ConditionType::Union => {
                            union_conditions.push(condition_data);
                        },
                        crate::models::watcher::ConditionType::Subtract => {
                            subtracting_conditions.push(condition_data);
                        }
                    }
                }
                
                let proto_watcher = WatcherData {
                    watcher_id: watcher_id_val, // 클라이언트에게는 watcher_id를 반환
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
            },
            None => Err(StorageError::NotFound(format!("Watcher with id {} not found", watcher_id)))
        }
    }
    
    /// 워처 그룹 등록
    async fn register_watcher_group(&self, account_hash: &str, device_hash: &str, watcher_group: &WatcherGroup) -> Result<i32> {
        
        let pool = self.get_pool();
        let mut conn = pool.get_conn().await.map_err(|e| {
            error!("Failed to get connection: {}", e);
            StorageError::Database(format!("Failed to get connection: {}", e))
        })?;
        
        // 날짜 형식 변환
        let created_at = time::datetime_to_mysql_string(&watcher_group.created_at);
        let updated_at = time::datetime_to_mysql_string(&watcher_group.updated_at);
        
        // SERIALIZABLE isolation level로 세션 설정 후 트랜잭션 시작
        conn.query_drop("SET SESSION TRANSACTION ISOLATION LEVEL SERIALIZABLE").await.map_err(|e| {
            error!("Failed to set session isolation level: {}", e);
            StorageError::Database(format!("Failed to set session isolation level: {}", e))
        })?;
        
        let mut tx = conn.start_transaction(TxOpts::default()).await.map_err(|e| {
            error!("Failed to start transaction: {}", e);
            StorageError::Database(format!("Failed to start transaction: {}", e))
        })?;
        
        // 기존 그룹이 있는지 확인하고 시간 비교
        
        let existing_group: Option<(String,)> = tx.exec_first(
            r"SELECT DATE_FORMAT(updated_at, '%Y-%m-%d %H:%i:%s') as updated_at_str
              FROM watcher_groups 
              WHERE account_hash = ? AND group_id = ?",
            (account_hash, watcher_group.group_id)
        ).await.map_err(|e| {
            error!("Failed to check existing watcher group: {}", e);
            StorageError::Database(format!("Failed to check existing watcher group: {}", e))
        })?;
        
        if let Some((existing_updated_at_str,)) = existing_group {
            // 서버의 기존 업데이트 시간 파싱
            if let Ok(existing_updated_at) = chrono::NaiveDateTime::parse_from_str(&existing_updated_at_str, "%Y-%m-%d %H:%M:%S") {
                let existing_updated_at_utc = chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(existing_updated_at, chrono::Utc);
                
                // 클라이언트의 업데이트 시간과 비교
                if existing_updated_at_utc >= watcher_group.updated_at {
                    info!("Server watcher group is newer (server: {}, client: {}), skipping update", 
                          existing_updated_at_utc, watcher_group.updated_at);
                    
                    // 트랜잭션 커밋 (변경사항 없음)
                    tx.commit().await.map_err(|e| {
                        error!("Failed to commit transaction: {}", e);
                        StorageError::Database(format!("Failed to commit transaction: {}", e))
                    })?;
                    
                    return Ok(watcher_group.group_id);
                }
            }
        }
        
        // 기존 사용자 데이터 모두 삭제 (사용자별 하나의 설정만 허용)
        
        // 1. watchers 삭제 (외래 키 제약 때문에 먼저 삭제)
        tx.exec_drop(
            "DELETE FROM watchers WHERE account_hash = ?",
            (account_hash,)
        ).await.map_err(|e| {
            error!("Failed to delete watchers: {}", e);
            StorageError::Database(format!("Failed to delete watchers: {}", e))
        })?;

        // 2. watcher_groups 삭제
        tx.exec_drop(
            "DELETE FROM watcher_groups WHERE account_hash = ?",
            (account_hash,)
        ).await.map_err(|e| {
            error!("Failed to delete watcher_groups: {}", e);
            StorageError::Database(format!("Failed to delete watcher_groups: {}", e))
        })?;
        

        tx.exec_drop(
            r"INSERT INTO watcher_groups (
                group_id, account_hash, device_hash, title, 
                created_at, updated_at, is_active
              ) VALUES (?, ?, ?, ?, ?, ?, ?)",
            (
                watcher_group.group_id,
                account_hash,
                device_hash,
                &watcher_group.title,
                &created_at,
                &updated_at,
                watcher_group.is_active,
            ),
        ).await.map_err(|e| {
            error!("Failed to insert watcher group: {}", e);
            StorageError::Database(format!("Failed to insert watcher group: {}", e))
        })?;

        // MySQL에서 마지막 삽입 ID 가져오기
        let group_id: i32 = tx.exec_first("SELECT LAST_INSERT_ID()", ())
            .await
            .map_err(|e| {
                error!("Failed to get last insert ID: {}", e);
                StorageError::Database(format!("Failed to get last insert ID: {}", e))
            })?
            .unwrap_or(0);
        
        // 트랜잭션 커밋
        tx.commit().await.map_err(|e| {
            error!("Failed to commit transaction: {}", e);
            StorageError::Database(format!("Failed to commit transaction: {}", e))
        })?;
        Ok(watcher_group.group_id) // 클라이언트 group_id를 반환해야 함
    }
    
    /// 워처 그룹 목록 조회
    async fn get_watcher_groups(&self, account_hash: &str) -> Result<Vec<WatcherGroup>> {
        let pool = self.get_pool();
        let mut conn = pool.get_conn().await.map_err(|e| {
            StorageError::Database(format!("Failed to get connection: {}", e))
        })?;
        
        // 계정 해시로 워처 그룹 목록 조회 - 날짜 형식을 문자열로 명시적 변환
        let groups_data: Vec<(i32, i32, String, String, String, String, bool)> = conn.exec(
            r"SELECT 
                id, group_id, account_hash, title, 
                DATE_FORMAT(created_at, '%Y-%m-%d %H:%i:%s') as created_at_str, 
                DATE_FORMAT(updated_at, '%Y-%m-%d %H:%i:%s') as updated_at_str, 
                is_active
              FROM watcher_groups 
              WHERE account_hash = ?
              ORDER BY id",
            (account_hash,)
        ).await.map_err(|e| {
            StorageError::Database(format!("Failed to query watcher groups: {}", e))
        })?;
        
        let mut result = Vec::with_capacity(groups_data.len());
        
        for (id, group_id_val, acc_hash, title, created_at_str, updated_at_str, is_active) in groups_data {
            // 날짜 문자열을 DateTime으로 변환
            let created_at = match NaiveDateTime::parse_from_str(&created_at_str, "%Y-%m-%d %H:%M:%S") {
                Ok(dt) => DateTime::<Utc>::from_naive_utc_and_offset(dt, Utc),
                Err(_) => Utc::now() // 오류 시 현재 시간으로 대체
            };
            
            let updated_at = match NaiveDateTime::parse_from_str(&updated_at_str, "%Y-%m-%d %H:%M:%S") {
                Ok(dt) => DateTime::<Utc>::from_naive_utc_and_offset(dt, Utc),
                Err(_) => Utc::now()
            };
            
            // 그룹에 연결된 워처 ID 목록 가져오기 (watchers 테이블에서 직접 조회)
            let watcher_ids: Vec<i32> = conn.exec(
                r"SELECT id 
                  FROM watchers 
                  WHERE group_id = ? AND account_hash = ?",
                (id, account_hash)
            ).await.map_err(|e| {
                StorageError::Database(format!("Failed to query group watchers: {}", e))
            })?;
            
            // WatcherGroup 객체 생성하여 결과에 추가
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

        let pool = self.get_pool();
        let mut conn = pool.get_conn().await.map_err(|e| {
            StorageError::Database(format!("Failed to get connection: {}", e))
        })?;
        
        // 계정 해시와 클라이언트 그룹 ID로 워처 그룹 조회 - 날짜 형식을 문자열로 명시적 변환
        let group_data: Option<(i32, i32, String, String, String, String, bool)> = conn.exec_first(
            r"SELECT 
                id, group_id, account_hash, title, 
                DATE_FORMAT(created_at, '%Y-%m-%d %H:%i:%s') as created_at_str, 
                DATE_FORMAT(updated_at, '%Y-%m-%d %H:%i:%s') as updated_at_str, 
                is_active
              FROM watcher_groups 
              WHERE account_hash = ? AND group_id = ?",
            (account_hash, group_id)
        ).await.map_err(|e| {
            StorageError::Database(format!("Failed to query watcher group: {}", e))
        })?;
        
        debug!("Query result: {:?}", group_data);

        if let Some((id, group_id_val, acc_hash, title, created_at_str, updated_at_str, is_active)) = group_data {
            // 날짜 문자열을 DateTime으로 변환
            let created_at = match NaiveDateTime::parse_from_str(&created_at_str, "%Y-%m-%d %H:%M:%S") {
                Ok(dt) => DateTime::<Utc>::from_naive_utc_and_offset(dt, Utc),
                Err(_) => Utc::now() // 오류 시 현재 시간으로 대체
            };
            
            let updated_at = match NaiveDateTime::parse_from_str(&updated_at_str, "%Y-%m-%d %H:%M:%S") {
                Ok(dt) => DateTime::<Utc>::from_naive_utc_and_offset(dt, Utc),
                Err(_) => Utc::now()
            };
            
            // 그룹에 연결된 워처 ID 목록 가져오기 (watchers 테이블에서 직접 조회)
            let watcher_ids: Vec<i32> = conn.exec(
                r"SELECT id 
                  FROM watchers 
                  WHERE group_id = ? AND account_hash = ?",
                (id, account_hash)
            ).await.map_err(|e| {
                StorageError::Database(format!("Failed to query group watchers: {}", e))
            })?;
            
            // WatcherGroup 객체 생성
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
        let pool = self.get_pool();
        let mut conn = pool.get_conn().await.map_err(|e| {
            StorageError::Database(format!("Failed to get connection: {}", e))
        })?;
        
        // 기존 그룹의 업데이트 시간 확인
        debug!("Checking existing watcher group timestamp before update");
        let existing_updated_at: Option<(String,)> = conn.exec_first(
            r"SELECT DATE_FORMAT(updated_at, '%Y-%m-%d %H:%i:%s') as updated_at_str
              FROM watcher_groups 
              WHERE id = ? AND account_hash = ?",
            (watcher_group.id, account_hash)
        ).await.map_err(|e| {
            StorageError::Database(format!("Failed to check existing watcher group timestamp: {}", e))
        })?;
        
        if let Some((existing_updated_at_str,)) = existing_updated_at {
            if let Ok(existing_updated_at) = chrono::NaiveDateTime::parse_from_str(&existing_updated_at_str, "%Y-%m-%d %H:%M:%S") {
                let existing_updated_at_utc = chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(existing_updated_at, chrono::Utc);
                
                // 서버 시간이 클라이언트보다 새로우면 업데이트 스킵
                if existing_updated_at_utc >= watcher_group.updated_at {
                    info!("Server watcher group is newer (server: {}, client: {}), skipping update", 
                          existing_updated_at_utc, watcher_group.updated_at);
                    return Ok(());
                } else {
                    info!("Client watcher group is newer (server: {}, client: {}), proceeding with update", 
                          existing_updated_at_utc, watcher_group.updated_at);
                }
            }
        }
        
        // 날짜 형식 변환
        let updated_at = time::datetime_to_mysql_string(&watcher_group.updated_at);
        
        // 트랜잭션 시작
        let mut tx = conn.start_transaction(TxOpts::default()).await.map_err(|e| {
            StorageError::Database(format!("Failed to start transaction: {}", e))
        })?;
        
        // 워처 그룹 업데이트
        tx.exec_drop(
            r"UPDATE watcher_groups SET 
                title = ?, 
                updated_at = ?, 
                is_active = ?
              WHERE id = ? AND account_hash = ?",
            (
                &watcher_group.title,
                &updated_at,
                watcher_group.is_active,
                watcher_group.id,
                account_hash,
            ),
        ).await.map_err(|e| {
            StorageError::Database(format!("Failed to update watcher group: {}", e))
        })?;
        
        // group_watchers 테이블 사용하지 않음 - watchers 테이블로 직접 관리
        
        // 트랜잭션 커밋
        tx.commit().await.map_err(|e| {
            StorageError::Database(format!("Failed to commit transaction: {}", e))
        })?;
        
        Ok(())
    }
    
    /// 워처 그룹 삭제
    async fn delete_watcher_group(&self, account_hash: &str, group_id: i32) -> Result<()> {
        let pool = self.get_pool();
        let mut conn = pool.get_conn().await.map_err(|e| {
            StorageError::Database(format!("Failed to get connection: {}", e))
        })?;
        
        // account_hash와 클라이언트 group_id를 모두 확인하여 삭제
        conn.exec_drop(
            "DELETE FROM watcher_groups WHERE group_id = ? AND account_hash = ?",
            (group_id, account_hash)
        ).await.map_err(|e| {
            StorageError::Database(format!("Failed to delete watcher group: {}", e))
        })?;
        
        Ok(())
    }
    
    /// 계정과 ID로 워처 그룹 조회 (프로토콜 버퍼 형식)
    async fn get_watcher_group_by_account_and_id(&self, account_hash: &str, group_id: i32) -> Result<Option<WatcherGroupData>> {
        let pool = self.get_pool();
        let mut conn = pool.get_conn().await.map_err(|e| {
            StorageError::Database(format!("Failed to get connection: {}", e))
        })?;
        
        // 계정 해시와 클라이언트 그룹 ID로 워처 그룹 조회 - 날짜 형식을 문자열로 명시적 변환
        let group_data: Option<(i32, i32, String, String)> = conn.exec_first(
            r"SELECT 
                id, group_id, title, DATE_FORMAT(updated_at, '%Y-%m-%d %H:%i:%s') as updated_at_str
              FROM watcher_groups 
              WHERE account_hash = ? AND group_id = ?",
            (account_hash, group_id)
        ).await.map_err(|e| {
            StorageError::Database(format!("Failed to query watcher group: {}", e))
        })?;
        
        if let Some((id, group_id_val, title, updated_at_str)) = group_data {
            // 날짜 문자열을 DateTime으로 변환
            let updated_at = match NaiveDateTime::parse_from_str(&updated_at_str, "%Y-%m-%d %H:%M:%S") {
                Ok(dt) => DateTime::<Utc>::from_naive_utc_and_offset(dt, Utc),
                Err(_) => Utc::now() // 오류 시 현재 시간으로 대체
            };
            
            // 그룹에 연결된 워처 ID 목록 가져오기 (watchers 테이블에서 직접 조회)
            let watcher_ids: Vec<i32> = conn.exec(
                r"SELECT id 
                  FROM watchers 
                  WHERE group_id = ? AND account_hash = ?",
                (id, account_hash)
            ).await.map_err(|e| {
                StorageError::Database(format!("Failed to query group watchers: {}", e))
            })?;
            
            // 각 워처의 정보 가져오기
            let mut watchers = Vec::with_capacity(watcher_ids.len());
            for watcher_id in watcher_ids {
                match self.get_watcher(watcher_id).await {
                    Ok(watcher) => watchers.push(watcher),
                    Err(e) => {
                        error!("Failed to get watcher {}: {}", watcher_id, e);
                        // 오류가 있어도 계속 진행
                    }
                }
            }
            
            // 타임스탬프 생성
            let timestamp = time::datetime_to_timestamp(&updated_at);
            
            // WatcherGroupData 프로토 객체 생성
            let group_data = WatcherGroupData {
                group_id: group_id_val, // 클라이언트에게는 group_id를 반환
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
        let pool = self.get_pool();
        let mut conn = pool.get_conn().await.map_err(|e| {
            StorageError::Database(format!("Failed to get connection: {}", e))
        })?;
        
        // 계정 해시로 프리셋 조회
        let preset_json: Option<String> = conn.exec_first(
            "SELECT preset_json FROM watcher_presets WHERE account_hash = ? ORDER BY updated_at DESC LIMIT 1",
            (account_hash,)
        ).await.map_err(|e| {
            StorageError::Database(format!("Failed to query watcher preset: {}", e))
        })?;
        
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
        
        let pool = self.get_pool();
        let mut conn = pool.get_conn().await.map_err(|e| {
            error!("❌ Failed to get database connection: {}", e);
            StorageError::Database(format!("Failed to get connection: {}", e))
        })?;
        
        // 현재 시간 (초)
        let now = chrono::Utc::now().timestamp();
        
        // 계정이 존재하지 않으면 자동 생성 (Foreign Key 오류 방지)
        match conn.exec_drop(
            r"INSERT IGNORE INTO accounts (account_hash, created_at, updated_at) 
              VALUES (?, ?, ?)",
            (account_hash, now, now)
        ).await {
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
        match conn.exec_drop(
            r"INSERT INTO watcher_presets (
                account_hash, preset_json, created_at, updated_at
              ) VALUES (?, ?, ?, ?)
              ON DUPLICATE KEY UPDATE 
                preset_json = VALUES(preset_json),
                updated_at = VALUES(updated_at)",
            (
                account_hash,
                &preset_json,
                now,
                now,
            ),
        ).await {
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
        
        let pool = self.get_pool();
        let mut conn = pool.get_conn().await.map_err(|e| {
            error!("Failed to get connection: {}", e);
            StorageError::Database(format!("Failed to get connection: {}", e))
        })?;

        let result: Option<(i32,)> = conn.exec_first(
            r"SELECT id FROM watchers WHERE account_hash = ? AND group_id = ? AND folder = ?",
            (account_hash, group_id, &normalized_folder)
        ).await.map_err(|e| {
            error!("Failed to query watcher by folder: {}", e);
            StorageError::Database(format!("Failed to query watcher: {}", e))
        })?;

        match result {
            Some((id,)) => {
                debug!("Found watcher with ID: {} for normalized folder: {}", id, normalized_folder);
                Ok(Some(id))
            },
            None => {
                debug!("No watcher found for normalized folder: {}", normalized_folder);
                Ok(None)
            },
        }
    }

    /// 워처 생성
    async fn create_watcher(&self, account_hash: &str, group_id: i32, folder: &str, is_recursive: bool, timestamp: i64) -> Result<i32> {
        // Normalize folder path to preserve tilde (~) prefix for home directory
        let normalized_folder = helpers::normalize_path_preserve_tilde(folder);
        debug!("Creating new watcher: account={}, group_id={}, original_folder={}, normalized_folder={}, is_recursive={}", 
               account_hash, group_id, folder, normalized_folder, is_recursive);
        
        let pool = self.get_pool();
        let mut conn = pool.get_conn().await.map_err(|e| {
            error!("Failed to get connection: {}", e);
            StorageError::Database(format!("Failed to get connection: {}", e))
        })?;

        // SERIALIZABLE isolation level로 세션 설정 후 트랜잭션 시작
        conn.query_drop("SET SESSION TRANSACTION ISOLATION LEVEL SERIALIZABLE").await.map_err(|e| {
            error!("Failed to set session isolation level: {}", e);
            StorageError::Database(format!("Failed to set session isolation level: {}", e))
        })?;
        
        let mut tx = conn.start_transaction(TxOpts::default()).await.map_err(|e| {
            error!("Failed to start transaction: {}", e);
            StorageError::Database(format!("Failed to start transaction: {}", e))
        })?;

        // 워처 등록
        let folder_name = normalized_folder.split('/').last().unwrap_or("Watcher").to_string();
        let title = format!("Watcher for {}", folder_name);

        // 클라이언트 group_id로부터 서버 DB의 watcher_groups.id를 가져옴 (watchers 테이블 FK 제약조건 때문)
        let db_group_id: Option<(i32,)> = tx.exec_first(
            "SELECT id FROM watcher_groups WHERE group_id = ? AND account_hash = ?",
            (group_id, account_hash)
        ).await.map_err(|e| {
            StorageError::Database(format!("Failed to get DB group ID: {}", e))
        })?;
        
        let db_group_id = match db_group_id {
            Some((id,)) => id,
            None => {
                error!("Watcher group not found for client group_id: {}", group_id);
                if let Err(rollback_err) = tx.rollback().await {
                    error!("Failed to rollback transaction: {}", rollback_err);
                }
                return Err(StorageError::NotFound(format!("Watcher group {} not found", group_id)));
            }
        };

        // 워처 삽입 전에 watcher_group이 정말로 존재하는지 최종 확인
        debug!("Final verification: Checking if watcher_group {} really exists before creating watcher", db_group_id);
        let final_check: Option<(i32,)> = tx.exec_first(
            "SELECT id FROM watcher_groups WHERE id = ? AND account_hash = ?",
            (db_group_id, account_hash)
        ).await.map_err(|e| {
            error!("Failed to verify watcher_group existence: {}", e);
            StorageError::Database(format!("Failed to verify watcher_group existence: {}", e))
        })?;
        
        if final_check.is_none() {
            error!("Critical error: watcher_group {} disappeared before watcher creation", db_group_id);
            if let Err(rollback_err) = tx.rollback().await {
                error!("Failed to rollback transaction: {}", rollback_err);
            }
            return Err(StorageError::Database(format!("Watcher group {} disappeared before watcher creation", db_group_id)));
        }
        
        debug!("Watcher group {} confirmed to exist, proceeding with watcher creation", db_group_id);

        debug!("Inserting watcher with title: {} (no watcher_id available)", title);
        // 워처 삽입 시도 - 이 메서드는 watcher_id를 알 수 없으므로 0으로 설정 (deprecated 예정)
        let result = tx.exec_drop(
            r"INSERT INTO watchers (
                watcher_id, account_hash, group_id, local_group_id, folder, title,
                is_recursive, created_at, updated_at, 
                is_active, extra_json
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            (
                0, // watcher_id를 알 수 없으므로 0으로 설정
                account_hash,
                db_group_id,  // 서버 DB group ID (FK 제약조건)
                group_id,     // 클라이언트 측 local_group_id (동기화용)
                &normalized_folder,  // 정규화된 경로 사용
                &title,
                is_recursive,
                timestamp,
                timestamp,
                true,  // is_active 기본값
                "{}"   // extra_json 기본값
            ),
        ).await;

        // 삽입에 실패한 경우 롤백 후 오류 반환
        if let Err(e) = result {
            error!("Failed to insert watcher: {}", e);
            if let Err(rollback_err) = tx.rollback().await {
                error!("Failed to rollback transaction: {}", rollback_err);
            }
            return Err(StorageError::Database(format!("Failed to insert watcher: {}", e)));
        }

        // 생성된 ID 조회
        let id_result = tx.exec_first::<(i32,), _, _>(
            "SELECT LAST_INSERT_ID()",
            ()
        ).await;

        let new_id = match id_result {
            Ok(Some((id,))) => {
                debug!("Got last insert ID: {}", id);
                id
            },
            Ok(None) => {
                error!("Failed to get last insert ID: No ID returned");
                if let Err(rollback_err) = tx.rollback().await {
                    error!("Failed to rollback transaction: {}", rollback_err);
                }
                return Err(StorageError::Database("Failed to create watcher: No ID returned".to_string()));
            },
            Err(e) => {
                error!("Failed to get last insert ID: {}", e);
                if let Err(rollback_err) = tx.rollback().await {
                    error!("Failed to rollback transaction: {}", rollback_err);
                }
                return Err(StorageError::Database(format!("Failed to get last insert ID: {}", e)));
            }
        };

        if new_id == 0 {
            error!("Failed to create watcher: Invalid ID (0)");
            if let Err(rollback_err) = tx.rollback().await {
                error!("Failed to rollback transaction: {}", rollback_err);
            }
            return Err(StorageError::Database("Failed to create watcher: Invalid ID".to_string()));
        }

        // group_watchers 테이블 사용하지 않음 - watchers 테이블의 group_id로 직접 관리

        debug!("Committing transaction for watcher creation");
        // 트랜잭션 커밋
        if let Err(e) = tx.commit().await {
            error!("Failed to commit transaction: {}", e);
            return Err(StorageError::Database(format!("Failed to commit transaction: {}", e)));
        }

        debug!("Created new watcher ID {} for normalized folder {} in group {}", new_id, normalized_folder, group_id);
        Ok(new_id)
    }

    /// 워처 생성 (conditions 포함)
    async fn create_watcher_with_conditions(&self, account_hash: &str, group_id: i32, watcher_data: &crate::sync::WatcherData, timestamp: i64) -> Result<i32> {
        // Normalize folder path to preserve tilde (~) prefix for home directory
        let normalized_folder = crate::utils::helpers::normalize_path_preserve_tilde(&watcher_data.folder);
        debug!("Creating new watcher with conditions: account={}, group_id={}, original_folder={}, normalized_folder={}, is_recursive={}", 
               account_hash, group_id, &watcher_data.folder, normalized_folder, watcher_data.recursive_path);
        
        let pool = self.get_pool();
        let mut conn = pool.get_conn().await.map_err(|e| {
            error!("Failed to get connection: {}", e);
            StorageError::Database(format!("Failed to get connection: {}", e))
        })?;

        // 워처 등록
        let folder_name = normalized_folder.split('/').last().unwrap_or("Watcher").to_string();
        let title = format!("Watcher for {}", folder_name);

        debug!("Inserting watcher with title: {}, watcher_id: {}", title, watcher_data.watcher_id);
        
        // 트랜잭션 시작 전에 watcher_group이 존재할 때까지 기다림 (race condition 해결)
        debug!("WATCHER_CREATE_DEBUG: Looking for watcher_group: group_id={}, account_hash={}", group_id, account_hash);
        
        let mut db_group_id: Option<i32> = None;
        for attempt in 1..=15 { // 더 많은 재시도 허용
            let group_result: Option<(i32,)> = conn.exec_first(
                "SELECT id FROM watcher_groups WHERE group_id = ? AND account_hash = ?",
                (group_id, account_hash)
            ).await.map_err(|e| {
                error!("Failed to execute watcher_groups query: {}", e);
                StorageError::Database(format!("Failed to get DB group ID: {}", e))
            })?;
            
            if let Some((id,)) = group_result {
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
        conn.query_drop("SET SESSION TRANSACTION ISOLATION LEVEL SERIALIZABLE").await.map_err(|e| {
            error!("Failed to set session isolation level: {}", e);
            StorageError::Database(format!("Failed to set session isolation level: {}", e))
        })?;
        
        let mut tx = conn.start_transaction(TxOpts::default()).await.map_err(|e| {
            error!("Failed to start transaction: {}", e);
            StorageError::Database(format!("Failed to start transaction: {}", e))
        })?;

        debug!("Proceeding with watcher creation for group ID: {}", db_group_id);

        // 기존 watcher가 있는지 확인하고 타임스탬프 비교 (local_group_id 포함)
        debug!("Checking for existing watcher with watcher_id: {}, account_hash: {}, local_group_id: {}", watcher_data.watcher_id, account_hash, group_id);
        let existing_watcher: Option<(i64,)> = tx.exec_first(
            "SELECT updated_at FROM watchers WHERE watcher_id = ? AND account_hash = ? AND local_group_id = ?",
            (watcher_data.watcher_id, account_hash, group_id)
        ).await.map_err(|e| {
            error!("Failed to check existing watcher: {}", e);
            StorageError::Database(format!("Failed to check existing watcher: {}", e))
        })?;
        
        if let Some((existing_updated_at,)) = existing_watcher {
            let existing_datetime = chrono::DateTime::from_timestamp(existing_updated_at, 0).unwrap_or_else(|| chrono::Utc::now());
            let client_datetime = chrono::DateTime::from_timestamp(timestamp, 0).unwrap_or_else(|| chrono::Utc::now());
            
            // 서버의 기존 watcher가 클라이언트보다 새로우면 업데이트 스킵
            if existing_datetime >= client_datetime {
                info!("Server watcher is newer (server: {}, client: {}), skipping watcher creation", 
                      existing_datetime, client_datetime);
                
                // 기존 watcher ID 반환 (local_group_id 포함)
                let existing_id: Option<(i32,)> = tx.exec_first(
                    "SELECT id FROM watchers WHERE watcher_id = ? AND account_hash = ? AND local_group_id = ?",
                    (watcher_data.watcher_id, account_hash, group_id)
                ).await.map_err(|e| {
                    error!("Failed to get existing watcher ID: {}", e);
                    StorageError::Database(format!("Failed to get existing watcher ID: {}", e))
                })?;
                
                if let Some((existing_id,)) = existing_id {
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
                tx.exec_drop(
                    "UPDATE files SET watcher_id = 0 WHERE watcher_id = (SELECT id FROM watchers WHERE watcher_id = ? AND account_hash = ? AND local_group_id = ?)",
                    (watcher_data.watcher_id, account_hash, group_id)
                ).await.map_err(|e| {
                    error!("Failed to mark files as orphaned: {}", e);
                    StorageError::Database(format!("Failed to mark files as orphaned: {}", e))
                })?;
                
                tx.exec_drop(
                    "DELETE FROM watcher_conditions WHERE watcher_id = (SELECT id FROM watchers WHERE watcher_id = ? AND account_hash = ? AND local_group_id = ?)",
                    (watcher_data.watcher_id, account_hash, group_id)
                ).await.map_err(|e| {
                    error!("Failed to delete existing watcher conditions: {}", e);
                    StorageError::Database(format!("Failed to delete existing watcher conditions: {}", e))
                })?;
                
                tx.exec_drop(
                    "DELETE FROM watchers WHERE watcher_id = ? AND account_hash = ? AND local_group_id = ?",
                    (watcher_data.watcher_id, account_hash, group_id)
                ).await.map_err(|e| {
                    error!("Failed to delete existing watcher: {}", e);
                    StorageError::Database(format!("Failed to delete existing watcher: {}", e))
                })?;
            }
        }

        // 워처 삽입 시도
        let result = tx.exec_drop(
            r"INSERT INTO watchers (
                watcher_id, account_hash, group_id, local_group_id, folder, title,
                is_recursive, created_at, updated_at, 
                is_active, extra_json
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            (
                watcher_data.watcher_id, // 클라이언트 워처 ID
                account_hash,
                db_group_id,  // 서버 DB group ID (FK 제약조건)
                group_id,     // 클라이언트 측 local_group_id (동기화용)
                &normalized_folder,  // 정규화된 경로 사용
                &title,
                watcher_data.recursive_path,
                timestamp,
                timestamp,
                true,  // is_active 기본값
                &watcher_data.extra_json   // extra_json
            ),
        ).await;

        // 삽입에 실패한 경우 롤백 후 오류 반환
        if let Err(e) = result {
            error!("Failed to insert watcher: {}", e);
            if let Err(rollback_err) = tx.rollback().await {
                error!("Failed to rollback transaction: {}", rollback_err);
            }
            return Err(StorageError::Database(format!("Failed to insert watcher: {}", e)));
        }

        // 생성된 ID 조회
        let id_result = tx.exec_first::<(i32,), _, _>(
            "SELECT LAST_INSERT_ID()",
            ()
        ).await;

        let new_id = match id_result {
            Ok(Some((id,))) => {
                debug!("Got last insert ID: {}", id);
                id
            },
            Ok(None) => {
                error!("Failed to get last insert ID: No ID returned");
                if let Err(rollback_err) = tx.rollback().await {
                    error!("Failed to rollback transaction: {}", rollback_err);
                }
                return Err(StorageError::Database("Failed to create watcher: No ID returned".to_string()));
            },
            Err(e) => {
                error!("Failed to get last insert ID: {}", e);
                if let Err(rollback_err) = tx.rollback().await {
                    error!("Failed to rollback transaction: {}", rollback_err);
                }
                return Err(StorageError::Database(format!("Failed to get last insert ID: {}", e)));
            }
        };

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

            let result = tx.exec_drop(
                r"INSERT INTO watcher_conditions (
                    account_hash, watcher_id, local_watcher_id, local_group_id, condition_type, `key`, value, operator, created_at, updated_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                (
                    &condition.account_hash,
                    condition.watcher_id,
                    condition.local_watcher_id,
                    condition.local_group_id,
                    condition.condition_type.to_string(),
                    &condition.key,
                    &value_json,
                    &condition.operator,
                    condition.created_at.timestamp(),
                    condition.updated_at.timestamp(),
                ),
            ).await;

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

            let result = tx.exec_drop(
                r"INSERT INTO watcher_conditions (
                    account_hash, watcher_id, local_watcher_id, local_group_id, condition_type, `key`, value, operator, created_at, updated_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                (
                    &condition.account_hash,
                    condition.watcher_id,
                    condition.local_watcher_id,
                    condition.local_group_id,
                    condition.condition_type.to_string(),
                    &condition.key,
                    &value_json,
                    &condition.operator,
                    condition.created_at.timestamp(),
                    condition.updated_at.timestamp(),
                ),
            ).await;

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
        
        let pool = self.get_pool();
        let mut conn = pool.get_conn().await.map_err(|e| {
            error!("Failed to get connection: {}", e);
            StorageError::Database(format!("Failed to get connection: {}", e))
        })?;

        // 워처 정보 조회 (local_group_id와 watcher_id로 검색)
        let watcher: Option<(i32, i32, String, i32, i32, String, bool)> = conn.exec_first(
            r"SELECT id, watcher_id, account_hash, group_id, local_group_id, folder, is_recursive 
              FROM watchers 
              WHERE account_hash = ? AND local_group_id = ? AND watcher_id = ?",
            (account_hash, group_id, watcher_id)
        ).await.map_err(|e| {
            error!("Failed to query watcher: {}", e);
            StorageError::Database(format!("Failed to query watcher: {}", e))
        })?;

        match watcher {
            Some((id, watcher_id_val, _account_hash, _db_group_id, _local_group_id, folder, is_recursive)) => {
                // 워처 조건들 조회
                let conditions = self.get_watcher_conditions(account_hash, id).await.unwrap_or_default();
                
                // union과 subtract 조건 분리
                let mut union_conditions = Vec::new();
                let mut subtracting_conditions = Vec::new();
                
                for condition in conditions {
                    let condition_data = crate::sync::ConditionData {
                        key: condition.key,
                        value: condition.value,
                    };
                    
                    match condition.condition_type {
                        crate::models::watcher::ConditionType::Union => {
                            union_conditions.push(condition_data);
                        },
                        crate::models::watcher::ConditionType::Subtract => {
                            subtracting_conditions.push(condition_data);
                        }
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
            },
            None => {
                debug!("Watcher not found: group_id={}, watcher_id={}", group_id, watcher_id);
                Ok(None)
            }
        }
    }

    // === Watcher Conditions Methods ===
    
    /// 워처 조건 생성
    async fn create_watcher_condition(&self, condition: &WatcherCondition) -> Result<i64> {
        let pool = self.get_pool();
        let mut conn = pool.get_conn().await.map_err(|e| {
            StorageError::Database(format!("Failed to get connection: {}", e))
        })?;

        // 트랜잭션 시작
        let mut tx = conn.start_transaction(TxOpts::default()).await.map_err(|e| {
            StorageError::Database(format!("Failed to start transaction: {}", e))
        })?;

        // watcher_id로부터 local_group_id를 조회하여 설정 (안전장치)
        let actual_local_group_id = if condition.local_group_id == 0 {
            let group_id_result: Option<(i32,)> = tx.exec_first(
                "SELECT watcher_id FROM watchers WHERE id = ?",
                (condition.watcher_id,)
            ).await.map_err(|e| {
                StorageError::Database(format!("Failed to get watcher_id for watcher: {}", e))
            })?;
            
            match group_id_result {
                Some((watcher_id,)) => watcher_id,
                None => {
                    error!("Watcher with ID {} not found", condition.watcher_id);
                    return Err(StorageError::NotFound(format!("Watcher with ID {} not found", condition.watcher_id)));
                }
            }
        } else {
            condition.local_group_id
        };

        // value를 JSON 배열로 직렬화
        let value_json = serde_json::to_string(&condition.value).map_err(|e| {
            StorageError::Database(format!("Failed to serialize condition values: {}", e))
        })?;

        // 워처 조건 삽입
        let result = tx.exec_drop(
            r"INSERT INTO watcher_conditions (
                account_hash, watcher_id, local_watcher_id, local_group_id, condition_type, `key`, value, operator, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            (
                &condition.account_hash,
                condition.watcher_id,
                condition.local_watcher_id,
                condition.local_group_id,
                condition.condition_type.to_string(),
                &condition.key,
                &value_json,
                &condition.operator,
                condition.created_at.timestamp(),
                condition.updated_at.timestamp(),
            ),
        ).await;

        // 삽입에 실패한 경우 롤백 후 오류 반환
        if let Err(e) = result {
            error!("Failed to insert watcher condition: {}", e);
            if let Err(rollback_err) = tx.rollback().await {
                error!("Failed to rollback transaction: {}", rollback_err);
            }
            return Err(StorageError::Database(format!("Failed to insert watcher condition: {}", e)));
        }

        // 생성된 ID 조회
        let id_result = tx.exec_first::<(i64,), _, _>(
            "SELECT LAST_INSERT_ID()",
            ()
        ).await;

        let new_id = match id_result {
            Ok(Some((id,))) => id,
            Ok(None) => {
                error!("Failed to get last insert ID: No ID returned");
                if let Err(rollback_err) = tx.rollback().await {
                    error!("Failed to rollback transaction: {}", rollback_err);
                }
                return Err(StorageError::Database("Failed to create watcher condition: No ID returned".to_string()));
            },
            Err(e) => {
                error!("Failed to get last insert ID: {}", e);
                if let Err(rollback_err) = tx.rollback().await {
                    error!("Failed to rollback transaction: {}", rollback_err);
                }
                return Err(StorageError::Database(format!("Failed to get last insert ID: {}", e)));
            }
        };

        if new_id == 0 {
            error!("Failed to create watcher condition: Invalid ID (0)");
            if let Err(rollback_err) = tx.rollback().await {
                error!("Failed to rollback transaction: {}", rollback_err);
            }
            return Err(StorageError::Database("Failed to create watcher condition: Invalid ID".to_string()));
        }

        // 트랜잭션 커밋
        if let Err(e) = tx.commit().await {
            error!("Failed to commit transaction: {}", e);
            return Err(StorageError::Database(format!("Failed to commit transaction: {}", e)));
        }

        debug!("Created new watcher condition ID {}", new_id);
        Ok(new_id)
    }
    
    /// 워처 조건 목록 조회
    async fn get_watcher_conditions(&self, account_hash: &str, watcher_id: i32) -> Result<Vec<WatcherCondition>> {
        let pool = self.get_pool();
        let mut conn = pool.get_conn().await.map_err(|e| {
            StorageError::Database(format!("Failed to get connection: {}", e))
        })?;

        // 계정 해시와 워처 ID로 조건 목록 조회 (보안상 필요)
        let conditions: Vec<(i64, String, i32, i32, i32, String, String, String, String, i64, i64)> = conn.exec(
            r"SELECT id, account_hash, watcher_id, local_watcher_id, local_group_id, condition_type, `key`, value, operator, created_at, updated_at
              FROM watcher_conditions
              WHERE account_hash = ? AND watcher_id = ?
              ORDER BY id",
            (account_hash, watcher_id)
        ).await.map_err(|e| {
            StorageError::Database(format!("Failed to query watcher conditions: {}", e))
        })?;

        let mut result = Vec::with_capacity(conditions.len());
        for (id, db_account_hash, db_watcher_id, local_watcher_id, local_group_id, condition_type_str, key, value_json, operator, created_at, updated_at) in conditions {
            let condition_type = condition_type_str.parse::<ConditionType>()
                .map_err(|e| StorageError::Database(format!("Invalid condition type: {}", e)))?;
            
            // JSON 배열을 Vec<String>으로 역직렬화
            let value: Vec<String> = serde_json::from_str(&value_json).map_err(|e| {
                StorageError::Database(format!("Failed to deserialize condition values: {}", e))
            })?;
            
            let created_at_dt = chrono::DateTime::from_timestamp(created_at, 0)
                .unwrap_or_else(|| chrono::Utc::now());
            let updated_at_dt = chrono::DateTime::from_timestamp(updated_at, 0)
                .unwrap_or_else(|| chrono::Utc::now());
            
            let condition = WatcherCondition {
                id: Some(id),
                account_hash: db_account_hash,
                watcher_id: db_watcher_id,
                local_watcher_id: local_watcher_id,
                local_group_id: local_group_id,
                condition_type,
                key,
                value,
                operator,
                created_at: created_at_dt,
                updated_at: updated_at_dt,
            };
            result.push(condition);
        }
        Ok(result)
    }
    
    /// 워처 조건 업데이트
    async fn update_watcher_condition(&self, condition: &WatcherCondition) -> Result<()> {
        let pool = self.get_pool();
        let mut conn = pool.get_conn().await.map_err(|e| {
            StorageError::Database(format!("Failed to get connection: {}", e))
        })?;

        let condition_id = condition.id.ok_or_else(|| {
            StorageError::ValidationError("Condition ID is required for update".to_string())
        })?;

        // 트랜잭션 시작
        let mut tx = conn.start_transaction(TxOpts::default()).await.map_err(|e| {
            StorageError::Database(format!("Failed to start transaction: {}", e))
        })?;

        // value를 JSON 배열로 직렬화
        let value_json = serde_json::to_string(&condition.value).map_err(|e| {
            StorageError::Database(format!("Failed to serialize condition values: {}", e))
        })?;

        // 워처 조건 업데이트
        let result = tx.exec_drop(
            r"UPDATE watcher_conditions SET
                local_watcher_id = ?,
                local_group_id = ?,
                condition_type = ?,
                `key` = ?,
                value = ?,
                operator = ?,
                updated_at = ?
              WHERE id = ?",
            (
                condition.local_watcher_id,
                condition.local_group_id,
                condition.condition_type.to_string(),
                &condition.key,
                &value_json,
                &condition.operator,
                condition.updated_at.timestamp(),
                condition_id,
            ),
        ).await;

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
        let pool = self.get_pool();
        let mut conn = pool.get_conn().await.map_err(|e| {
            StorageError::Database(format!("Failed to get connection: {}", e))
        })?;

        // 워처 조건 삭제
        let result = conn.exec_drop(
            "DELETE FROM watcher_conditions WHERE id = ?",
            (condition_id,)
        ).await;

        // 삭제에 실패한 경우 오류 반환
        if let Err(e) = result {
            error!("Failed to delete watcher condition: {}", e);
            return Err(StorageError::Database(format!("Failed to delete watcher condition: {}", e)));
        }

        Ok(())
    }
    
    /// 워처의 모든 조건 삭제
    async fn delete_all_watcher_conditions(&self, watcher_id: i32) -> Result<()> {
        let pool = self.get_pool();
        let mut conn = pool.get_conn().await.map_err(|e| {
            StorageError::Database(format!("Failed to get connection: {}", e))
        })?;

        // 워처 ID로 모든 조건 삭제
        let result = conn.exec_drop(
            "DELETE FROM watcher_conditions WHERE watcher_id = ?",
            (watcher_id,)
        ).await;

        // 삭제에 실패한 경우 오류 반환
        if let Err(e) = result {
            error!("Failed to delete all watcher conditions: {}", e);
            return Err(StorageError::Database(format!("Failed to delete all watcher conditions: {}", e)));
        }

        Ok(())
    }
    
    /// 워처 조건 일괄 저장 (기존 조건 삭제 후 새로 저장)
    async fn save_watcher_conditions(&self, watcher_id: i32, conditions: &[WatcherCondition]) -> Result<()> {
        let pool = self.get_pool();
        let mut conn = pool.get_conn().await.map_err(|e| {
            StorageError::Database(format!("Failed to get connection: {}", e))
        })?;

        // 트랜잭션 시작
        let mut tx = conn.start_transaction(TxOpts::default()).await.map_err(|e| {
            StorageError::Database(format!("Failed to start transaction: {}", e))
        })?;

        // 기존 조건 삭제
        tx.exec_drop(
            "DELETE FROM watcher_conditions WHERE watcher_id = ?",
            (watcher_id,)
        ).await.map_err(|e| {
            StorageError::Database(format!("Failed to delete existing conditions: {}", e))
        })?;

        // 새 조건 삽입
        for condition in conditions {
            // value를 JSON 배열로 직렬화
            let value_json = serde_json::to_string(&condition.value).map_err(|e| {
                StorageError::Database(format!("Failed to serialize condition values: {}", e))
            })?;
            
            tx.exec_drop(
                r"INSERT INTO watcher_conditions (
                    account_hash, watcher_id, local_group_id, condition_type, `key`, value, operator, created_at, updated_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                (
                    &condition.account_hash,
                    condition.watcher_id,
                    condition.local_group_id,
                    condition.condition_type.to_string(),
                    &condition.key,
                    &value_json,
                    &condition.operator,
                    condition.created_at.timestamp(),
                    condition.updated_at.timestamp(),
                ),
            ).await.map_err(|e| {
                StorageError::Database(format!("Failed to insert watcher condition: {}", e))
            })?;
        }

        // 트랜잭션 커밋
        if let Err(e) = tx.commit().await {
            error!("Failed to commit transaction: {}", e);
            return Err(StorageError::Database(format!("Failed to commit transaction: {}", e)));
        }

                  Ok(())
      }
    
    /// 클라이언트 group_id로 서버 group_id 조회
    async fn get_server_group_id(&self, account_hash: &str, client_group_id: i32) -> Result<Option<i32>> {
        let pool = self.get_pool();
        let mut conn = pool.get_conn().await.map_err(|e| {
            error!("Failed to get connection: {}", e);
            StorageError::Database(format!("Failed to get connection: {}", e))
        })?;
        
        debug!("Getting server group ID for client_group_id={}, account_hash={}", client_group_id, account_hash);
        
        let server_id: Option<(i32,)> = conn.exec_first(
            "SELECT id FROM watcher_groups WHERE account_hash = ? AND group_id = ?",
            (account_hash, client_group_id)
        ).await.map_err(|e| {
            error!("Failed to get server group ID: {}", e);
            StorageError::Database(format!("Failed to get server group ID: {}", e))
        })?;
        
        Ok(server_id.map(|(id,)| id))
    }
    
    /// 클라이언트 group_id와 watcher_id로 서버 IDs 조회
    async fn get_server_ids(&self, account_hash: &str, client_group_id: i32, client_watcher_id: i32) -> Result<Option<(i32, i32)>> {
        let pool = self.get_pool();
        let mut conn = pool.get_conn().await.map_err(|e| {
            error!("Failed to get connection: {}", e);
            StorageError::Database(format!("Failed to get connection: {}", e))
        })?;
        
        debug!("Getting server IDs for client_group_id={}, client_watcher_id={}, account_hash={}", 
               client_group_id, client_watcher_id, account_hash);
        
        // 먼저 그룹 ID 변환
        let server_group_id: Option<(i32,)> = conn.exec_first(
            "SELECT id FROM watcher_groups WHERE account_hash = ? AND group_id = ?",
            (account_hash, client_group_id)
        ).await.map_err(|e| {
            error!("Failed to get server group ID: {}", e);
            StorageError::Database(format!("Failed to get server group ID: {}", e))
        })?;
        
        if let Some((group_id,)) = server_group_id {
            // 워처 ID 변환
            let server_watcher_id: Option<(i32,)> = conn.exec_first(
                "SELECT id FROM watchers WHERE account_hash = ? AND local_group_id = ? AND watcher_id = ?",
                (account_hash, client_group_id, client_watcher_id)
            ).await.map_err(|e| {
                error!("Failed to get server watcher ID: {}", e);
                StorageError::Database(format!("Failed to get server watcher ID: {}", e))
            })?;
            
            if let Some((watcher_id,)) = server_watcher_id {
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
