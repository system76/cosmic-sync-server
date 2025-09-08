use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex as TokioMutex;
use tracing::debug;

use crate::models::account::Account;
use crate::models::auth::AuthToken;
use crate::models::device::{Device, DeviceInfo as ModelDeviceInfo};
use crate::models::file::FileInfo as ModelFileInfo;
use crate::models::file::{FileInfo, FileNotice};
use crate::models::watcher::{WatcherGroup, WatcherPreset};
use crate::models::FileEntry;

use crate::storage::{Result, Storage, StorageError, StorageMetrics};
use crate::sync::{DeviceInfo, WatcherData, WatcherGroupData};
use chrono::{TimeZone, Utc};
use prost_types::Timestamp;

// In-memory storage data structure (using Mutex for thread safety)
struct StorageData {
    accounts: HashMap<String, Account>, // account_hash -> account
    devices: HashMap<String, Device>,   // device_hash -> device_info
    files: HashMap<u64, FileInfo>,      // file_id -> file_info
    file_data: HashMap<u64, Vec<u8>>,   // file_id -> binary_data
    watcher_groups: HashMap<i32, WatcherGroup>, // group_id -> watcher_group
    watchers: HashMap<i32, crate::models::watcher::Watcher>, // watcher_id -> watcher
    watcher_presets: HashMap<String, Vec<String>>, // account_hash -> presets
    auth_tokens: HashMap<String, AuthToken>, // token -> auth_token
    encryption_keys: HashMap<String, String>, // account_hash -> encryption_key
    next_file_id: u64,                  // 파일 ID 자동 증가를 위한 카운터
    next_condition_id: i64,             // 조건 ID 자동 증가를 위한 카운터
    watcher_conditions: HashMap<i64, crate::models::watcher::WatcherCondition>, // condition_id -> condition
}

impl StorageData {
    fn new() -> Self {
        Self {
            accounts: HashMap::new(),
            devices: HashMap::new(),
            files: HashMap::new(),
            file_data: HashMap::new(),
            watcher_groups: HashMap::new(),
            watchers: HashMap::new(),
            watcher_presets: HashMap::new(),
            auth_tokens: HashMap::new(),
            encryption_keys: HashMap::new(),
            next_file_id: 1,      // 1부터 시작
            next_condition_id: 1, // 1부터 시작
            watcher_conditions: HashMap::new(),
        }
    }
}

/// In-memory storage implementation (useful for testing)
pub struct MemoryStorage {
    data: TokioMutex<StorageData>,
}

impl MemoryStorage {
    /// Create a new memory storage instance
    pub fn new() -> Self {
        Self {
            data: TokioMutex::new(StorageData::new()),
        }
    }
}

#[async_trait]
impl Storage for MemoryStorage {
    /// Get the storage instance as Any for downcasting
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    /// Create a new account
    async fn create_account(&self, account: &Account) -> crate::storage::Result<()> {
        let mut data = self.data.lock().await;
        data.accounts
            .insert(account.account_hash.clone(), account.clone());
        Ok(())
    }

    /// Get account by ID
    async fn get_account_by_id(&self, id: &str) -> crate::storage::Result<Option<Account>> {
        let data = self.data.lock().await;

        for account in data.accounts.values() {
            if account.user_id == id {
                return Ok(Some(account.clone()));
            }
        }

        Ok(None)
    }

    /// Get account by email
    async fn get_account_by_email(&self, email: &str) -> crate::storage::Result<Option<Account>> {
        let data = self.data.lock().await;

        for account in data.accounts.values() {
            if account.email.to_lowercase() == email.to_lowercase() {
                return Ok(Some(account.clone()));
            }
        }

        Ok(None)
    }

    /// Get account by hash
    async fn get_account_by_hash(
        &self,
        account_hash: &str,
    ) -> crate::storage::Result<Option<Account>> {
        let data = self.data.lock().await;
        Ok(data.accounts.get(account_hash).cloned())
    }

    /// Update account
    async fn update_account(&self, account: &Account) -> crate::storage::Result<()> {
        let mut data = self.data.lock().await;
        data.accounts
            .insert(account.account_hash.clone(), account.clone());
        Ok(())
    }

    /// Delete account
    async fn delete_account(&self, account_hash: &str) -> crate::storage::Result<()> {
        let mut data = self.data.lock().await;
        data.accounts.remove(account_hash);
        Ok(())
    }

    /// Create auth token
    async fn create_auth_token(&self, auth_token: &AuthToken) -> crate::storage::Result<()> {
        let mut data = self.data.lock().await;
        data.auth_tokens
            .insert(auth_token.access_token.clone(), auth_token.clone());
        Ok(())
    }

    /// Get auth token
    async fn get_auth_token(&self, token: &str) -> crate::storage::Result<Option<AuthToken>> {
        let data = self.data.lock().await;
        Ok(data.auth_tokens.get(token).cloned())
    }

