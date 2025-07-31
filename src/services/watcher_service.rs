use std::sync::Arc;
use std::sync::Mutex;
use std::collections::HashMap;
use tracing::debug;
use crate::models::watcher::{WatcherGroup, WatcherGroupData, Watcher as ModelWatcher};
use crate::sync::{self, WatcherData};
use crate::storage::{Storage, Result as StorageResult, StorageError};
use chrono::Utc;

/// Service for managing watcher groups (uses proto structures only)
#[derive(Clone)]
pub struct WatcherService {
    // Memory cache for watcher groups
    watcher_groups: Arc<Mutex<HashMap<String, Vec<WatcherGroup>>>>,
    
    // Database storage
    storage: Arc<dyn Storage>,
}

impl WatcherService {
    /// Create a new WatcherService instance
    pub fn new(storage: Arc<dyn Storage>) -> Self {
        Self {
            watcher_groups: Arc::new(Mutex::new(HashMap::new())),
            storage,
        }
    }
    
    /// Create a WatcherService instance with storage (same as new)
    pub fn with_storage(storage: Arc<dyn Storage>) -> Self {
        Self::new(storage)
    }
    
    /// Store a watcher group (uses DB model only)
    pub async fn store_watcher_group(
        &self,
        account_hash: String,
        device_hash: String,
        group: Option<WatcherGroup>,
    ) -> StorageResult<i32> {
        debug!("Store watcher group: account={}, device={}", account_hash, device_hash);
        let group = match group {
            Some(g) => g,
            None => return Err(crate::storage::StorageError::ValidationError("Group data is missing".to_string())),
        };
        // Store in DB using register_watcher_group
        self.storage.register_watcher_group(&account_hash, &device_hash, &group).await
    }
    
    /// Get watchers by group ID or specific watcher IDs
    pub async fn get_watchers(
        &self,
        account_hash: &str,
        group_id: i32,
        watcher_ids: Vec<i32>,
    ) -> StorageResult<Vec<WatcherData>> {
        debug!("Get watchers: account={}, group={}", account_hash, group_id);
        
        // 그룹 정보를 가져옴
        let group_data = self.storage.get_watcher_group_by_account_and_id(account_hash, group_id).await?;
        if let Some(group) = group_data {
            return Ok(group.watchers);
        }
        
        Ok(Vec::new())
    }

    pub async fn get_watchers_by_group(
        &self,
        account_hash: &str,
        group_id: i32,
    ) -> StorageResult<Vec<WatcherData>> {
        debug!("Get watchers in group: account={}, group={}", account_hash, group_id);
        
        // 그룹 정보를 가져옴
        let group_data = self.storage.get_watcher_group_by_account_and_id(account_hash, group_id).await?;
        if let Some(group) = group_data {
            return Ok(group.watchers);
        }
        
        Ok(Vec::new())
    }

    pub async fn get_watcher_group_by_account_and_id(
        &self,
        account_hash: &str,
        group_id: i32,
    ) -> StorageResult<Option<sync::WatcherGroupData>> {
        debug!("Get watcher group: account={}, group_id={}", account_hash, group_id);
        
        // Get the group and its watchers
        let group_opt = self.storage.get_watcher_group_by_account_and_id(account_hash, group_id).await?;
        
        // Convert to proto structure and return if found
        Ok(Some(group_opt.ok_or(StorageError::NotFound("WatcherGroup not found".to_string()))?))
    }
    /// Delete watcher group by account and id
    pub async fn delete_watcher_group(
        &self,
        account_hash: &str,
        device_hash: &str,
        group_id: i32,
    ) -> StorageResult<()> {
        debug!("Delete watcher group by account: account={}, device={}, group_id={}",
              account_hash, device_hash, group_id);
        
        self.storage.delete_watcher_group(account_hash, group_id).await
    }

    /// WatcherPreset 관련 메서드
    
    /// Register watcher presets
    pub async fn register_watcher_preset(
        &self,
        account_hash: &str,
        device_hash: &str,
        presets: Vec<String>,
    ) -> StorageResult<()> {
        debug!("Register watcher presets: account={}, device={}, count={}", 
              account_hash, device_hash, presets.len());
        self.storage.register_watcher_preset_proto(account_hash, device_hash, presets).await
    }
    
