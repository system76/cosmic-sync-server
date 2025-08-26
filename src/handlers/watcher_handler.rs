use async_trait::async_trait;
use futures::StreamExt;
use tonic::{Request, Response, Status};
use futures::FutureExt;
use crate::sync::{
    GetWatcherGroupRequest, GetWatcherGroupResponse,
    GetWatcherGroupsRequest, GetWatcherGroupsResponse,
    RegisterWatcherPresetRequest, RegisterWatcherPresetResponse,
    UpdateWatcherPresetRequest, UpdateWatcherPresetResponse,
    GetWatcherPresetRequest, GetWatcherPresetResponse,
    WatcherGroupData,
    RegisterWatcherGroupRequest, RegisterWatcherGroupResponse,
    UpdateWatcherGroupRequest, UpdateWatcherGroupResponse,
    DeleteWatcherGroupRequest, DeleteWatcherGroupResponse,
    SyncConfigurationRequest, SyncConfigurationResponse,
    SyncStats,
    AuthUpdateNotification, DeviceUpdateNotification, 
    EncryptionKeyUpdateNotification, FileUpdateNotification,
    WatcherPresetUpdateNotification, WatcherGroupUpdateNotification,
    VersionUpdateNotification
};
use std::sync::Arc;
use crate::server::app_state::AppState;
use crate::services::Handler;
use crate::models::WatcherGroup;
use crate::utils::auth;
use tracing::{info, warn, error, debug};
use chrono::Utc;

/// Watcher 관련 요청을 처리하는 핸들러
pub struct WatcherHandler {
    pub app_state: Arc<AppState>,
}

impl WatcherHandler {
    /// 새 Watcher 핸들러 생성
    pub fn new(app_state: Arc<AppState>) -> Self {
        Self { app_state }
    }
    
    /// Handle get watcher group request
    pub async fn get_watcher_group(&self, request: Request<GetWatcherGroupRequest>) -> Result<Response<GetWatcherGroupResponse>, Status> {
        let req = request.into_inner();
        // 토큰으로 검증된 account_hash를 우선 사용
        let verified = self.app_state.oauth.verify_token(&req.auth_token).await
            .map_err(|_| Status::unauthenticated("Invalid authentication token"))?;
        let account_hash = verified.account_hash;
        let device_hash = req.device_hash;
        let group_id = req.group_id;
        
        
        debug!("Processing get watcher group for user: {}, device: {}, group: {}", account_hash, device_hash, group_id);
        
        // Validate device if required
        auth::validate_device_if_required(
            self.app_state.storage.as_ref(), 
            &account_hash, 
            &device_hash
        ).await?;
        
        // Get the watcher group
        let group = match self.app_state.storage.get_watcher_group_by_account_and_id(&account_hash, group_id).await {
            Ok(Some(group)) => group,
            Ok(None) => {
                return Err(Status::not_found(format!("Watcher group not found: {}", group_id)));
            },
            Err(e) => {
                error!("Error getting watcher group: {}", e);
                return Err(Status::internal("Error getting watcher group"));
            }
        };
        
        let response = GetWatcherGroupResponse {
            success: true,
            watchergroup: Some(group),
            return_message: String::new(),
        };
        
        Ok(Response::new(response))
    }
    
