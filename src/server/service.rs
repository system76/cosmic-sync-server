use std::sync::Arc;
use tonic::{Request, Response, Status};
use futures::StreamExt;
pub use crate::sync::sync_service_server::SyncService;
use crate::sync::sync_client_service_server::SyncClientService;
use crate::sync::{
    OAuthExchangeRequest, OAuthExchangeResponse,
    RegisterDeviceRequest, RegisterDeviceResponse,
    ListDevicesRequest, ListDevicesResponse,
    DeleteDeviceRequest, DeleteDeviceResponse,
    RequestEncryptionKeyRequest, RequestEncryptionKeyResponse,
    UploadFileRequest, UploadFileResponse,
    DownloadFileRequest, DownloadFileResponse,
    ListFilesRequest, ListFilesResponse,
    DeleteFileRequest, DeleteFileResponse,
    HealthCheckRequest, HealthCheckResponse,
    UpdateDeviceInfoRequest, UpdateDeviceInfoResponse,
    ValidateTokenRequest, ValidateTokenResponse,
    LoginRequest, LoginResponse,
    VerifyLoginRequest, VerifyLoginResponse,
    CheckAuthStatusRequest, CheckAuthStatusResponse,
    RegisterWatcherPresetRequest, RegisterWatcherPresetResponse,
    UpdateWatcherPresetRequest, UpdateWatcherPresetResponse,
    GetWatcherPresetRequest, GetWatcherPresetResponse,
    RegisterWatcherGroupRequest, RegisterWatcherGroupResponse,
    UpdateWatcherGroupRequest, UpdateWatcherGroupResponse,
    DeleteWatcherGroupRequest, DeleteWatcherGroupResponse,
    GetWatcherGroupRequest, GetWatcherGroupResponse,
    SyncConfigurationRequest, SyncConfigurationResponse,
    AuthSuccessNotification, AuthNotificationResponse,
    GetWatcherGroupsRequest, GetWatcherGroupsResponse,
    GetFileHistoryRequest, GetFileHistoryResponse,
    RestoreFileVersionRequest, RestoreFileVersionResponse,
    BroadcastFileRestoreRequest, BroadcastFileRestoreResponse,
    VersionUpdateNotification,
    SubscribeRequest,
    AuthUpdateNotification,
    DeviceUpdateNotification,
    EncryptionKeyUpdateNotification,
    FileUpdateNotification,
    WatcherPresetUpdateNotification,
    WatcherGroupUpdateNotification,
    GetAccountInfoRequest,
    GetAccountInfoResponse,
    FindFileRequest,
    FindFileResponse,
    CheckFileExistsRequest,
    CheckFileExistsResponse,
};
use crate::services::{Handler};
use crate::handlers::{
    AuthHandler,
    DeviceHandler,
    FileHandler,
    WatcherHandler,
    SyncHandler,
};
use crate::server::app_state::AppState;
use tracing::{debug, error, info, warn};
use std::pin::Pin;
use futures::Stream;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use crate::auth::oauth::OAuthService;

/// Synchronization service implementation
pub struct SyncServiceImpl {
    /// Application state
    pub app_state: Arc<AppState>,
    /// OAuth service
    oauth: Arc<OAuthService>,
    /// Authentication handler
    auth_handler: AuthHandler,
    /// Device handler
    device_handler: DeviceHandler,
    /// File handler
    file_handler: FileHandler,
    /// Watcher handler
    watcher_handler: WatcherHandler,
    /// Sync handler
    sync_handler: SyncHandler,
}

impl SyncServiceImpl {
    /// Create a new SyncServiceImpl instance
    pub fn new(app_state: Arc<AppState>) -> Self {
        let auth_handler = AuthHandler::new(app_state.clone());
        
        let device_handler = DeviceHandler::new(app_state.clone());
        
        let file_handler = FileHandler::new(app_state.clone());
        
        let watcher_handler = WatcherHandler::new(app_state.clone());
        
        let sync_handler = SyncHandler::new(app_state.clone());
        
        Self {
            oauth: Arc::new(app_state.oauth.clone()),
            app_state,
            auth_handler,
            device_handler,
            file_handler,
            watcher_handler,
            sync_handler,
        }
    }

