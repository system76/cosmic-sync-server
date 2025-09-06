use crate::server::app_state::AppState;
use crate::services::version_service::VersionService;
use crate::services::Handler;
use crate::storage::Storage;
use crate::sync::{
    AuthUpdateNotification, BroadcastFileRestoreRequest, BroadcastFileRestoreResponse,
    DeviceUpdateNotification, EncryptionKeyUpdateNotification, FileUpdateNotification,
    GetFileHistoryRequest, GetFileHistoryResponse, HealthCheckRequest, HealthCheckResponse,
    RequestEncryptionKeyRequest, RequestEncryptionKeyResponse, RestoreFileVersionRequest,
    RestoreFileVersionResponse, VersionUpdateNotification, WatcherGroupUpdateNotification,
    WatcherPresetUpdateNotification,
};
use async_trait::async_trait;
use std::sync::Arc;
use tonic::{Request, Response, Status};
use tracing::{debug, error, info, warn};

/// Handler for synchronization-related requests
pub struct SyncHandler {
    pub app_state: Arc<AppState>,
}

impl SyncHandler {
    /// Create a new sync handler
    pub fn new(app_state: Arc<AppState>) -> Self {
        Self { app_state }
    }

    /// Handle encryption key request
    pub async fn request_encryption_key(
        &self,
        request: Request<RequestEncryptionKeyRequest>,
    ) -> Result<Response<RequestEncryptionKeyResponse>, Status> {
        let req = request.into_inner();
        debug!(
            "Encryption key request: account={}, device={}",
            req.account_hash, req.device_hash
        );

        // Validate auth token
        if req.auth_token.is_empty() {
            warn!("Request for encryption key without token");
            return Err(Status::unauthenticated("Authentication token is required"));
        }

        // validate token
        let auth_result = match self.app_state.oauth.verify_token(&req.auth_token).await {
            Ok(result) => result,
            Err(e) => {
                error!("Auth token validation failed: {}", e);
                return Ok(Response::new(RequestEncryptionKeyResponse {
                    success: false,
                    encryption_key: String::new(),
                    return_message: "Authentication failed".to_string(),
                }));
            }
        };

        if !auth_result.valid || auth_result.account_hash != req.account_hash {
            debug!(
                "Normalizing account_hash for encryption key: request={} -> token={}",
                req.account_hash, auth_result.account_hash
            );
            return Ok(Response::new(RequestEncryptionKeyResponse {
                success: false,
                encryption_key: String::new(),
                return_message: "Account authentication failed".to_string(),
            }));
        }

        // Get or create encryption key for the account
        match self
            .app_state
            .encryption
            .get_or_create_key(&auth_result.account_hash)
            .await
        {
            Ok(key) => {
                info!(
                    "Encryption key provided for account: {}",
                    auth_result.account_hash
                );
                Ok(Response::new(RequestEncryptionKeyResponse {
                    success: true,
                    encryption_key: key,
                    return_message: String::new(),
                }))
            }
            Err(e) => {
                error!("Failed to get encryption key: {}", e);
                Ok(Response::new(RequestEncryptionKeyResponse {
                    success: false,
                    encryption_key: String::new(),
                    return_message: format!("Encryption key processing error: {}", e),
                }))
            }
        }
    }

    /// Handle health check request
    pub async fn health_check(
        &self,
        _request: Request<HealthCheckRequest>,
    ) -> Result<Response<HealthCheckResponse>, Status> {
        debug!("Processing health check request");

        let version = option_env!("CARGO_PKG_VERSION").unwrap_or("unknown");

        Ok(Response::new(HealthCheckResponse {
            status: "SERVING".to_string(),
            version: version.to_string(),
        }))
    }