    /// Validate auth token
    async fn validate_auth_token(
        &self,
        token: &str,
        account_hash: &str,
    ) -> crate::storage::Result<bool> {
        let data = self.data.lock().await;

        if let Some(auth_token) = data.auth_tokens.get(token) {
            return Ok(auth_token.account_hash == account_hash);
        }

        Ok(false)
    }

    /// Update auth token
    async fn update_auth_token(&self, auth_token: &AuthToken) -> crate::storage::Result<()> {
        let mut data = self.data.lock().await;
        data.auth_tokens
            .insert(auth_token.access_token.clone(), auth_token.clone());
        Ok(())
    }

    /// Delete auth token
    async fn delete_auth_token(&self, token: &str) -> crate::storage::Result<()> {
        let mut data = self.data.lock().await;
        data.auth_tokens.remove(token);
        Ok(())
    }

    /// Register a new device
    async fn register_device(&self, device: &Device) -> crate::storage::Result<()> {
        let mut data = self.data.lock().await;

        let key = format!("{}:{}", device.account_hash, device.device_hash);
        data.devices.insert(key, device.clone());

        Ok(())
    }

    /// Get device by hash
    async fn get_device(
        &self,
        account_hash: &str,
        device_hash: &str,
    ) -> crate::storage::Result<Option<Device>> {
        let data = self.data.lock().await;

        let key = format!("{}:{}", account_hash, device_hash);
        Ok(data.devices.get(&key).cloned())
    }

    /// List devices by account
    async fn list_devices(&self, account_hash: &str) -> crate::storage::Result<Vec<Device>> {
        let data = self.data.lock().await;

        let devices: Vec<Device> = data
            .devices
            .values()
            .filter(|device| device.account_hash == account_hash)
            .cloned()
            .collect();

        Ok(devices)
    }

    /// Update device
    async fn update_device(&self, device: &Device) -> crate::storage::Result<()> {
        let mut data = self.data.lock().await;

        let key = format!("{}:{}", device.account_hash, device.device_hash);
        data.devices.insert(key, device.clone());

        Ok(())
    }

    /// Delete device
    async fn delete_device(
        &self,
        account_hash: &str,
        device_hash: &str,
    ) -> crate::storage::Result<()> {
        let mut data = self.data.lock().await;

        let key = format!("{}:{}", account_hash, device_hash);
        data.devices.remove(&key);

        Ok(())
    }

    /// Validate device
    async fn validate_device(
        &self,
        account_hash: &str,
        device_hash: &str,
    ) -> crate::storage::Result<bool> {
        let data = self.data.lock().await;

        let key = format!("{}:{}", account_hash, device_hash);
        let valid = data.devices.contains_key(&key);

        Ok(valid)
    }

    /// Store file info
    async fn store_file_info(&self, file: FileInfo) -> crate::storage::Result<u64> {
        let mut data = self.data.lock().await;

        let file_id = if file.file_id == 0 {
            // 새 ID 생성
            let id = data.next_file_id;
            data.next_file_id += 1;
            id
        } else {
            file.file_id
        };

        // 파일 정보에 ID 할당
        let mut file_info = file;
        file_info.file_id = file_id;

        // 파일 정보 저장
        data.files.insert(file_id, file_info);

        Ok(file_id)
    }

    /// Get file info by id
    async fn get_file_info(&self, file_id: u64) -> crate::storage::Result<Option<FileInfo>> {
        let data = self.data.lock().await;
        Ok(data.files.get(&file_id).cloned())
    }

    /// Get file info by path
    async fn get_file_info_by_path(
        &self,
        account_hash: &str,
        file_path: &str,
        group_id: i32,
    ) -> crate::storage::Result<Option<FileInfo>> {
        let data = self.data.lock().await;

        for file in data.files.values() {
            if file.account_hash == account_hash
                && file.file_path == file_path
                && file.group_id == group_id
            {
                return Ok(Some(file.clone()));
            }
        }

        Ok(None)
    }

    /// Get file by hash
    async fn get_file_by_hash(
        &self,
        account_hash: &str,
        file_hash: &str,
    ) -> crate::storage::Result<Option<FileInfo>> {
        let data = self.data.lock().await;

        for file in data.files.values() {
            if file.account_hash == account_hash && file.file_hash == file_hash {
                return Ok(Some(file.clone()));
            }
        }

        Ok(None)
    }

    /// List files
    async fn list_files(
        &self,
        account_hash: &str,
        group_id: i32,
        upload_time_from: Option<i64>,
    ) -> crate::storage::Result<Vec<FileInfo>> {
        let data = self.data.lock().await;

        let files: Vec<FileInfo> = data
            .files
            .values()
            .filter(|file| {
                file.account_hash == account_hash
                    && file.group_id == group_id
                    && if let Some(time) = upload_time_from {
                        // 업로드 시간 필터링이 있는 경우 확인
                        file.updated_time.seconds >= time
                    } else {
                        true
                    }
            })
            .cloned()
            .collect();

        Ok(files)
    }