    async fn validate_auth(&self, auth_token: &str, account_hash: &str) -> Result<(), Status> {
        debug!("Auth validation started: account_hash={}, token_length={}", account_hash, auth_token.len());
        
        if auth_token.is_empty() {
            error!("Empty auth token provided for account: {}", account_hash);
            return Err(Status::unauthenticated("Empty authentication token"));
        }
        
        // oauth를 통해 토큰 검증
        match self.app_state.oauth.verify_token(auth_token).await {
            Ok(auth_result) => {
                debug!("Token verification result: valid={}, token_account={}, expected_account={}", 
                       auth_result.valid, auth_result.account_hash, account_hash);
                
                if auth_result.valid && auth_result.account_hash == account_hash {
                    debug!("Auth validation successful for account: {}", account_hash);
                    Ok(())
                } else {
                    error!("Auth validation failed: token valid={}, expected_account={}, actual_account={}", 
                           auth_result.valid, account_hash, auth_result.account_hash);
                    Err(Status::unauthenticated("Invalid authentication"))
                }
            },
            Err(e) => {
                error!("Token verification error for account {}: {}", account_hash, e);
                Err(Status::unauthenticated("Invalid authentication"))
            }
        }
    }
    
    /// 새로 구독한 클라이언트에게 기존 파일들에 대한 초기 동기화 알림 전송 (개선된 버전)
    async fn send_initial_file_sync(&self, account_hash: &str, device_hash: &str) -> Result<(), Status> {
        info!("🔄 Starting enhanced initial file sync for device: {}:{}", account_hash, device_hash);
        
        // 연결 상태 업데이트
        let connection_key = format!("{}:{}", account_hash, device_hash);
        self.app_state.connection_tracker.update_sync_time(&connection_key).await;
        
        // 해당 계정의 모든 활성 파일 조회
                        let files = match self.app_state.storage.list_files(account_hash, 1, None).await {
            Ok(files) => files,
            Err(e) => {
                error!("Failed to list files for initial sync: {}", e);
                return Err(Status::internal("Failed to retrieve files for initial sync"));
            }
        };
        
        let mut sync_count = 0;
        let mut skip_count = 0;
        let subscriber_key = format!("{}:{}", account_hash, device_hash);
        
        // 배치 크기 설정 (한 번에 너무 많은 알림을 보내지 않도록)
        const BATCH_SIZE: usize = 50;
        let total_files = files.len();
        
        // 각 파일에 대해 배치 처리로 개별 알림 전송
        for (batch_idx, batch) in files.chunks(BATCH_SIZE).enumerate() {
            debug!("📦 Processing batch {}/{} ({} files)", 
                   batch_idx + 1, (total_files + BATCH_SIZE - 1) / BATCH_SIZE, batch.len());
            
            for file in batch {
                                        // Skip files that are logically deleted (check by status or other field if available)
                        // Note: is_deleted field not available in current FileInfo struct
                
                // 같은 장치에서 업로드된 파일은 제외 (클라이언트가 이미 가지고 있음)
                if file.device_hash == device_hash {
                    skip_count += 1;
                    debug!("⏭️ Skipping file {} (same device: {})", file.filename, device_hash);
                    continue;
                }
                
                                        // 파일 업데이트 알림 생성
                        let file_info = crate::sync::FileInfo {
                            file_id: file.file_id as u64,
                            filename: file.filename.clone(),
                            file_hash: file.file_hash.clone(),
                            device_hash: file.device_hash.clone(),
                            group_id: file.group_id,
                            watcher_id: file.watcher_id,
                            file_path: file.file_path.clone(),
                            file_size: file.size as u64,
                            revision: file.revision,
                            is_encrypted: file.is_encrypted,
                            updated_time: Some(prost_types::Timestamp {
                                seconds: file.updated_time.seconds,
                                nanos: file.updated_time.nanos,
                            }),
                        };
                        
                        let file_update_notification = crate::sync::FileUpdateNotification {
                            account_hash: account_hash.to_string(),
                            device_hash: file.device_hash.clone(),
                            file_info: Some(file_info),
                            update_type: crate::sync::file_update_notification::UpdateType::Uploaded as i32,
                            timestamp: file.updated_time.seconds,
                        };
                
                                        // 특정 구독자에게만 전송 (새로 연결된 클라이언트)
                        if let Some(sender) = {
                            let subscribers = self.app_state.notification_manager.get_file_update_subscribers().lock().await;
                            subscribers.get(&subscriber_key).cloned()
                        } {
                    match sender.send(Ok(file_update_notification)).await {
                        Ok(_) => {
                            sync_count += 1;
                            debug!("📤 Initial sync notification sent for file: {} ({})", file.filename, file.file_id);
                        },
                        Err(e) => {
                            warn!("Failed to send initial sync notification for file {}: {}", file.filename, e);
                            break; // 전송 실패 시 이 배치 중단
                        }
                    }
                } else {
                    warn!("Subscriber {} not found during initial sync", subscriber_key);
                    break; // 구독자가 없으면 중단
                }
            }
            
            // 배치 간 짧은 지연 (채널이 막히는 것을 방지)
            if batch_idx + 1 < (total_files + BATCH_SIZE - 1) / BATCH_SIZE {
                tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
            }
        }
        
        info!("🎉 Enhanced initial file sync completed: {} files synced, {} skipped, total processed: {} for {}:{}", 
              sync_count, skip_count, total_files, account_hash, device_hash);
        Ok(())
    }
}