    /// Handle get file history request
    pub async fn get_file_history(
        &self,
        request: Request<GetFileHistoryRequest>,
    ) -> Result<Response<GetFileHistoryResponse>, Status> {
        let req = request.into_inner();
        debug!(
            "Get file history request: account={}, path={}",
            req.account_hash, req.file_path
        );

        // Validate auth token
        if req.auth_token.is_empty() {
            warn!("Get file history request without token");
            return Err(Status::unauthenticated("Authentication token is required"));
        }

        // Validate token
        let auth_result = match self.app_state.oauth.verify_token(&req.auth_token).await {
            Ok(result) => result,
            Err(e) => {
                error!("Auth token validation failed: {}", e);
                return Ok(Response::new(GetFileHistoryResponse {
                    success: false,
                    return_message: "Authentication failed".to_string(),
                    versions: vec![],
                    total_versions: 0,
                    has_more: false,
                }));
            }
        };

        if !auth_result.valid || auth_result.account_hash != req.account_hash {
            debug!(
                "Normalizing account_hash for file history: request={} -> token={}",
                req.account_hash, auth_result.account_hash
            );
            return Ok(Response::new(GetFileHistoryResponse {
                success: false,
                return_message: "Account authentication failed".to_string(),
                versions: vec![],
                total_versions: 0,
                has_more: false,
            }));
        }

        // Use version service to get file history
        match self.app_state.version_service.get_file_history(req).await {
            Ok(response) => {
                info!(
                    "File history retrieved for account: {}",
                    auth_result.account_hash
                );
                Ok(Response::new(response))
            }
            Err(e) => {
                error!("Failed to get file history: {}", e);
                Ok(Response::new(GetFileHistoryResponse {
                    success: false,
                    return_message: format!("Failed to get file history: {}", e),
                    versions: vec![],
                    total_versions: 0,
                    has_more: false,
                }))
            }
        }
    }

    /// Handle restore file version request
    pub async fn restore_file_version(
        &self,
        request: Request<RestoreFileVersionRequest>,
    ) -> Result<Response<RestoreFileVersionResponse>, Status> {
        let req = request.into_inner();
        debug!(
            "Restore file version request: account={}, file_id={}, revision={}",
            req.account_hash, req.file_id, req.target_revision
        );

        // Validate auth token
        if req.auth_token.is_empty() {
            warn!("Restore file version request without token");
            return Err(Status::unauthenticated("Authentication token is required"));
        }

        // Validate token
        let auth_result = match self.app_state.oauth.verify_token(&req.auth_token).await {
            Ok(result) => result,
            Err(e) => {
                error!("Auth token validation failed: {}", e);
                return Ok(Response::new(RestoreFileVersionResponse {
                    success: false,
                    return_message: "Authentication failed".to_string(),
                    new_revision: 0,
                    restored_file_info: None,
                    notified_devices: 0,
                }));
            }
        };

        if !auth_result.valid || auth_result.account_hash != req.account_hash {
            debug!(
                "Normalizing account_hash for file restore: request={} -> token={}",
                req.account_hash, auth_result.account_hash
            );
            return Ok(Response::new(RestoreFileVersionResponse {
                success: false,
                return_message: "Account authentication failed".to_string(),
                new_revision: 0,
                restored_file_info: None,
                notified_devices: 0,
            }));
        }

        // Use version service to restore file version
        match self
            .app_state
            .version_service
            .restore_file_version(req)
            .await
        {
            Ok(response) => {
                info!(
                    "File version restored for account: {}",
                    auth_result.account_hash
                );
                Ok(Response::new(response))
            }
            Err(e) => {
                error!("Failed to restore file version: {}", e);
                Ok(Response::new(RestoreFileVersionResponse {
                    success: false,
                    return_message: format!("Failed to restore file version: {}", e),
                    new_revision: 0,
                    restored_file_info: None,
                    notified_devices: 0,
                }))
            }
        }
    }