    /// List files except those uploaded by specific device
    async fn list_files_except_device(
        &self,
        account_hash: &str,
        group_id: i32,
        exclude_device_hash: &str,
        upload_time_from: Option<i64>,
    ) -> crate::storage::Result<Vec<FileInfo>> {
        let data = self.data.lock().await;

        let files: Vec<FileInfo> = data
            .files
            .values()
            .filter(|file| {
                file.account_hash == account_hash &&
                file.group_id == group_id &&
                file.device_hash != exclude_device_hash && // 지정된 디바이스가 업로드한 파일 제외
                if let Some(time) = upload_time_from {
                    // 업로드 시간 필터링이 있는 경우 확인
                    file.updated_time.seconds >= time
                } else {
                    true
                }
            })
            .cloned()
            .collect();

        Ok(files)
    }

    /// Delete a file including its metadata and content
    async fn delete_file(&self, account_hash: &str, file_id: u64) -> crate::storage::Result<()> {
        let mut data = self.data.lock().await;

        if let Some(file) = data.files.get(&file_id) {
            // 파일이 특정 계정에 속해 있는지 확인
            if file.account_hash == account_hash {
                // 파일 데이터 삭제
                data.file_data.remove(&file_id);

                // 파일 메타데이터 삭제
                data.files.remove(&file_id);

                Ok(())
            } else {
                Err(StorageError::PermissionDenied(format!(
                    "File not owned by user: {}",
                    account_hash
                )))
            }
        } else {
            Err(StorageError::NotFound(format!(
                "File not found: {}",
                file_id
            )))
        }
    }

    /// Store file data
    async fn store_file_data(
        &self,
        file_id: u64,
        data_bytes: Vec<u8>,
    ) -> crate::storage::Result<()> {
        let mut data = self.data.lock().await;
        data.file_data.insert(file_id, data_bytes);
        Ok(())
    }

    /// Get file data
    async fn get_file_data(&self, file_id: u64) -> crate::storage::Result<Option<Vec<u8>>> {
        let data = self.data.lock().await;
        Ok(data.file_data.get(&file_id).cloned())
    }

    /// Store encryption key
    async fn store_encryption_key(
        &self,
        account_hash: &str,
        encryption_key: &str,
    ) -> crate::storage::Result<()> {
        let mut data = self.data.lock().await;
        data.encryption_keys
            .insert(account_hash.to_string(), encryption_key.to_string());
        Ok(())
    }

    /// Get encryption key
    async fn get_encryption_key(
        &self,
        account_hash: &str,
    ) -> crate::storage::Result<Option<String>> {
        let data = self.data.lock().await;
        Ok(data.encryption_keys.get(account_hash).cloned())
    }

    /// Register watcher group
    async fn register_watcher_group(
        &self,
        account_hash: &str,
        device_hash: &str,
        watcher_group: &WatcherGroup,
    ) -> crate::storage::Result<i32> {
        let mut data = self.data.lock().await;

        // 새 서버 ID 생성 (AUTO_INCREMENT)
        let server_id = data.watcher_groups.keys().max().map_or(1, |id| id + 1);

        // 워처 그룹 복제 및 서버 ID 설정
        let mut group = watcher_group.clone();
        group.id = server_id; // 서버에서 생성한 ID로 변경
                              // local_id는 클라이언트에서 온 값 그대로 유지

        // watcher_ids를 복사
        group.watcher_ids = watcher_group.watcher_ids.clone();

        // 그룹 추가
        data.watcher_groups.insert(server_id, group);

        Ok(server_id)
    }

    /// Get watcher groups
    async fn get_watcher_groups(
        &self,
        account_hash: &str,
    ) -> crate::storage::Result<Vec<WatcherGroup>> {
        let data = self.data.lock().await;

        let groups = data
            .watcher_groups
            .values()
            .filter(|group| group.account_hash == account_hash)
            .cloned()
            .collect();

        Ok(groups)
    }

    /// Get user watcher group
    async fn get_user_watcher_group(
        &self,
        account_hash: &str,
        group_id: i32,
    ) -> crate::storage::Result<Option<WatcherGroup>> {
        let data = self.data.lock().await;

        if let Some(group) = data.watcher_groups.get(&group_id) {
            if group.account_hash == account_hash {
                return Ok(Some(group.clone()));
            }
        }

        Ok(None)
    }

    /// Update watcher group
    async fn update_watcher_group(
        &self,
        account_hash: &str,
        watcher_group: &WatcherGroup,
    ) -> crate::storage::Result<()> {
        let mut data = self.data.lock().await;

        let id = watcher_group.id;

        // 기존 그룹 찾기
        if let Some(group) = data.watcher_groups.get(&id) {
            // 사용자 확인
            if group.account_hash != account_hash {
                return Err(StorageError::PermissionDenied(
                    "Not the owner of the watcher group".to_string(),
                ));
            }
        } else {
            return Err(StorageError::NotFound(
                "Watcher group not found".to_string(),
            ));
        }

        // 그룹 업데이트
        data.watcher_groups.insert(id, watcher_group.clone());

        Ok(())
    }