#[tonic::async_trait]
impl SyncService for SyncServiceImpl {
    // 연관 타입 명시적 정의
    type SubscribeToAuthUpdatesStream = Pin<Box<dyn Stream<Item = Result<AuthUpdateNotification, Status>> + Send + 'static>>;
    type SubscribeToDeviceUpdatesStream = Pin<Box<dyn Stream<Item = Result<DeviceUpdateNotification, Status>> + Send + 'static>>;
    type SubscribeToEncryptionKeyUpdatesStream = Pin<Box<dyn Stream<Item = Result<EncryptionKeyUpdateNotification, Status>> + Send + 'static>>;
    type SubscribeToFileUpdatesStream = Pin<Box<dyn Stream<Item = Result<FileUpdateNotification, Status>> + Send + 'static>>;
    type SubscribeToWatcherPresetUpdatesStream = Pin<Box<dyn Stream<Item = Result<WatcherPresetUpdateNotification, Status>> + Send + 'static>>;
    type SubscribeToWatcherGroupUpdatesStream = Pin<Box<dyn Stream<Item = Result<WatcherGroupUpdateNotification, Status>> + Send + 'static>>;

    // OAuth related methods
    async fn exchange_oauth_code(
        &self,
        request: Request<OAuthExchangeRequest>,
    ) -> Result<Response<OAuthExchangeResponse>, Status> {
        debug!("OAuth code exchange request received");
        self.auth_handler.handle_oauth_exchange(request).await
    }
    
    // Device related methods
    async fn register_device(
        &self,
        request: Request<RegisterDeviceRequest>,
    ) -> Result<Response<RegisterDeviceResponse>, Status> {
        debug!("Device registration request received");
        self.device_handler.handle_register_device(request).await
    }
    
    async fn list_devices(
        &self,
        request: Request<ListDevicesRequest>,
    ) -> Result<Response<ListDevicesResponse>, Status> {
        debug!("Device list request received");
        self.device_handler.handle_list_devices(request).await
    }
    
    async fn delete_device(
        &self,
        request: Request<DeleteDeviceRequest>,
    ) -> Result<Response<DeleteDeviceResponse>, Status> {
        debug!("Device deletion request received");
        self.device_handler.handle_delete_device(request).await
    }
    
    async fn update_device_info(
        &self,
        request: Request<UpdateDeviceInfoRequest>,
    ) -> Result<Response<UpdateDeviceInfoResponse>, Status> {
        debug!("Device info update request received");
        self.device_handler.handle_update_device_info(request).await
    }
    
    // Encryption key related methods
    async fn request_encryption_key(
        &self,
        request: Request<RequestEncryptionKeyRequest>,
    ) -> Result<Response<RequestEncryptionKeyResponse>, Status> {
        debug!("Encryption key request received");
        self.sync_handler.handle_request_encryption_key(request).await
    }
    
    // File related methods
    async fn upload_file(
        &self,
        request: Request<UploadFileRequest>,
    ) -> Result<Response<UploadFileResponse>, Status> {
        debug!("File upload request received");
        self.file_handler.handle_upload_file(request).await
    }
    
    async fn download_file(
        &self,
        request: Request<DownloadFileRequest>,
    ) -> Result<Response<DownloadFileResponse>, Status> {
        debug!("File download request received");
        self.file_handler.handle_download_file(request).await
    }
    
    async fn list_files(
        &self,
        request: Request<ListFilesRequest>,
    ) -> Result<Response<ListFilesResponse>, Status> {
        debug!("File list request received");
        self.file_handler.handle_list_files(request).await
    }
    
    async fn delete_file(
        &self,
        request: Request<DeleteFileRequest>,
    ) -> Result<Response<DeleteFileResponse>, Status> {
        debug!("File deletion request received");
        self.file_handler.handle_delete_file(request).await
    }
    
    async fn find_file_by_criteria(
        &self,
        request: Request<FindFileRequest>,
    ) -> Result<Response<FindFileResponse>, Status> {
        debug!("Find file by criteria request received");
        self.file_handler.handle_find_file_by_criteria(request).await
    }
    