    /// Handle broadcast file restore request
    pub async fn broadcast_file_restore(
        &self,
        request: Request<BroadcastFileRestoreRequest>,
    ) -> Result<Response<BroadcastFileRestoreResponse>, Status> {
        let req = request.into_inner();
        debug!(
            "Broadcast file restore request: account={}, file_id={}",
            req.account_hash, req.file_id
        );

        // Validate auth token
        if req.auth_token.is_empty() {
            warn!("Broadcast file restore request without token");
            return Err(Status::unauthenticated("Authentication token is required"));
        }

        // Validate token
        let auth_result = match self.app_state.oauth.verify_token(&req.auth_token).await {
            Ok(result) => result,
            Err(e) => {
                error!("Auth token validation failed: {}", e);
                return Ok(Response::new(BroadcastFileRestoreResponse {
                    success: false,
                    return_message: "Authentication failed".to_string(),
                    notified_device_hashes: vec![],
                    total_notified: 0,
                }));
            }
        };

        if !auth_result.valid || auth_result.account_hash != req.account_hash {
            debug!(
                "Normalizing account_hash for broadcast restore: request={} -> token={}",
                req.account_hash, auth_result.account_hash
            );
            return Ok(Response::new(BroadcastFileRestoreResponse {
                success: false,
                return_message: "Account authentication failed".to_string(),
                notified_device_hashes: vec![],
                total_notified: 0,
            }));
        }

        // Use version service to broadcast file restore
        match self
            .app_state
            .version_service
            .broadcast_file_restore(req.clone())
            .await
        {
            Ok(response) => {
                info!(
                    "File restore broadcasted for account: {}",
                    auth_result.account_hash
                );
                // Publish version restored event
                let routing_key = format!(
                    "version.restored.{}.{}",
                    auth_result.account_hash, req.file_id
                );
                let payload = serde_json::json!({
                    "type": "version_restored",
                    "id": nanoid::nanoid!(8),
                    "account_hash": auth_result.account_hash,
                    "file_id": req.file_id,
                    "target_revision": req.new_revision,
                    "source_device_hash": req.source_device_hash,
                    "timestamp": chrono::Utc::now().timestamp(),
                })
                .to_string()
                .into_bytes();
                if let Err(e) = self
                    .app_state
                    .event_bus
                    .publish(&routing_key, payload)
                    .await
                {
                    debug!("EventBus publish failed (noop or disconnected): {}", e);
                }
                Ok(Response::new(response))
            }
            Err(e) => {
                error!("Failed to broadcast file restore: {}", e);
                Ok(Response::new(BroadcastFileRestoreResponse {
                    success: false,
                    return_message: format!("Failed to broadcast file restore: {}", e),
                    notified_device_hashes: vec![],
                    total_notified: 0,
                }))
            }
        }
    }
}

#[async_trait]
impl Handler for SyncHandler {
    // define streaming return type
    type SubscribeToAuthUpdatesStream = std::pin::Pin<
        Box<dyn futures::Stream<Item = Result<AuthUpdateNotification, Status>> + Send + 'static>,
    >;
    type SubscribeToDeviceUpdatesStream = std::pin::Pin<
        Box<dyn futures::Stream<Item = Result<DeviceUpdateNotification, Status>> + Send + 'static>,
    >;
    type SubscribeToEncryptionKeyUpdatesStream = std::pin::Pin<
        Box<
            dyn futures::Stream<Item = Result<EncryptionKeyUpdateNotification, Status>>
                + Send
                + 'static,
        >,
    >;
    type SubscribeToFileUpdatesStream = std::pin::Pin<
        Box<dyn futures::Stream<Item = Result<FileUpdateNotification, Status>> + Send + 'static>,
    >;
    type SubscribeToWatcherPresetUpdatesStream = std::pin::Pin<
        Box<
            dyn futures::Stream<Item = Result<WatcherPresetUpdateNotification, Status>>
                + Send
                + 'static,
        >,
    >;
    type SubscribeToWatcherGroupUpdatesStream = std::pin::Pin<
        Box<
            dyn futures::Stream<Item = Result<WatcherGroupUpdateNotification, Status>>
                + Send
                + 'static,
        >,
    >;
    type SubscribeToVersionUpdatesStream = std::pin::Pin<
        Box<dyn futures::Stream<Item = Result<VersionUpdateNotification, Status>> + Send + 'static>,
    >;

    async fn handle_request_encryption_key(
        &self,
        request: Request<RequestEncryptionKeyRequest>,
    ) -> Result<Response<RequestEncryptionKeyResponse>, Status> {
        self.request_encryption_key(request).await
    }

    async fn handle_health_check(
        &self,
        request: Request<HealthCheckRequest>,
    ) -> Result<Response<HealthCheckResponse>, Status> {
        self.health_check(request).await
    }

    // Version management handler implementations
    async fn handle_get_file_history(
        &self,
        request: Request<GetFileHistoryRequest>,
    ) -> Result<Response<GetFileHistoryResponse>, Status> {
        self.get_file_history(request).await
    }

    async fn handle_restore_file_version(
        &self,
        request: Request<RestoreFileVersionRequest>,
    ) -> Result<Response<RestoreFileVersionResponse>, Status> {
        self.restore_file_version(request).await
    }

    async fn handle_broadcast_file_restore(
        &self,
        request: Request<BroadcastFileRestoreRequest>,
    ) -> Result<Response<BroadcastFileRestoreResponse>, Status> {
        self.broadcast_file_restore(request).await
    }
}