    /// Delete watcher group
    async fn delete_watcher_group(
        &self,
        account_hash: &str,
        group_id: i32,
    ) -> crate::storage::Result<()> {
        let mut data = self.data.lock().await;

        // 그룹 확인 및 소유자 검증
        let owned = match data.watcher_groups.get(&group_id) {
            Some(group) if group.account_hash == account_hash => true,
            Some(_) => {
                return Err(StorageError::PermissionDenied(
                    "Not the owner of the watcher group".to_string(),
                ))
            }
            None => {
                return Err(StorageError::NotFound(
                    "Watcher group not found".to_string(),
                ))
            }
        };

        if owned {
            // 해당 그룹에 속한 watcher 및 조건 정리
            let watcher_ids: Vec<i32> = data
                .watchers
                .iter()
                .filter(|(_, w)| w.account_hash == account_hash && w.group_id == group_id)
                .map(|(id, _)| *id)
                .collect();

            // 조건 삭제
            let condition_ids: Vec<i64> = data
                .watcher_conditions
                .iter()
                .filter(|(_, c)| watcher_ids.contains(&c.watcher_id))
                .map(|(id, _)| *id)
                .collect();
            for cid in condition_ids {
                data.watcher_conditions.remove(&cid);
            }

            // 워처 삭제
            for wid in watcher_ids {
                data.watchers.remove(&wid);
            }

            // 그룹 삭제
            data.watcher_groups.remove(&group_id);
        }

        Ok(())
    }

    /// Get watcher group by account and id
    async fn get_watcher_group_by_account_and_id(
        &self,
        account_hash: &str,
        group_id: i32,
    ) -> crate::storage::Result<Option<WatcherGroupData>> {
        let data = self.data.lock().await;

        if let Some(group) = data.watcher_groups.get(&group_id) {
            // 그룹의 소유자 확인
            if group.account_hash != account_hash {
                return Ok(None);
            }

            // WatcherGroupData 객체 구성
            let group_data = WatcherGroupData {
                group_id: group.group_id, // 클라이언트에게는 group_id를 반환
                title: group.title.clone(),
                last_updated: None,
                watchers: Vec::new(),
            };

            return Ok(Some(group_data));
        }

        Ok(None)
    }

    /// Register watcher preset (proto호환 메소드)
    async fn register_watcher_preset_proto(
        &self,
        account_hash: &str,
        device_hash: &str,
        presets: Vec<String>,
    ) -> crate::storage::Result<()> {
        let mut data = self.data.lock().await;
        data.watcher_presets
            .insert(account_hash.to_string(), presets);
        Ok(())
    }

    /// Update watcher preset (proto호환 메소드)
    async fn update_watcher_preset_proto(
        &self,
        account_hash: &str,
        device_hash: &str,
        presets: Vec<String>,
    ) -> crate::storage::Result<()> {
        let mut data = self.data.lock().await;
        data.watcher_presets
            .insert(account_hash.to_string(), presets);
        Ok(())
    }

    /// Get watcher preset
    async fn get_watcher_preset(&self, account_hash: &str) -> crate::storage::Result<Vec<String>> {
        let data = self.data.lock().await;

        if let Some(presets) = data.watcher_presets.get(account_hash) {
            return Ok(presets.clone());
        }

        Ok(Vec::new())
    }

    /// 파일 경로와 이름으로 파일 찾기
    async fn find_file_by_path_and_name(
        &self,
        account_hash: &str,
        file_path: &str,
        filename: &str,
        revision: i64,
    ) -> crate::storage::Result<Option<FileInfo>> {
        let data = self.data.lock().await;

        for file in data.files.values() {
            // 계정, 경로, 파일명, 리비전이 일치하는지 확인합니다
            if file.account_hash == account_hash
                && file.file_path == file_path
                && file.filename == filename
                && (revision == 0 || file.revision == revision)
            {
                return Ok(Some(file.clone()));
            }
        }

        Ok(None)
    }

    /// 경로, 파일명, 그룹 ID로 파일 찾기
    async fn find_file_by_criteria(
        &self,
        account_hash: &str,
        group_id: i32,
        watcher_id: i32,
        file_path: &str,
        filename: &str,
    ) -> crate::storage::Result<Option<FileInfo>> {
        let data = self.data.lock().await;

        for file in data.files.values() {
            // 계정, 그룹 ID, 워처 ID, 경로, 파일명이 일치하는 파일 찾기
            if file.account_hash == account_hash
                && file.group_id == group_id
                && file.watcher_id == watcher_id
                && file.file_path == file_path
                && file.filename == filename
            {
                // 메모리 저장소에서는 is_deleted 필드가 없으므로, 모든 조건에 맞는 첫 번째 파일을 반환
                debug!(
                    "파일을 찾았습니다: file_id={}, path={}, group_id={}, watcher_id={}",
                    file.file_id, file_path, group_id, watcher_id
                );
                return Ok(Some(file.clone()));
            }
        }

        debug!(
            "파일을 찾을 수 없습니다: path={}, filename={}, group_id={}, watcher_id={}",
            file_path, filename, group_id, watcher_id
        );
        Ok(None)
    }