    /// CheckFileExists - 파일 존재 여부 확인 (삭제된 파일도 포함)
    async fn check_file_exists(&self, request: Request<CheckFileExistsRequest>) -> Result<Response<CheckFileExistsResponse>, Status> {
        debug!("Check file exists request received");
        self.file_handler.handle_check_file_exists(request).await
    }
    
    // WatcherPreset related methods
    async fn register_watcher_preset(
        &self,
        request: Request<RegisterWatcherPresetRequest>,
    ) -> Result<Response<RegisterWatcherPresetResponse>, Status> {
        debug!("Register watcher preset request received");
        self.watcher_handler.handle_register_watcher_preset(request).await
    }
    
    async fn update_watcher_preset(
        &self,
        request: Request<UpdateWatcherPresetRequest>,
    ) -> Result<Response<UpdateWatcherPresetResponse>, Status> {
        debug!("Update watcher preset request received");
        self.watcher_handler.handle_update_watcher_preset(request).await
    }
    
    async fn get_watcher_preset(
        &self,
        request: Request<GetWatcherPresetRequest>,
    ) -> Result<Response<GetWatcherPresetResponse>, Status> {
        debug!("Get watcher preset request received");
        self.watcher_handler.handle_get_watcher_preset(request).await
    }
    
    // WatcherGroup related methods
    async fn register_watcher_group(
        &self,
        request: Request<RegisterWatcherGroupRequest>,
    ) -> Result<Response<RegisterWatcherGroupResponse>, Status> {
        debug!("Register watcher group request received");
        self.watcher_handler.handle_register_watcher_group(request).await
    }
    
    async fn update_watcher_group(
        &self,
        request: Request<UpdateWatcherGroupRequest>,
    ) -> Result<Response<UpdateWatcherGroupResponse>, Status> {
        debug!("Update watcher group request received");
        self.watcher_handler.handle_update_watcher_group(request).await
    }
    
    async fn delete_watcher_group(
        &self,
        request: Request<DeleteWatcherGroupRequest>,
    ) -> Result<Response<DeleteWatcherGroupResponse>, Status> {
        debug!("Delete watcher group request received");
        self.watcher_handler.handle_delete_watcher_group(request).await
    }
    
    async fn get_watcher_group(
        &self,
        request: Request<GetWatcherGroupRequest>,
    ) -> Result<Response<GetWatcherGroupResponse>, Status> {
        debug!("Get watcher group request received");
        self.watcher_handler.handle_get_watcher_group(request).await
    }
    
    async fn get_watcher_groups(
        &self,
        request: Request<GetWatcherGroupsRequest>,
    ) -> Result<Response<GetWatcherGroupsResponse>, Status> {
        debug!("Get watcher groups request received");
        self.watcher_handler.handle_get_watcher_groups(request).await
    }
    
    // Health check method
    async fn health_check(
        &self,
        request: Request<HealthCheckRequest>,
    ) -> Result<Response<HealthCheckResponse>, Status> {
        debug!("Health check request received");
        self.sync_handler.handle_health_check(request).await
    }
    
    // Token validation
    async fn validate_token(
        &self,
        request: Request<ValidateTokenRequest>,
    ) -> Result<Response<ValidateTokenResponse>, Status> {
        debug!("Token validation request received");
        self.auth_handler.handle_validate_token(request).await
    }
    
    // Authentication status check
    async fn check_auth_status(
        &self,
        request: Request<CheckAuthStatusRequest>,
    ) -> Result<Response<CheckAuthStatusResponse>, Status> {
        debug!("Auth status check request received");
        
        // 요청에서 device_hash 추출 및 클론 (소유권 문제 해결)
        let device_hash = request.get_ref().device_hash.clone();
        info!("Check auth status for device_hash: {}", device_hash);
        
        // AuthHandler의 check_auth_status 호출
        let result = self.auth_handler.check_auth_status(request).await;
        
        // 응답 로깅 (성공/실패)
        match &result {
            Ok(response) => {
                let resp = response.get_ref();
                if resp.is_complete {
                    info!("Auth status check: authenticated for device_hash: {}", device_hash);
                } else {
                    info!("Auth status check: not yet authenticated for device_hash: {}", device_hash);
                }
            },
            Err(e) => {
                error!("Auth status check error: {}", e);
            }
        }
        
        result
    }
    
    // Login method
    async fn login(
        &self,
        request: Request<LoginRequest>,
    ) -> Result<Response<LoginResponse>, Status> {
        debug!("Login request received");
        self.auth_handler.handle_login(request).await
    }
    
    // Login verification
    async fn verify_login(
        &self,
        request: Request<VerifyLoginRequest>,
    ) -> Result<Response<VerifyLoginResponse>, Status> {
        debug!("Login verification request received");
        self.auth_handler.handle_verify_login(request).await
    }
    