    /// Handle get all watcher groups request
    pub async fn get_watcher_groups(&self, request: Request<GetWatcherGroupsRequest>) -> Result<Response<GetWatcherGroupsResponse>, Status> {
        let req = request.into_inner();
        // 토큰으로 검증된 account_hash를 우선 사용
        let verified = self.app_state.oauth.verify_token(&req.auth_token).await
            .map_err(|_| Status::unauthenticated("Invalid authentication token"))?;
        let user_id = verified.account_hash;
        let device_hash = req.device_hash;
        
        
        debug!("Processing get all watcher groups for user: {}, device: {}", user_id, device_hash);
        
        // Validate device if required
        auth::validate_device_if_required(
            self.app_state.storage.as_ref(), 
            &user_id, 
            &device_hash
        ).await?;
        
        // Get all watcher groups
        let groups = match self.app_state.storage.get_watcher_groups(&user_id).await {
            Ok(groups) => {
                // Convert each group to WatcherGroupData
                let mut group_data = Vec::new();
                for group in groups {
                    // group.group_id (클라이언트 ID)를 사용해야 함
                    if let Ok(Some(data)) = self.app_state.storage.get_watcher_group_by_account_and_id(&user_id, group.group_id).await {
                        group_data.push(data);
                    }
                }
                group_data
            },
            Err(e) => {
                error!("Error getting watcher groups: {}", e);
                return Err(Status::internal("Error getting watcher groups"));
            }
        };
        
        let response = GetWatcherGroupsResponse {
            success: true,
            watchergroup: groups,
            return_message: String::new(),
        };
        
        Ok(Response::new(response))
    }
    
    /// Handle register watcher preset request
    pub async fn register_watcher_preset(&self, request: Request<RegisterWatcherPresetRequest>) -> Result<Response<RegisterWatcherPresetResponse>, Status> {
        let req = request.into_inner();
        // 토큰으로 검증된 account_hash를 우선 사용
        let verified = self.app_state.oauth.verify_token(&req.auth_token).await
            .map_err(|_| Status::unauthenticated("Invalid authentication token"))?;
        let account_hash = verified.account_hash;
        let device_hash = req.device_hash;
        let presets = req.presets.clone();
        
        debug!("Processing register watcher preset for user: {}, device: {}", account_hash, device_hash);
        
        // Validate device if required
        auth::validate_device_if_required(
            self.app_state.storage.as_ref(), 
            &account_hash, 
            &device_hash
        ).await?;
        
        // 프리셋 저장
        if let Err(e) = self.app_state.storage.register_watcher_preset_proto(&account_hash, &device_hash, presets.clone()).await {
            error!("Error registering watcher preset: {}", e);
            return Err(Status::internal("Error registering watcher preset"));
        }
        
        // 브로드캐스트 알림 전송 (다른 장치들에게)
        if let Err(e) = self.app_state.broadcast_watcher_preset_update(
            &account_hash,
            &device_hash,
            presets,
            crate::sync::watcher_preset_update_notification::UpdateType::Created,
        ).await {
            warn!("Failed to broadcast watcher preset update: {}", e);
        }
        
        let response = RegisterWatcherPresetResponse {
            success: true,
            return_message: String::new(),
        };
        
        Ok(Response::new(response))
    }
    
    /// Handle update watcher preset request
    pub async fn update_watcher_preset(&self, request: Request<UpdateWatcherPresetRequest>) -> Result<Response<UpdateWatcherPresetResponse>, Status> {
        let req = request.into_inner();
        // 토큰으로 검증된 account_hash를 우선 사용
        let verified = self.app_state.oauth.verify_token(&req.auth_token).await
            .map_err(|_| Status::unauthenticated("Invalid authentication token"))?;
        let account_hash = verified.account_hash;
        let device_hash = req.device_hash;
        let presets = req.presets.clone();
        
        debug!("Processing update watcher preset for user: {}, device: {}", account_hash, device_hash);
        
        // Validate device if required
        auth::validate_device_if_required(
            self.app_state.storage.as_ref(), 
            &account_hash, 
            &device_hash
        ).await?;
        
        // 프리셋 업데이트
        if let Err(e) = self.app_state.storage.update_watcher_preset_proto(&account_hash, &device_hash, presets.clone()).await {
            error!("Error updating watcher preset: {}", e);
            return Err(Status::internal("Error updating watcher preset"));
        }
        
        // 브로드캐스트 알림 전송 (다른 장치들에게)
        if let Err(e) = self.app_state.broadcast_watcher_preset_update(
            &account_hash,
            &device_hash,
            presets,
            crate::sync::watcher_preset_update_notification::UpdateType::Updated,
        ).await {
            warn!("Failed to broadcast watcher preset update: {}", e);
        }
        
        let response = UpdateWatcherPresetResponse {
            success: true,
            return_message: String::new(),
        };
        
        Ok(Response::new(response))
    }
    