    /// 폴더 경로로 워처 찾기
    async fn find_watcher_by_folder(
        &self,
        account_hash: &str,
        group_id: i32,
        folder: &str,
    ) -> crate::storage::Result<Option<i32>> {
        // Normalize folder path to preserve tilde (~) prefix for home directory
        let normalized_folder = crate::utils::helpers::normalize_path_preserve_tilde(folder);

        let data = self.data.lock().await;

        // 메모리 기반 스토리지에서는 간단하게 구현
        for (id, watcher) in &data.watchers {
            if watcher.account_hash == account_hash
                && watcher.group_id == group_id
                && watcher.folder == normalized_folder
            {
                return Ok(Some(*id));
            }
        }

        Ok(None)
    }

    // removed: create_watcher without conditions (use create_watcher_with_conditions instead)

    /// 워처 생성 (conditions 포함)
    async fn create_watcher_with_conditions(
        &self,
        account_hash: &str,
        group_id: i32,
        watcher_data: &crate::sync::WatcherData,
        timestamp: i64,
    ) -> crate::storage::Result<i32> {
        // Normalize folder path to preserve tilde (~) prefix for home directory
        let normalized_folder =
            crate::utils::helpers::normalize_path_preserve_tilde(&watcher_data.folder);

        let mut data = self.data.lock().await;

        // 새 워처 ID 생성
        let next_id = data.watchers.keys().max().map_or(1, |id| id + 1);

        // 워처 구조체 생성
        let watcher = crate::models::watcher::Watcher {
            id: next_id,
            watcher_id: watcher_data.watcher_id, // 클라이언트 워처 ID
            account_hash: account_hash.to_string(),
            group_id,
            local_group_id: group_id, // 클라이언트 측 local_group_id (동기화용)
            title: format!("Watcher {}", next_id),
            folder: normalized_folder, // 정규화된 경로 사용
            union_conditions: watcher_data
                .union_conditions
                .iter()
                .map(|cd| crate::models::watcher::Condition {
                    key: cd.key.clone(),
                    value: cd.value.clone(), // ConditionData.value는 이미 Vec<String>
                })
                .collect(),
            subtracting_conditions: watcher_data
                .subtracting_conditions
                .iter()
                .map(|cd| crate::models::watcher::Condition {
                    key: cd.key.clone(),
                    value: cd.value.clone(), // ConditionData.value는 이미 Vec<String>
                })
                .collect(),
            recursive_path: watcher_data.recursive_path,
            preset: watcher_data.preset,
            custom_type: watcher_data.custom_type.clone(),
            update_mode: watcher_data.update_mode.clone(),
            created_at: chrono::DateTime::from_timestamp(timestamp, 0)
                .unwrap_or_else(|| chrono::Utc::now()),
            updated_at: chrono::DateTime::from_timestamp(timestamp, 0)
                .unwrap_or_else(|| chrono::Utc::now()),
            is_active: true,
            extra_json: watcher_data.extra_json.clone(),
        };

        // 데이터 저장
        data.watchers.insert(next_id, watcher);

        // conditions도 별도 저장
        for condition in &watcher_data.union_conditions {
            let condition_id = data.next_condition_id;
            data.next_condition_id += 1;

            let watcher_condition = crate::models::watcher::WatcherCondition {
                id: Some(condition_id),
                account_hash: account_hash.to_string(), // account_hash 추가
                watcher_id: next_id,
                local_watcher_id: watcher_data.watcher_id, // 클라이언트 측 watcher ID
                local_group_id: group_id,                  // 클라이언트 측 group ID
                condition_type: crate::models::watcher::ConditionType::Union,
                key: condition.key.clone(),
                value: condition.value.clone(), // ConditionData.value는 이미 Vec<String>
                operator: "equals".to_string(),
                created_at: chrono::DateTime::from_timestamp(timestamp, 0)
                    .unwrap_or_else(|| chrono::Utc::now()),
                updated_at: chrono::DateTime::from_timestamp(timestamp, 0)
                    .unwrap_or_else(|| chrono::Utc::now()),
            };
            data.watcher_conditions
                .insert(condition_id, watcher_condition);
        }

        for condition in &watcher_data.subtracting_conditions {
            let condition_id = data.next_condition_id;
            data.next_condition_id += 1;

            let watcher_condition = crate::models::watcher::WatcherCondition {
                id: Some(condition_id),
                account_hash: account_hash.to_string(), // account_hash 추가
                watcher_id: next_id,
                local_watcher_id: watcher_data.watcher_id, // 클라이언트 측 watcher ID
                local_group_id: group_id,                  // 클라이언트 측 group ID
                condition_type: crate::models::watcher::ConditionType::Subtract,
                key: condition.key.clone(),
                value: condition.value.clone(), // ConditionData.value는 이미 Vec<String>
                operator: "equals".to_string(),
                created_at: chrono::DateTime::from_timestamp(timestamp, 0)
                    .unwrap_or_else(|| chrono::Utc::now()),
                updated_at: chrono::DateTime::from_timestamp(timestamp, 0)
                    .unwrap_or_else(|| chrono::Utc::now()),
            };
            data.watcher_conditions
                .insert(condition_id, watcher_condition);
        }

        // 해당 그룹에 워처 ID 추가
        if let Some(group) = data.watcher_groups.get_mut(&group_id) {
            if !group.watcher_ids.contains(&next_id) {
                group.watcher_ids.push(next_id);
            }
        }

        Ok(next_id)
    }