    // 계정 정보 조회
    async fn get_account_info(
        &self,
        request: Request<GetAccountInfoRequest>,
    ) -> Result<Response<GetAccountInfoResponse>, Status> {
        debug!("Account info request received");
        self.auth_handler.handle_get_account_info(request).await
    }
    
    // 통합 설정 동기화
    async fn sync_configuration(
        &self,
        request: Request<SyncConfigurationRequest>,
    ) -> Result<Response<SyncConfigurationResponse>, Status> {
        debug!("Integrated configuration sync request received");
        self.watcher_handler.handle_sync_configuration(request).await
    }
    
    // 스트리밍 구독 메서드 구현
    async fn subscribe_to_auth_updates(
        &self,
        request: Request<SubscribeRequest>,
    ) -> Result<Response<Self::SubscribeToAuthUpdatesStream>, Status> {
        debug!("Auth updates subscription request received");
        
        let (tx, rx) = mpsc::channel(128);
        let stream = ReceiverStream::new(rx);
        
        // TODO: 실제 사용자 인증 상태 모니터링 및 업데이트 로직 구현
        tokio::spawn(async move {
            // 향후 실제 이벤트 전송 로직 구현
        });
        
        Ok(Response::new(Box::pin(stream) as Self::SubscribeToAuthUpdatesStream))
    }
    
    async fn subscribe_to_device_updates(
        &self,
        request: Request<SubscribeRequest>,
    ) -> Result<Response<Self::SubscribeToDeviceUpdatesStream>, Status> {
        debug!("Device updates subscription request received");
        
        let (tx, rx) = mpsc::channel(128);
        let stream = ReceiverStream::new(rx);
        
        // TODO: 실제 장치 업데이트 모니터링 구현
        tokio::spawn(async move {
            // 향후 실제 이벤트 전송 로직 구현
        });
        
        Ok(Response::new(Box::pin(stream) as Self::SubscribeToDeviceUpdatesStream))
    }
    
    async fn subscribe_to_encryption_key_updates(
        &self,
        request: Request<SubscribeRequest>,
    ) -> Result<Response<Self::SubscribeToEncryptionKeyUpdatesStream>, Status> {
        debug!("Encryption key updates subscription request received");
        
        let (tx, rx) = mpsc::channel(128);
        let stream = ReceiverStream::new(rx);
        
        // TODO: 실제 암호화 키 업데이트 모니터링 구현
        tokio::spawn(async move {
            // 향후 실제 이벤트 전송 로직 구현
        });
        
        Ok(Response::new(Box::pin(stream) as Self::SubscribeToEncryptionKeyUpdatesStream))
    }
    
    async fn subscribe_to_file_updates(
        &self,
        request: Request<SubscribeRequest>,
    ) -> Result<Response<Self::SubscribeToFileUpdatesStream>, Status> {
        let req = request.into_inner();
        let device_hash = req.device_hash;
        
        info!("File updates subscription request received from device_hash: {}", device_hash);
        
        // 먼저 토큰을 검증하고 실제 account_hash를 가져옴
        let auth_result = match self.app_state.oauth.verify_token(&req.auth_token).await {
            Ok(result) => {
                if !result.valid {
                    error!("Invalid auth token for device: {}", device_hash);
                    return Err(Status::unauthenticated("Invalid authentication token"));
                }
                result
            },
            Err(e) => {
                error!("Token verification error for device {}: {}", device_hash, e);
                return Err(Status::unauthenticated("Invalid authentication"));
            }
        };
        
        let account_hash = auth_result.account_hash; // 토큰에서 추출한 실제 account_hash 사용
        
        // 요청된 account_hash와 다르면 경고 로그 출력 (하지만 에러는 아님)
        if req.account_hash != account_hash {
            warn!("Client sent different account_hash (requested: {}, actual: {}) for device: {}", 
                  req.account_hash, account_hash, device_hash);
        }
        
        // 구독 채널 생성 - 더 큰 버퍼 사용
        let (tx, rx) = mpsc::channel(1024);
        let stream = ReceiverStream::new(rx);
        
        // 구독자 등록 (device_hash를 키로 사용)
        let subscriber_key = format!("{}:{}", account_hash, device_hash);
        
        // 연결 상태 추적 등록
        let connection_key = self.app_state.connection_tracker
            .register_connection(device_hash.clone(), account_hash.clone()).await;
        
        match self.app_state.notification_manager.register_file_update_subscriber(
            subscriber_key.clone(), 
            tx
        ).await {
            Ok(_) => {
                // 구독 해제를 처리하기 위한 백그라운드 작업
                let notification_manager = self.app_state.notification_manager.clone();
                let connection_tracker = self.app_state.connection_tracker.clone();
                let subscriber_key_clone = subscriber_key.clone();
                let connection_key_clone = connection_key.clone();
                tokio::spawn(async move {
                    // 클라이언트 연결이 종료되면 자동으로 구독 해제
                    // 더 긴 타임아웃 사용 (7일) - 실제로는 연결 종료 시 해제됨
                    tokio::time::sleep(tokio::time::Duration::from_secs(3600 * 24 * 7)).await;
                    
                    // 구독 해제 시도
                    info!("Unregistering file update subscriber (timeout): {}", subscriber_key_clone);
                    if let Err(e) = notification_manager.unregister_file_update_subscriber(&subscriber_key_clone).await {
                        error!("Error unregistering subscriber: {:?}", e);
                    }
                    
                    // 연결 상태를 disconnected로 마킹
                    connection_tracker.mark_disconnected(&connection_key_clone).await;
                });
                
                // TODO: 재시도 메커니즘이 필요한 경우 여기에 구현
                // 현재는 초기 동기화로 대체
                
                // 초기 동기화: 기존 파일들에 대한 알림 전송
                if let Err(e) = self.send_initial_file_sync(&account_hash, &device_hash).await {
                    warn!("Failed to send initial file sync to {}:{}: {}", account_hash, device_hash, e);
                }
                
                info!("File updates subscription registered successfully for: {}", subscriber_key);
                Ok(Response::new(Box::pin(stream) as Self::SubscribeToFileUpdatesStream))
            },
            Err(e) => {
                error!("Failed to register file update subscriber: {:?}", e);
                Err(Status::internal(format!("Failed to register subscriber: {}", e)))
            }
        }
    }
    