    /// Handle get watcher preset request
    pub async fn get_watcher_preset(&self, request: Request<GetWatcherPresetRequest>) -> Result<Response<GetWatcherPresetResponse>, Status> {
        let req = request.into_inner();
        // 토큰으로 검증된 account_hash를 우선 사용
        let verified = self.app_state.oauth.verify_token(&req.auth_token).await
            .map_err(|_| Status::unauthenticated("Invalid authentication token"))?;
        let account_hash = verified.account_hash;
        let device_hash = req.device_hash;
        
        debug!("Processing get watcher preset for user: {}, device: {}", account_hash, device_hash);
        
        // Validate device if required
        auth::validate_device_if_required(
            self.app_state.storage.as_ref(), 
            &account_hash, 
            &device_hash
        ).await?;
        
        // 프리셋 조회
        let presets = match self.app_state.storage.get_watcher_preset(&account_hash).await {
            Ok(presets) => presets,
            Err(_) => Vec::new(),
        };
        
        let response = GetWatcherPresetResponse {
            success: true,
            presets,
            return_message: String::new(),
        };
        
        Ok(Response::new(response))
    }
    
    /// Handle register watcher group request
    pub async fn register_watcher_group(&self, request: Request<RegisterWatcherGroupRequest>) -> Result<Response<RegisterWatcherGroupResponse>, Status> {
        let req = request.into_inner();
        let account_hash = req.account_hash;
        let device_hash = req.device_hash;
        let group_id = req.group_id;
        
        debug!("Processing register watcher group for user: {}, device: {}", account_hash, device_hash);
        debug!("Register watcher group request data: group_id={}, title={}", group_id, req.title);
        if let Some(ref watcher_data) = req.watcher_data {
            debug!("Watcher data included: folder={}, recursive={}", watcher_data.folder, watcher_data.recursive_path);
        } else {
            debug!("No watcher data included in request");
        }
        
        // Validate device if required
        auth::validate_device_if_required(
            self.app_state.storage.as_ref(), 
            &account_hash, 
            &device_hash
        ).await?;
        
        debug!("Registering watcher group: group_id={}, title={}", req.group_id, req.title);
        
        // 먼저 WatcherGroup을 생성
        let group = WatcherGroup {
            id: 0, // 서버에서 AUTO_INCREMENT로 생성
            group_id: req.group_id, // 클라이언트 그룹 ID
            title: req.title.clone(),
            account_hash: account_hash.clone(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            is_active: true,
            watcher_ids: Vec::new(),
        };
        
        // Optional: validate watcher folder (reject numeric-only segments unless whitelisted)
        if let Some(watcher_data) = &req.watcher_data {
            if let Err(msg) = crate::utils::validator::validate_watcher_folder(&watcher_data.folder) {
                warn!("Invalid watcher folder '{}': {}", watcher_data.folder, msg);
                return Err(Status::invalid_argument(msg));
            }
        }

        // Register watcher group in storage first
        let registered_group_id = match self.app_state.storage.register_watcher_group(&account_hash, &device_hash, &group).await {
            Ok(id) => {
                info!("Watcher group registered successfully: account_hash={}, group_id={}, server_db_id={}", account_hash, req.group_id, id);
                id
            },
            Err(e) => {
                error!("Failed to register watcher group: account_hash={}, group_id={}, error={}", account_hash, req.group_id, e);
                return Err(Status::internal(format!("Failed to register watcher group: {}", e)));
            }
        };

        // watcher 생성 (있다면)
        if let Some(watcher_data) = req.watcher_data {
            info!("Creating watcher: folder={}, recursive={}", watcher_data.folder, watcher_data.recursive_path);
            let timestamp = Utc::now().timestamp();
            let _watcher_id = self.app_state.storage
                .create_watcher_with_conditions(&account_hash, registered_group_id, &watcher_data, timestamp)
                .await
                .map_err(|e| {
                    error!("Failed to create watcher: folder={}, error={}", watcher_data.folder, e);
                    Status::internal(format!("Failed to create watcher: {}", e))
                })?;
            info!("Watcher created successfully: folder={}", watcher_data.folder);
        }

        debug!("Successfully completed watcher setup");
        
        // 클라이언트에게는 원본 group_id를 반환 (서버 DB ID가 아님!)
        let client_group_id = req.group_id;
        
        // Broadcast notification about the new watcher group  
        debug!("Broadcasting watcher group update notification");
        if let Err(e) = self.app_state.broadcast_watcher_group_update(
            &account_hash,
            &device_hash,
            registered_group_id, // broadcast에는 서버 DB ID 사용
            crate::sync::watcher_group_update_notification::UpdateType::Created,
        ).await {
            warn!("Failed to broadcast watcher group update: {}", e);
        } else {
            debug!("Successfully broadcast notification");
        }
        
        let response = RegisterWatcherGroupResponse {
            success: true,
            group_id: client_group_id, // 클라이언트에게는 원본 group_id 반환
            return_message: String::new(),
        };
        
        debug!("Returning successful response for register watcher group: client_group_id={}, server_db_id={}", client_group_id, registered_group_id);
        Ok(Response::new(response))
    }
    
    /// Handle update watcher group request
    pub async fn update_watcher_group(&self, request: Request<UpdateWatcherGroupRequest>) -> Result<Response<UpdateWatcherGroupResponse>, Status> {
        let req = request.into_inner();
        let account_hash = req.account_hash;
        let device_hash = req.device_hash;
        let group_id = req.group_id;
        let title = req.title;
        
        debug!("Processing update watcher group for user: {}, device: {}", account_hash, device_hash);
        
        // Validate device if required
        auth::validate_device_if_required(
            self.app_state.storage.as_ref(), 
            &account_hash, 
            &device_hash
        ).await?;
        
        // 기존 그룹 가져오기
        let existing_group = match self.app_state.storage.get_user_watcher_group(&account_hash, group_id).await {
            Ok(Some(group)) => group,
            Ok(None) => {
                return Err(Status::not_found(format!("Watcher group {} not found", group_id)));
            },
            Err(e) => {
                error!("Error getting watcher group: {}", e);
                return Err(Status::internal("Error getting watcher group"));
            }
        };
        
        // watcher_data에서 watcher를 생성 또는 업데이트
        let mut watcher_ids = existing_group.watcher_ids.clone();
        
        if let Some(watcher_data) = req.watcher_data {
            // 워처 생성 또는 조회
            let watcher_id = match self.app_state.create_or_get_watcher(
                &account_hash, 
                group_id, 
                &watcher_data
            ).await {
                Ok(id) => id,
                Err(e) => {
                    error!("Failed to create/get watcher: {}", e);
                    return Err(Status::internal(format!("Failed to create watcher: {}", e)));
                }
            };
            
            // 워처 ID가 리스트에 없으면 추가
            if !watcher_ids.contains(&watcher_id) {
                watcher_ids.push(watcher_id);
            }
            // 조건이 동봉되었다면 저장(save_watcher_conditions) 보장
            use crate::models::watcher::{WatcherCondition, ConditionType};
            let now = chrono::Utc::now();
            let mut conditions: Vec<WatcherCondition> = Vec::new();
            for cd in &watcher_data.union_conditions {
                conditions.push(WatcherCondition {
                    id: None,
                    account_hash: account_hash.clone(),
                    watcher_id,
                    local_watcher_id: watcher_data.watcher_id,
                    local_group_id: group_id,
                    condition_type: ConditionType::Union,
                    key: cd.key.clone(),
                    value: cd.value.clone(),
                    operator: "equals".to_string(),
                    created_at: now,
                    updated_at: now,
                });
            }
            for cd in &watcher_data.subtracting_conditions {
                conditions.push(WatcherCondition {
                    id: None,
                    account_hash: account_hash.clone(),
                    watcher_id,
                    local_watcher_id: watcher_data.watcher_id,
                    local_group_id: group_id,
                    condition_type: ConditionType::Subtract,
                    key: cd.key.clone(),
                    value: cd.value.clone(),
                    operator: "equals".to_string(),
                    created_at: now,
                    updated_at: now,
                });
            }
            // Guard: skip overwriting when both arrays are empty
            if !(watcher_data.union_conditions.is_empty() && watcher_data.subtracting_conditions.is_empty()) {
                if let Err(e) = self.app_state.storage.save_watcher_conditions(watcher_id, &conditions).await {
                    error!("Failed to save watcher conditions: {}", e);
                    return Err(Status::internal("Failed to save watcher conditions"));
                }
            } else {
                debug!("Skip saving empty conditions to preserve existing watcher conditions: watcher_id={}", watcher_id);
            }
        }
        
        // UpdateWatcherGroupRequest 에서 WatcherGroup 생성
        let watcher_group = WatcherGroup {
            id: group_id,
            group_id: existing_group.group_id, // 기존 group_id 유지
            account_hash: account_hash.clone(),
            title,
            created_at: existing_group.created_at,
            updated_at: Utc::now(),
            is_active: true,
            watcher_ids,
        };
        
        // Update watcher group in storage
        match self.app_state.storage.update_watcher_group(&account_hash, &watcher_group).await {
            Ok(_) => {},
            Err(e) => {
                error!("Failed to update watcher group: {}", e);
                return Err(Status::internal(format!("Failed to update watcher group: {}", e)));
            }
        };
        
        // Broadcast notification about the updated watcher group
        if let Err(e) = self.app_state.broadcast_watcher_group_update(
            &account_hash,
            &device_hash,
            group_id,
            crate::sync::watcher_group_update_notification::UpdateType::Created,
        ).await {
            warn!("Failed to broadcast watcher group update: {}", e);
        }
        
        let response = UpdateWatcherGroupResponse {
            success: true,
            group_id,
            return_message: String::new(),
        };
        
        Ok(Response::new(response))
    }
    
    /// Handle delete watcher group request
    pub async fn delete_watcher_group(&self, request: Request<DeleteWatcherGroupRequest>) -> Result<Response<DeleteWatcherGroupResponse>, Status> {
        let req = request.into_inner();
        let account_hash = req.account_hash;
        let device_hash = req.device_hash;
        let group_id = req.group_id;
        
        debug!("Processing delete watcher group for user: {}, device: {}, group: {}", account_hash, device_hash, group_id);
        
        // Validate device if required
        auth::validate_device_if_required(
            self.app_state.storage.as_ref(), 
            &account_hash, 
            &device_hash
        ).await?;
        
        // 삭제 전 그룹 정보 저장 (알림용)
        let group_data = match self.app_state.storage.get_watcher_group_by_account_and_id(&account_hash, group_id).await {
            Ok(Some(data)) => data,
            Ok(None) => {
                return Err(Status::not_found(format!("Watcher group {} not found", group_id)));
            },
            Err(e) => {
                error!("Error getting watcher group: {}", e);
                return Err(Status::internal("Error getting watcher group"));
            }
        };
        
        // Delete the watcher group
        if let Err(e) = self.app_state.storage.delete_watcher_group(&account_hash, group_id).await {
            error!("Error deleting watcher group: {}", e);
            return Err(Status::internal("Error deleting watcher group"));
        }
        
        // 삭제 알림 브로드캐스트
        let notification = crate::sync::WatcherGroupUpdateNotification {
            account_hash: account_hash.to_string(),
            device_hash: device_hash.to_string(),
            group_data: Some(group_data),
            update_type: crate::sync::watcher_group_update_notification::UpdateType::Deleted as i32,
            timestamp: chrono::Utc::now().timestamp(),
        };
        
        if let Err(e) = self.app_state.notification_manager.broadcast_watcher_group_update(
            &account_hash,
            Some(&device_hash),
            notification
        ).await {
            warn!("Failed to broadcast watcher group deletion: {}", e);
        }
        
        let response = DeleteWatcherGroupResponse {
            success: true,
            group_id,
            return_message: String::new(),
        };
        
        Ok(Response::new(response))
    }

    /// Handle integrated configuration synchronization
    pub async fn sync_configuration(&self, request: Request<SyncConfigurationRequest>) -> Result<Response<SyncConfigurationResponse>, Status> {
        let req = request.into_inner();
        // 토큰으로 검증된 account_hash를 우선 사용
        let verified = self.app_state.oauth.verify_token(&req.auth_token).await
            .map_err(|_| Status::unauthenticated("Invalid authentication token"))?;
        let account_hash = verified.account_hash;
        let device_hash = req.device_hash.clone();
        let client_watcher_groups = req.watcher_groups;
        let client_presets = req.presets;
        let incremental = req.incremental;
        let force_update = req.force_update;
        let client_timestamp = req.client_timestamp;
        
        let sync_start = std::time::Instant::now();
        
        debug!("Processing integrated configuration sync for user: {}, device: {}, incremental: {}, force: {}", 
               account_hash, device_hash, incremental, force_update);
        
        // Validate device if required
        auth::validate_device_if_required(
            self.app_state.storage.as_ref(), 
            &account_hash, 
            &device_hash
        ).await?;
        
        let mut stats = SyncStats {
            groups_updated: 0,
            groups_created: 0,
            groups_deleted: 0,
            presets_updated: 0,
            sync_timestamp: chrono::Utc::now().timestamp(),
            total_operations: 0,
            sync_duration_ms: 0.0,
        };
        
        let mut conflicts_detected = false;
        let mut conflict_details = Vec::new();
        
        // 1. Watcher Groups 동기화
        for group_data in client_watcher_groups {
            let group_id = group_data.group_id;
            
            // 기존 그룹 확인
            let existing_group = self.app_state.storage.get_user_watcher_group(&account_hash, group_id).await;
            
            match existing_group {
                Ok(Some(existing)) => {
                    // 기존 그룹 업데이트
                    if force_update || !incremental {
                        // create WatcherGroup object
                        let updated_group = WatcherGroup {
                            id: existing.id,
                            group_id: existing.group_id,
                            account_hash: account_hash.clone(),
                            title: if group_data.title.is_empty() { existing.title.clone() } else { group_data.title.clone() },
                            created_at: existing.created_at,
                            updated_at: Utc::now(),
                            is_active: true,
                            watcher_ids: existing.watcher_ids,
                        };
                        
                        if let Err(e) = self.app_state.storage.update_watcher_group(&account_hash, &updated_group).await {
                            warn!("Failed to update watcher group {}: {}", group_id, e);
                            conflict_details.push(format!("Failed to update group {}: {}", group_id, e));
                            conflicts_detected = true;
                        } else {
                            stats.groups_updated += 1;
                            stats.total_operations += 1;
                            
                            // 그룹 내 전달된 watcher 들 반영(조건 포함)
                            for watcher in group_data.watchers {
                                match self.app_state.create_or_get_watcher(&account_hash, group_id, &watcher).await {
                                    Ok(wid) => {
                                        // 이미 create_or_get 내에서 조건 저장 수행
                                        debug!("Applied watcher {} for group {}", wid, group_id);
                                    }
                                    Err(e) => {
                                        warn!("Failed to upsert watcher for group {}: {}", group_id, e);
                                        conflict_details.push(format!("Watcher upsert failed in group {}: {}", group_id, e));
                                        conflicts_detected = true;
                                    }
                                }
                            }
                            
                            // send real-time notification
                            if let Err(e) = self.app_state.broadcast_watcher_group_update(
                                &account_hash,
                                &device_hash,
                                group_id,
                                crate::sync::watcher_group_update_notification::UpdateType::Updated,
                            ).await {
                                warn!("Failed to broadcast watcher group update: {}", e);
                            }
                        }
                    }
                },
                Ok(None) => {
                    // create new group
                    let new_group = WatcherGroup {
                        id: 0, // AUTO_INCREMENT
                        group_id,
                        account_hash: account_hash.clone(),
                        title: if group_data.title.is_empty() { "Synced Group".to_string() } else { group_data.title.clone() },
                        created_at: Utc::now(),
                        updated_at: Utc::now(),
                        is_active: true,
                        watcher_ids: Vec::new(),
                    };
                    
                    if let Err(e) = self.app_state.storage.register_watcher_group(&account_hash, &device_hash, &new_group).await {
                        warn!("Failed to create watcher group {}: {}", group_id, e);
                        conflict_details.push(format!("Failed to create group {}: {}", group_id, e));
                        conflicts_detected = true;
                    } else {
                        stats.groups_created += 1;
                        stats.total_operations += 1;
                        
                        // 그룹 내 전달된 watcher 들 반영(조건 포함)
                        for watcher in group_data.watchers {
                            match self.app_state.create_or_get_watcher(&account_hash, group_id, &watcher).await {
                                Ok(wid) => {
                                    debug!("Created watcher {} for new group {}", wid, group_id);
                                }
                                Err(e) => {
                                    warn!("Failed to create watcher for new group {}: {}", group_id, e);
                                    conflict_details.push(format!("Watcher create failed in new group {}: {}", group_id, e));
                                    conflicts_detected = true;
                                }
                            }
                        }
                        
                        // send real-time notification
                        if let Err(e) = self.app_state.broadcast_watcher_group_update(
                            &account_hash,
                            &device_hash,
                            group_id,
                            crate::sync::watcher_group_update_notification::UpdateType::Created,
                        ).await {
                            warn!("Failed to broadcast watcher group creation: {}", e);
                        }
                    }
                },
                Err(e) => {
                    warn!("Error checking existing group {}: {}", group_id, e);
                    conflict_details.push(format!("Error checking group {}: {}", group_id, e));
                    conflicts_detected = true;
                }
            }
        }
        
        // 2. Watcher Presets 동기화
        if !client_presets.is_empty() {
            match self.app_state.storage.register_watcher_preset_proto(&account_hash, &device_hash, client_presets.clone()).await {
                Ok(_) => {
                    stats.presets_updated = 1;
                    stats.total_operations += 1;
                    info!("Successfully synced {} presets from client", client_presets.len());
                    
                    // send real-time notification
                    if let Err(e) = self.app_state.broadcast_watcher_preset_update(
                        &account_hash,
                        &device_hash,
                        client_presets.clone(),
                        crate::sync::watcher_preset_update_notification::UpdateType::Updated,
                    ).await {
                        warn!("Failed to broadcast preset update: {}", e);
                    }
                },
                Err(e) => {
                    warn!("Failed to sync presets: {}", e);
                    conflict_details.push(format!("Failed to sync presets: {}", e));
                    conflicts_detected = true;
                }
            }
        }
        
        // 3. get latest server state
        let server_groups = self.app_state.storage.get_watcher_groups(&account_hash).await
            .unwrap_or_default();
        let mut tasks = futures::stream::FuturesUnordered::new();
        for group in server_groups.into_iter() {
            let storage = self.app_state.storage.clone();
            let account = account_hash.clone();
            tasks.push(async move {
                storage.get_watcher_group_by_account_and_id(&account, group.group_id).await.ok().flatten()
            });
        }
        let mut server_watcher_groups = Vec::new();
        while let Some(item) = tasks.next().await {
            if let Some(data) = item { server_watcher_groups.push(data); }
        }
            
        let server_presets = self.app_state.storage.get_watcher_preset(&account_hash).await
            .unwrap_or_default();
        
        // complete statistics
        stats.sync_duration_ms = sync_start.elapsed().as_millis() as f64;
        
        let response = SyncConfigurationResponse {
            success: !conflicts_detected,
            return_message: if conflicts_detected { 
                format!("Partial sync completed with {} conflicts", conflict_details.len())
            } else { 
                "Configuration sync completed successfully".to_string() 
            },
            stats: Some(stats),
            server_watcher_groups,
            server_presets,
            server_timestamp: chrono::Utc::now().timestamp(),
            conflicts_detected,
            conflict_details,
        };
        
        info!("Integrated configuration sync completed for user: {}, operations: {}, duration: {:.2}ms", 
              account_hash, response.stats.as_ref().map(|s| s.total_operations).unwrap_or_default(), response.stats.as_ref().map(|s| s.sync_duration_ms).unwrap_or_default());
        
        Ok(Response::new(response))
    }
}

#[async_trait]
impl Handler for WatcherHandler {
    // define streaming return type
    type SubscribeToAuthUpdatesStream = std::pin::Pin<Box<dyn futures::Stream<Item = Result<AuthUpdateNotification, Status>> + Send + 'static>>;
    type SubscribeToDeviceUpdatesStream = std::pin::Pin<Box<dyn futures::Stream<Item = Result<DeviceUpdateNotification, Status>> + Send + 'static>>;
    type SubscribeToEncryptionKeyUpdatesStream = std::pin::Pin<Box<dyn futures::Stream<Item = Result<EncryptionKeyUpdateNotification, Status>> + Send + 'static>>;
    type SubscribeToFileUpdatesStream = std::pin::Pin<Box<dyn futures::Stream<Item = Result<FileUpdateNotification, Status>> + Send + 'static>>;
    type SubscribeToWatcherPresetUpdatesStream = std::pin::Pin<Box<dyn futures::Stream<Item = Result<WatcherPresetUpdateNotification, Status>> + Send + 'static>>;
    type SubscribeToWatcherGroupUpdatesStream = std::pin::Pin<Box<dyn futures::Stream<Item = Result<WatcherGroupUpdateNotification, Status>> + Send + 'static>>;
    type SubscribeToVersionUpdatesStream = std::pin::Pin<Box<dyn futures::Stream<Item = Result<VersionUpdateNotification, Status>> + Send + 'static>>;