    /// 그룹 ID와 워처 ID로 워처 정보 조회
    async fn get_watcher_by_group_and_id(
        &self,
        account_hash: &str,
        group_id: i32,
        watcher_id: i32,
    ) -> crate::storage::Result<Option<crate::sync::WatcherData>> {
        let data = self.data.lock().await;

        for watcher in data.watchers.values() {
            if watcher.account_hash == account_hash
                && watcher.local_group_id == group_id
                && watcher.watcher_id == watcher_id
            {
                let proto_watcher = crate::sync::WatcherData {
                    watcher_id: watcher.watcher_id,
                    folder: watcher.folder.clone(),
                    union_conditions: watcher
                        .union_conditions
                        .iter()
                        .map(|c| crate::sync::ConditionData {
                            key: c.key.clone(),
                            value: c.value.clone(),
                        })
                        .collect(),
                    subtracting_conditions: watcher
                        .subtracting_conditions
                        .iter()
                        .map(|c| crate::sync::ConditionData {
                            key: c.key.clone(),
                            value: c.value.clone(),
                        })
                        .collect(),
                    recursive_path: watcher.recursive_path,
                    preset: watcher.preset,
                    custom_type: watcher.custom_type.clone(),
                    update_mode: watcher.update_mode.clone(),
                    is_active: watcher.is_active,
                    extra_json: watcher.extra_json.clone(),
                };

                return Ok(Some(proto_watcher));
            }
        }

        Ok(None)
    }

    /// 클라이언트 group_id로 서버 group_id 조회
    async fn get_server_group_id(
        &self,
        account_hash: &str,
        client_group_id: i32,
    ) -> crate::storage::Result<Option<i32>> {
        let data = self.data.lock().await;

        for (server_id, group) in &data.watcher_groups {
            if group.account_hash == account_hash && group.group_id == client_group_id {
                return Ok(Some(*server_id));
            }
        }

        Ok(None)
    }

    /// 클라이언트 group_id와 watcher_id로 서버 IDs 조회
    async fn get_server_ids(
        &self,
        account_hash: &str,
        client_group_id: i32,
        client_watcher_id: i32,
    ) -> crate::storage::Result<Option<(i32, i32)>> {
        let data = self.data.lock().await;

        // 먼저 서버 group_id 찾기
        let mut server_group_id = None;
        for (id, group) in &data.watcher_groups {
            if group.account_hash == account_hash && group.group_id == client_group_id {
                server_group_id = Some(*id);
                break;
            }
        }

        if let Some(group_id) = server_group_id {
            // 워처 찾기
            for (id, watcher) in &data.watchers {
                if watcher.account_hash == account_hash
                    && watcher.local_group_id == client_group_id
                    && watcher.watcher_id == client_watcher_id
                {
                    return Ok(Some((group_id, *id)));
                }
            }
        }

        Ok(None)
    }

    /// 파일 정보 조회 (삭제된 파일 포함)
    async fn get_file_info_include_deleted(
        &self,
        file_id: u64,
    ) -> crate::storage::Result<Option<(FileInfo, bool)>> {
        // 메모리 스토리지에서는 삭제된 파일을 추적하지 않으므로,
        // 기존 get_file_info 메서드를 호출하고 삭제 여부는 항상 false로 반환
        match self.get_file_info(file_id).await? {
            Some(info) => Ok(Some((info, false))),
            None => Ok(None),
        }
    }

    /// 파일 ID로 존재 여부와 삭제 상태 확인
    async fn check_file_exists(&self, file_id: u64) -> crate::storage::Result<(bool, bool)> {
        let data = self.data.lock().await;

        if let Some(file) = data.files.get(&file_id) {
            // is_deleted 필드가 없으므로 항상 false로 가정
            Ok((true, false))
        } else {
            // 파일이 존재하지 않음
            Ok((false, false))
        }
    }

    // === Watcher Conditions Methods ===