    async fn subscribe_to_watcher_preset_updates(
        &self,
        request: Request<SubscribeRequest>,
    ) -> Result<Response<Self::SubscribeToWatcherPresetUpdatesStream>, Status> {
        debug!("Watcher preset updates subscription request received");
        
        let req = request.into_inner();
        let device_hash = req.device_hash;
        
        // 먼저 토큰을 검증하고 실제 account_hash를 가져옴
        let auth_result = match self.app_state.oauth.verify_token(&req.auth_token).await {
            Ok(result) => {
                if !result.valid {
                    error!("Invalid auth token for device: {}", device_hash);
                    return Err(Status::unauthenticated("Invalid authentication token"));
                }
                result
            },
            Err(e) => {
                error!("Token verification error for device {}: {}", device_hash, e);
                return Err(Status::unauthenticated("Invalid authentication"));
            }
        };
        
        let account_hash = auth_result.account_hash; // 토큰에서 추출한 실제 account_hash 사용
        
        // 요청된 account_hash와 다르면 경고 로그 출력 (하지만 에러는 아님)
        if req.account_hash != account_hash {
            warn!("Client sent different account_hash (requested: {}, actual: {}) for device: {}", 
                  req.account_hash, account_hash, device_hash);
        }
        
        // 장치 검증
        let is_dev_mode = std::env::var("COSMIC_SYNC_DEV_MODE").unwrap_or_default() == "1";
        let is_test_mode = std::env::var("COSMIC_SYNC_TEST_MODE").unwrap_or_default() == "1";
        
        if !is_dev_mode && !is_test_mode {
            let is_valid_device = match self.app_state.storage.validate_device(&account_hash, &device_hash).await {
                Ok(valid) => valid,
                Err(e) => {
                    error!("Error validating device: {}", e);
                    return Err(Status::internal("Error validating device"));
                }
            };
            
            if !is_valid_device {
                return Err(Status::unauthenticated("Invalid device"));
            }
        }
        
        // 채널 생성 - 버퍼 크기 증가
        let (tx, rx) = mpsc::channel(1024);
        let stream = ReceiverStream::new(rx);
        
        // 구독 키 생성
        let sub_key = format!("{}:{}", account_hash, device_hash);
        
        // 기존 구독이 있으면 해제 (중복 구독 방지)
        match self.app_state.notification_manager.unregister_watcher_preset_update_subscriber(&sub_key).await {
            Ok(true) => debug!("Removed existing watcher preset subscription: {}", sub_key),
            Ok(false) => debug!("No existing watcher preset subscription to remove: {}", sub_key),
            Err(e) => warn!("Failed to remove existing preset subscription: {:?}", e),
        }
        
        // NotificationManager에 구독자 등록
        if let Err(e) = self.app_state.notification_manager.register_watcher_preset_update_subscriber(sub_key.clone(), tx).await {
            error!("Failed to register watcher preset update subscriber: {:?}", e);
            return Err(Status::internal(format!("Failed to register preset subscriber: {}", e)));
        }
        
        info!("Registered watcher preset update subscriber: {}", sub_key);
        
        Ok(Response::new(Box::pin(stream) as Self::SubscribeToWatcherPresetUpdatesStream))
    }
    