    /// Update watcher presets
    pub async fn update_watcher_preset(
        &self,
        account_hash: &str,
        device_hash: &str,
        presets: Vec<String>,
    ) -> StorageResult<()> {
        debug!("Update watcher presets: account={}, device={}, count={}", 
              account_hash, device_hash, presets.len());
        self.storage.update_watcher_preset_proto(account_hash, device_hash, presets).await
    }
    
    /// Sync watcher presets 
    pub async fn sync_watcher_preset(
        &self,
        account_hash: &str,
        device_hash: &str,
    ) -> StorageResult<()> {
        debug!("Sync watcher presets: account={}, device={}", account_hash, device_hash);
        // sync_watcher_preset 대신 get_watcher_preset을 사용하여 데이터만 반환
        // 실제 동기화 로직은 호출하는 쪽에서 처리해야 함
        self.storage.get_watcher_preset(account_hash).await?;
        Ok(())
    }
    
    /// Get watcher presets
    pub async fn get_watcher_preset(
        &self,
        account_hash: &str,
        device_hash: &str,
    ) -> StorageResult<Vec<String>> {
        debug!("Get watcher presets: account={}, device={}", account_hash, device_hash);
        self.storage.get_watcher_preset(account_hash).await
    }
    
    /// WatcherGroup 관련 메서드
    
    /// Get watcher group by ID
    pub async fn get_watcher_group(
        &self,
        account_hash: &str,
        group_id: i32,
    ) -> StorageResult<sync::WatcherGroupData> {
        debug!("Get watcher group: account={}, group_id={}", account_hash, group_id);
        
        // Get the group and its watchers
        let group = self.storage.get_watcher_group_by_account_and_id(account_hash, group_id).await?;
        
        // Convert to proto structure and return
        Ok(group.ok_or(StorageError::NotFound("WatcherGroup not found".to_string()))?)
    }
    
    /// Register watcher group
    pub async fn register_watcher_group(
        &self,
        account_hash: &str,
        device_hash: &str,
        group_id: i32,
        title: String,
        watcher_data: sync::WatcherData,
    ) -> StorageResult<i32> {
        debug!("Register watcher group: account={}, device={}, group_id={}",
              account_hash, device_hash, group_id);
        
        // WatcherGroup 객체 생성
        let watcher_group = WatcherGroup {
            id: 0, // 서버에서 AUTO_INCREMENT로 생성
            group_id: group_id, // 클라이언트 그룹 ID
            account_hash: account_hash.to_string(),
            title,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            is_active: true,
            watcher_ids: Vec::new(),
        };
        
        // 저장
        self.storage.register_watcher_group(account_hash, device_hash, &watcher_group).await
    }
    
    /// Update watcher group
    pub async fn update_watcher_group(
        &self,
        account_hash: &str,
        device_hash: &str,
        group_id: i32,
        title: String,
        watcher_data: sync::WatcherData,
    ) -> StorageResult<i32> {
        debug!("Update watcher group: account={}, device={}, group_id={}",
              account_hash, device_hash, group_id);
        
        // 기존 그룹 조회
        let group_opt = self.storage.get_user_watcher_group(account_hash, group_id).await?;
        
        if let Some(mut watcher_group) = group_opt {
            // 정보 업데이트
            watcher_group.title = title;
            watcher_group.updated_at = Utc::now();
            
            // 저장
            self.storage.update_watcher_group(account_hash, &watcher_group).await?;
            return Ok(group_id);
        }
        
        Err(StorageError::NotFound(format!("WatcherGroup {} not found", group_id)))
    }
    
    /// Sync watcher group
    pub async fn sync_watcher_group(
        &self,
        account_hash: &str,
        device_hash: &str,
        group_id: i32,
    ) -> StorageResult<()> {
        debug!("Sync watcher group: account={}, device={}, group_id={}",
              account_hash, device_hash, group_id);
        
        // 그룹 정보 가져오기만 함 - 실제 동기화 로직은 호출자가 처리해야 함
        let _ = self.storage.get_watcher_group_by_account_and_id(account_hash, group_id).await?;
        Ok(())
    }

    // 그룹 목록 조회 (내부 호출용)
    pub async fn get_watcher_groups(&self, account_hash: &str, device_hash: &str) -> StorageResult<Vec<WatcherGroup>> {
        debug!("WatcherService::get_watcher_groups - account_hash={}, device_hash={}", account_hash, device_hash);
        
        // 스토리지에서 그룹 목록 조회
        self.storage.get_watcher_groups(account_hash).await
    }
} 