    /// Create a new watcher condition
    async fn create_watcher_condition(
        &self,
        condition: &crate::models::watcher::WatcherCondition,
    ) -> crate::storage::Result<i64> {
        let mut data = self.data.lock().await;

        let new_id = data.next_condition_id;
        data.next_condition_id += 1;

        let mut new_condition = condition.clone();
        new_condition.id = Some(new_id);

        data.watcher_conditions.insert(new_id, new_condition);

        Ok(new_id)
    }

    /// Get all conditions for a watcher
    async fn get_watcher_conditions(
        &self,
        account_hash: &str,
        watcher_id: i32,
    ) -> crate::storage::Result<Vec<crate::models::watcher::WatcherCondition>> {
        let data = self.data.lock().await;

        let conditions: Vec<crate::models::watcher::WatcherCondition> = data
            .watcher_conditions
            .values()
            .filter(|condition| {
                condition.watcher_id == watcher_id && condition.account_hash == account_hash
            })
            .cloned()
            .collect();

        Ok(conditions)
    }

    /// Update a watcher condition
    async fn update_watcher_condition(
        &self,
        condition: &crate::models::watcher::WatcherCondition,
    ) -> crate::storage::Result<()> {
        let mut data = self.data.lock().await;

        let condition_id = condition.id.ok_or_else(|| {
            crate::storage::StorageError::ValidationError(
                "Condition ID is required for update".to_string(),
            )
        })?;

        if data.watcher_conditions.contains_key(&condition_id) {
            data.watcher_conditions
                .insert(condition_id, condition.clone());
            Ok(())
        } else {
            Err(crate::storage::StorageError::NotFound(format!(
                "Condition with ID {} not found",
                condition_id
            )))
        }
    }

    /// Delete a watcher condition
    async fn delete_watcher_condition(&self, condition_id: i64) -> crate::storage::Result<()> {
        let mut data = self.data.lock().await;

        if data.watcher_conditions.remove(&condition_id).is_some() {
            Ok(())
        } else {
            Err(crate::storage::StorageError::NotFound(format!(
                "Condition with ID {} not found",
                condition_id
            )))
        }
    }

    /// Delete all conditions for a watcher
    async fn delete_all_watcher_conditions(&self, watcher_id: i32) -> crate::storage::Result<()> {
        let mut data = self.data.lock().await;

        let condition_ids: Vec<i64> = data
            .watcher_conditions
            .iter()
            .filter(|(_, condition)| condition.watcher_id == watcher_id)
            .map(|(id, _)| *id)
            .collect();

        for condition_id in condition_ids {
            data.watcher_conditions.remove(&condition_id);
        }

        Ok(())
    }

    /// Save watcher conditions (replace all existing conditions)
    async fn save_watcher_conditions(
        &self,
        watcher_id: i32,
        conditions: &[crate::models::watcher::WatcherCondition],
    ) -> crate::storage::Result<()> {
        let mut data = self.data.lock().await;

        // Delete existing conditions for this watcher
        let condition_ids: Vec<i64> = data
            .watcher_conditions
            .iter()
            .filter(|(_, condition)| condition.watcher_id == watcher_id)
            .map(|(id, _)| *id)
            .collect();

        for condition_id in condition_ids {
            data.watcher_conditions.remove(&condition_id);
        }

        // Add new conditions
        for condition in conditions {
            let new_id = data.next_condition_id;
            data.next_condition_id += 1;

            let mut new_condition = condition.clone();
            new_condition.id = Some(new_id);
            new_condition.watcher_id = watcher_id;

            data.watcher_conditions.insert(new_id, new_condition);
        }

        Ok(())
    }

    /// 클라이언트 group_id로 서버 group_id 조회
    async fn get_client_watcher_id(
        &self,
        account_hash: &str,
        server_group_id: i32,
        server_watcher_id: i32,
    ) -> crate::storage::Result<Option<(i32, i32)>> {
        // MemoryStorage에서는 ID 변환이 필요 없음 (서버 ID를 사용하지 않음)
        Ok(Some((server_group_id, server_watcher_id)))
    }

    // Removed legacy WatcherPreset aliases (use proto-based methods only)

    // WatcherCondition 관련 메서드들
    async fn register_watcher_condition(
        &self,
        account_hash: &str,
        condition: &crate::models::watcher::WatcherCondition,
    ) -> crate::storage::Result<i64> {
        self.create_watcher_condition(condition).await
    }

    async fn get_watcher_condition(
        &self,
        condition_id: i64,
    ) -> crate::storage::Result<Option<crate::models::watcher::WatcherCondition>> {
        let data = self.data.lock().await;
        Ok(data.watcher_conditions.get(&condition_id).cloned())
    }

    async fn get_watcher_conditions_by_watcher(
        &self,
        account_hash: &str,
        watcher_id: i32,
    ) -> crate::storage::Result<Vec<crate::models::watcher::WatcherCondition>> {
        self.get_watcher_conditions(account_hash, watcher_id).await
    }