    async fn subscribe_to_watcher_group_updates(
        &self,
        request: Request<SubscribeRequest>,
    ) -> Result<Response<Self::SubscribeToWatcherGroupUpdatesStream>, Status> {
        debug!("Watcher group updates subscription request received");
        
        let req = request.into_inner();
        let device_hash = req.device_hash;
        
        // 먼저 토큰을 검증하고 실제 account_hash를 가져옴
        let auth_result = match self.app_state.oauth.verify_token(&req.auth_token).await {
            Ok(result) => {
                if !result.valid {
                    error!("Invalid auth token for device: {}", device_hash);
                    return Err(Status::unauthenticated("Invalid authentication token"));
                }
                result
            },
            Err(e) => {
                error!("Token verification error for device {}: {}", device_hash, e);
                return Err(Status::unauthenticated("Invalid authentication"));
            }
        };
        
        let account_hash = auth_result.account_hash; // 토큰에서 추출한 실제 account_hash 사용
        
        // 요청된 account_hash와 다르면 경고 로그 출력 (하지만 에러는 아님)
        if req.account_hash != account_hash {
            warn!("Client sent different account_hash (requested: {}, actual: {}) for device: {}", 
                  req.account_hash, account_hash, device_hash);
        }
        
        // 장치 검증
        let is_dev_mode = std::env::var("COSMIC_SYNC_DEV_MODE").unwrap_or_default() == "1";
        let is_test_mode = std::env::var("COSMIC_SYNC_TEST_MODE").unwrap_or_default() == "1";
        
        if !is_dev_mode && !is_test_mode {
            let is_valid_device = match self.app_state.storage.validate_device(&account_hash, &device_hash).await {
                Ok(valid) => valid,
                Err(e) => {
                    error!("Error validating device: {}", e);
                    return Err(Status::internal("Error validating device"));
                }
            };
            
            if !is_valid_device {
                return Err(Status::unauthenticated("Invalid device"));
            }
        }
        
        // 채널 생성 - 버퍼 크기 증가
        let (tx, rx) = mpsc::channel(1024); // 버퍼 크기를 1024로 증가 (이전 128)
        let stream = ReceiverStream::new(rx);
        
        // 구독 키 생성
        let sub_key = format!("{}:{}", account_hash, device_hash);
        
        // 기존 구독이 있으면 해제 (중복 구독 방지)
        match self.app_state.notification_manager.unregister_watcher_group_update_subscriber(&sub_key).await {
            Ok(true) => debug!("Removed existing watcher group subscription: {}", sub_key),
            Ok(false) => debug!("No existing watcher group subscription to remove: {}", sub_key),
            Err(e) => warn!("Failed to remove existing subscription: {:?}", e),
        }
        
        // NotificationManager에 구독자 등록
        if let Err(e) = self.app_state.notification_manager.register_watcher_group_update_subscriber(sub_key.clone(), tx).await {
            error!("Failed to register watcher group update subscriber: {:?}", e);
            return Err(Status::internal(format!("Failed to register subscriber: {}", e)));
        }
        
        info!("Registered watcher group update subscriber: {}", sub_key);
        
        // 연결 상태 확인용 초기 메시지 전송 (PING 역할)
        let heartbeat_interval = std::env::var("HEARTBEAT_INTERVAL_SECS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(10); // 기본값 10초로 단축 (이전 30초)
        
        // 클라이언트 연결 상태 모니터링을 위한 태스크
        let notification_manager_clone = self.app_state.notification_manager.clone();
        let sub_key_clone = sub_key.clone();
        let account_hash_clone = account_hash.clone();
        let device_hash_clone = device_hash.clone();
        
        tokio::spawn(async move {
            // 초기화 지연
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            debug!("Starting watcher group updates monitoring for {}", sub_key_clone);
            
            let mut keep_alive_interval = tokio::time::interval(tokio::time::Duration::from_secs(heartbeat_interval));
            let mut failure_count = 0;
            let max_failures = 3; // 최대 3번의 실패 후 구독 해제
            
            loop {
                tokio::select! {
                    _ = keep_alive_interval.tick() => {
                        // 구독자 여전히 활성 상태인지 확인
                        let is_active = notification_manager_clone.is_watcher_group_subscriber_active(&sub_key_clone).await;
                        if !is_active {
                            debug!("Watcher group subscriber {} no longer active, stopping monitoring", sub_key_clone);
                            break;
                        }
                        
                        // 연결이 활성 상태인지 ping 메시지 전송을 시도
                        let ping_notification = WatcherGroupUpdateNotification {
                            account_hash: account_hash_clone.clone(),
                            device_hash: device_hash_clone.clone(),
                            group_data: None, // 데이터 없음 (핑 목적)
                            update_type: 0, // CREATED
                            timestamp: chrono::Utc::now().timestamp(),
                        };
                        
                        // 특정 구독자에게만 핑 전송
                        match notification_manager_clone.ping_watcher_group_subscriber(&sub_key_clone, ping_notification).await {
                            Ok(_) => {
                                // 성공적으로 전송되면 실패 카운트 리셋
                                failure_count = 0;
                            },
                            Err(_) => {
                                // 전송 실패시 실패 카운트 증가
                                failure_count += 1;
                                warn!("Failed to ping watcher group subscriber {}, failure count: {}/{}", 
                                    sub_key_clone, failure_count, max_failures);
                                
                                if failure_count >= max_failures {
                                    warn!("Max failures reached for {}, unregistering subscriber", sub_key_clone);
                                    break;
                                }
                            }
                        }
                    }
                }
            }
            
            // 구독 해제 - 중복 호출 방지를 위해 한 번만 시도
            match notification_manager_clone.unregister_watcher_group_update_subscriber(&sub_key_clone).await {
                Ok(true) => {
                    info!("Unregistered watcher group update subscriber: {}", sub_key_clone);
                },
                Ok(false) => {
                    debug!("Watcher group update subscriber {} was already removed", sub_key_clone);
                },
                Err(e) => {
                    warn!("Failed to unregister watcher group update subscriber: {:?}", e);
                }
            }
        });
        
        Ok(Response::new(Box::pin(stream) as Self::SubscribeToWatcherGroupUpdatesStream))
    }

    // Version management methods implementation
    async fn get_file_history(
        &self,
        request: Request<GetFileHistoryRequest>,
    ) -> Result<Response<GetFileHistoryResponse>, Status> {
        match self.sync_handler.get_file_history(request).await {
            Ok(response) => Ok(response),
            Err(status) => Err(status),
        }
    }

    async fn restore_file_version(
        &self,
        request: Request<RestoreFileVersionRequest>,
    ) -> Result<Response<RestoreFileVersionResponse>, Status> {
        match self.sync_handler.restore_file_version(request).await {
            Ok(response) => Ok(response),
            Err(status) => Err(status),
        }
    }

    async fn broadcast_file_restore(
        &self,
        request: Request<BroadcastFileRestoreRequest>,
    ) -> Result<Response<BroadcastFileRestoreResponse>, Status> {
        match self.sync_handler.broadcast_file_restore(request).await {
            Ok(response) => Ok(response),
            Err(status) => Err(status),
        }
    }

    type SubscribeToVersionUpdatesStream = std::pin::Pin<
        Box<dyn futures::Stream<Item = Result<VersionUpdateNotification, Status>> + Send + 'static>
    >;

    async fn subscribe_to_version_updates(
        &self,
        request: Request<SubscribeRequest>,
    ) -> Result<Response<Self::SubscribeToVersionUpdatesStream>, Status> {
        debug!("Version updates subscription requested");
        
        let (tx, rx) = mpsc::channel(128);
        
        // Create a stream from the receiver
        let stream = tokio_stream::wrappers::ReceiverStream::new(rx).map(|notification| {
            Ok(notification)
        });
        
        Ok(Response::new(Box::pin(stream) as Self::SubscribeToVersionUpdatesStream))
    }
}

/// Synchronization client service implementation
pub struct SyncClientServiceImpl {
    /// Application state
    app_state: Arc<AppState>,
    /// Authentication handler
    auth_handler: AuthHandler,
}

impl SyncClientServiceImpl {
    /// Create a new SyncClientServiceImpl instance
    pub fn new(app_state: Arc<AppState>) -> Self {
        let auth_handler = AuthHandler::new(app_state.clone());
        
        Self {
            app_state,
            auth_handler,
        }
    }
}

#[tonic::async_trait]
impl SyncClientService for SyncClientServiceImpl {
    async fn notify_auth_success(
        &self,
        request: Request<AuthSuccessNotification>,
    ) -> Result<Response<AuthNotificationResponse>, Status> {
        debug!("Auth success notification received");
        
        // TODO: 인증 성공 메시지 처리 구현
        
        Ok(Response::new(AuthNotificationResponse {
            success: true,
            return_message: "Notification received".to_string(),
        }))
    }
} 