    async fn handle_register_watcher_preset(
        &self,
        request: Request<RegisterWatcherPresetRequest>,
    ) -> Result<Response<RegisterWatcherPresetResponse>, Status> {
        self.register_watcher_preset(request).await
    }
    
    async fn handle_update_watcher_preset(
        &self,
        request: Request<UpdateWatcherPresetRequest>,
    ) -> Result<Response<UpdateWatcherPresetResponse>, Status> {
        self.update_watcher_preset(request).await
    }
    
    async fn handle_get_watcher_preset(
        &self,
        request: Request<GetWatcherPresetRequest>,
    ) -> Result<Response<GetWatcherPresetResponse>, Status> {
        self.get_watcher_preset(request).await
    }
    
    async fn handle_register_watcher_group(
        &self,
        request: Request<RegisterWatcherGroupRequest>,
    ) -> Result<Response<RegisterWatcherGroupResponse>, Status> {
        self.register_watcher_group(request).await
    }
    
    async fn handle_update_watcher_group(
        &self,
        request: Request<UpdateWatcherGroupRequest>,
    ) -> Result<Response<UpdateWatcherGroupResponse>, Status> {
        self.update_watcher_group(request).await
    }
    
    async fn handle_delete_watcher_group(
        &self,
        request: Request<DeleteWatcherGroupRequest>,
    ) -> Result<Response<DeleteWatcherGroupResponse>, Status> {
        self.delete_watcher_group(request).await
    }
    
    async fn handle_get_watcher_group(
        &self,
        request: Request<GetWatcherGroupRequest>,
    ) -> Result<Response<GetWatcherGroupResponse>, Status> {
        self.get_watcher_group(request).await
    }
    
    async fn handle_get_watcher_groups(
        &self,
        request: Request<GetWatcherGroupsRequest>,
    ) -> Result<Response<GetWatcherGroupsResponse>, Status> {
        self.get_watcher_groups(request).await
    }
    
    async fn handle_sync_configuration(
        &self,
        request: Request<SyncConfigurationRequest>,
    ) -> Result<Response<SyncConfigurationResponse>, Status> {
        self.sync_configuration(request).await
    }
} 