    // 중복 검사 메서드들
    async fn check_duplicate_watcher_group(
        &self,
        account_hash: &str,
        local_group_id: i32,
    ) -> crate::storage::Result<Option<i32>> {
        let data = self.data.lock().await;

        for (server_id, group) in &data.watcher_groups {
            if group.account_hash == account_hash && group.group_id == local_group_id {
                return Ok(Some(*server_id));
            }
        }

        Ok(None)
    }

    async fn check_duplicate_watcher(
        &self,
        account_hash: &str,
        local_watcher_id: i32,
    ) -> crate::storage::Result<Option<i32>> {
        let data = self.data.lock().await;

        for (server_id, watcher) in &data.watchers {
            if watcher.account_hash == account_hash && watcher.watcher_id == local_watcher_id {
                return Ok(Some(*server_id));
            }
        }

        Ok(None)
    }

    // FileNotice 관련 메서드들 (간단한 구현)
    async fn store_file_notice(
        &self,
        file_notice: &crate::models::file::FileNotice,
    ) -> crate::storage::Result<()> {
        // MemoryStorage에서는 FileNotice를 저장하지 않음 (임시 구현)
        Ok(())
    }

    async fn get_file_notices(
        &self,
        account_hash: &str,
        device_hash: &str,
    ) -> crate::storage::Result<Vec<crate::models::file::FileNotice>> {
        // MemoryStorage에서는 빈 리스트 반환 (임시 구현)
        Ok(Vec::new())
    }

    async fn delete_file_notice(
        &self,
        account_hash: &str,
        device_hash: &str,
        file_id: u64,
    ) -> crate::storage::Result<()> {
        // MemoryStorage에서는 아무 동작 없음 (임시 구현)
        Ok(())
    }

    // Version management methods implementation for MemoryStorage
    async fn get_file_history(
        &self,
        account_hash: &str,
        file_path: &str,
        group_id: i32,
    ) -> crate::storage::Result<Vec<crate::models::file::SyncFile>> {
        // For MemoryStorage, we return an empty list as this is primarily for testing
        // In a real implementation, this would query a files history table
        debug!(
            "MemoryStorage: Getting file history for path: {} in group: {} (returning empty list)",
            file_path, group_id
        );
        Ok(Vec::new())
    }

    async fn get_file_versions_by_id(
        &self,
        account_hash: &str,
        file_id: u64,
    ) -> crate::storage::Result<Vec<crate::models::file::SyncFile>> {
        // For MemoryStorage, we return an empty list as this is primarily for testing
        debug!(
            "MemoryStorage: Getting file versions for file_id: {} (returning empty list)",
            file_id
        );
        Ok(Vec::new())
    }

    async fn get_file_by_revision(
        &self,
        account_hash: &str,
        file_id: u64,
        revision: i64,
    ) -> crate::storage::Result<crate::models::file::SyncFile> {
        // For MemoryStorage, we return a NotFound error as this is primarily for testing
        debug!(
            "MemoryStorage: Getting file by revision: file_id={}, revision={} (not implemented)",
            file_id, revision
        );
        Err(crate::storage::StorageError::NotFound(format!(
            "File version not found in MemoryStorage: file_id={}, revision={}",
            file_id, revision
        )))
    }

    async fn store_file(&self, file: &crate::models::file::SyncFile) -> crate::storage::Result<()> {
        // For MemoryStorage, we just log the operation as this is primarily for testing
        debug!(
            "MemoryStorage: Storing file version: file_id={}, revision={}",
            file.file_id, file.revision
        );
        Ok(())
    }

    async fn get_devices_for_account(
        &self,
        account_hash: &str,
    ) -> crate::storage::Result<Vec<Device>> {
        let data = self.data.lock().await;
        let devices: Vec<Device> = data
            .devices
            .values()
            .filter(|device| device.account_hash == account_hash)
            .cloned()
            .collect();

        debug!(
            "MemoryStorage: Found {} devices for account: {}",
            devices.len(),
            account_hash
        );
        Ok(devices)
    }

    // Stub implementations for missing Storage trait methods
    async fn health_check(&self) -> Result<bool> {
        Ok(true)
    }

    async fn get_metrics(&self) -> Result<StorageMetrics> {
        let data = self.data.lock().await;
        Ok(StorageMetrics {
            total_queries: data.files.len() as u64, // Use file count as proxy
            successful_queries: data.accounts.len() as u64, // Use account count as proxy
            failed_queries: 0,
            average_query_time_ms: 0.0,
            active_connections: 1,
            idle_connections: 0,
            cache_hits: data.devices.len() as u64, // Use device count as proxy
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
        let data = self.data.lock().await;
        Ok(data.accounts.values().cloned().collect())
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

    async fn purge_deleted_files_older_than(&self, _ttl_secs: i64) -> Result<u64> {
        Ok(0)
    }

    async fn trim_old_revisions(&self, _max_revisions: i32) -> Result<u64> {
        Ok(0)
    }
}
