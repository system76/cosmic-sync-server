use tonic::{Request, Response, Status};
use crate::sync::{
    LoginRequest, LoginResponse,
    RegisterDeviceRequest, RegisterDeviceResponse,
    ListDevicesRequest, ListDevicesResponse,
    DeleteDeviceRequest, DeleteDeviceResponse,
    RequestEncryptionKeyRequest, RequestEncryptionKeyResponse,
    UpdateDeviceInfoRequest, UpdateDeviceInfoResponse,
    AuthUpdateNotification, DeviceUpdateNotification, 
    EncryptionKeyUpdateNotification, FileUpdateNotification,
    WatcherPresetUpdateNotification, WatcherGroupUpdateNotification,
    VersionUpdateNotification
};
use crate::sync;
use std::sync::Arc;
use crate::server::app_state::AppState;
use crate::models::device::Device;
use tracing::{info, warn, error, debug};
use chrono::Utc;
use uuid::Uuid;
use async_trait::async_trait;
use crate::services::Handler;
use prost_types;

/// Handler for device-related requests
pub struct DeviceHandler {
    app_state: Arc<AppState>,
}

impl DeviceHandler {
    /// Create a new device handler
    pub fn new(app_state: Arc<AppState>) -> Self {
        Self { app_state }
    }
    
    /// Validate auth token and return account hash if valid
    async fn validate_auth_token(&self, auth_token: &str) -> Result<String, Status> {
        match self.app_state.oauth.validate_token(auth_token).await {
            Ok(account_hash) => Ok(account_hash),
            Err(_) => Err(Status::unauthenticated("Authentication failed")),
        }
    }
    
    /// Register device API handler
    pub async fn register_device(&self, request: Request<RegisterDeviceRequest>) -> Result<Response<RegisterDeviceResponse>, Status> {
        let mut req = request.into_inner();
        info!("device registration request: account_hash={}, device_hash={}, os_version={}, app_version={}",
             req.account_hash, req.device_hash, req.os_version, req.app_version);

        // Input validation (device_hash만 필수)
        if req.device_hash.is_empty() {
            return Ok(Response::new(RegisterDeviceResponse {
                success: false,
                device_hash: String::new(),
                return_message: "Device hash cannot be empty".to_string(),
            }));
        }

        if req.device_hash.len() > 128 {
            return Ok(Response::new(RegisterDeviceResponse {
                success: false,
                device_hash: String::new(),
                return_message: "Device hash too long (max 128 characters)".to_string(),
            }));
        }

        if req.auth_token.is_empty() {
            return Ok(Response::new(RegisterDeviceResponse {
                success: false,
                device_hash: String::new(),
                return_message: "Authentication token required".to_string(),
            }));
        }
        
        // validate authentication token and normalize account_hash
        match self.app_state.oauth.validate_token(&req.auth_token).await {
            Ok(validated_account_hash) => {
                if !req.account_hash.is_empty() && validated_account_hash != req.account_hash {
                    debug!("Normalizing account_hash from request={} to token={}", req.account_hash, validated_account_hash);
                }
                let server_account_hash = validated_account_hash;
                
                info!("device registration/update: account_hash={}, device_hash={}", server_account_hash, req.device_hash);
                
                let device = Device::new(
                    server_account_hash.clone(),
                    req.device_hash.clone(),
                    req.is_active,
                    req.os_version.clone(),
                    req.app_version.clone(),
                );
                
                match self.app_state.device.register_device(&device).await {
                    Ok(_) => {
                        info!("✅ device registration/update successful: account_hash={}, device_hash={}", server_account_hash, req.device_hash);
                        let response = RegisterDeviceResponse {
                            success: true,
                            device_hash: device.device_hash.clone(),
                            return_message: "device registration/update successful".to_string(),
                        };
                        Ok(Response::new(response))
                    },
                    Err(e) => {
                        error!("device registration/update failed: account_hash={}, device_hash={}, error={}", server_account_hash, req.device_hash, e);
                        let response = RegisterDeviceResponse {
                            success: false,
                            device_hash: String::new(),
                            return_message: format!("device registration/update failed: {}", e),
                        };
                        Ok(Response::new(response))
                    }
                }
            },
            Err(e) => {
                warn!("authentication token verification failed: {}", e);
                Ok(Response::new(RegisterDeviceResponse {
                    success: false,
                    device_hash: String::new(),
                    return_message: format!("authentication failed: {}", e),
                }))
            }
        }
    }
    
    /// Update device info API handler
    pub async fn update_device_info(&self, request: Request<UpdateDeviceInfoRequest>) -> Result<Response<UpdateDeviceInfoResponse>, Status> {
        let req = request.into_inner();
        
        info!("device info update request: account={}, device={}, os_version={}, app_version={}", 
              req.account_hash, req.device_hash, req.os_version, req.app_version);
        
        // process without token - only validate account hash and device hash
        match self.app_state.device.get_device(&req.device_hash, &req.account_hash).await {
            Ok(Some(mut device)) => {
                // update device info
                info!("update existing device info: {}", req.device_hash);
                debug!("previous info: os_version={}, app_version={}, is_active={}", 
                      device.os_version, device.app_version, device.is_active);
                
                // compare previous info with changed info
                let os_changed = device.os_version != req.os_version;
                let app_changed = device.app_version != req.app_version;
                let active_changed = device.is_active != req.is_active;
                
                // update device info
                device.is_active = req.is_active;
                device.os_version = req.os_version;
                device.app_version = req.app_version;
                device.updated_at = Utc::now(); // update updated_at
                
                debug!("new info: os_version={}, app_version={}, is_active={}", 
                      device.os_version, device.app_version, device.is_active);
                
                // log changed info
                if os_changed || app_changed || active_changed {
                    info!("device info changed: os_version_changed={}, app_version_changed={}, active_changed={}", 
                         os_changed, app_changed, active_changed);
                }
                
                // save updated device info
                match self.app_state.device.update_device(&device).await {
                    Ok(()) => {
                        info!("device info updated successfully: {}", req.device_hash);
                        Ok(Response::new(UpdateDeviceInfoResponse {
                            success: true,
                            return_message: "device info updated successfully".to_string(),
                        }))
                    },
                    Err(e) => {
                        error!("device info update failed: {}", e);
                        Ok(Response::new(UpdateDeviceInfoResponse {
                            success: false,
                            return_message: format!("device info update failed: {}", e),
                        }))
                    }
                }
            },
            Ok(None) => {
                // if device does not exist, register new device
                info!("requested device does not exist in database. register new device: {}", req.device_hash);
                
                // create new device object
                let new_device = Device::new(
                    req.account_hash.clone(),
                    req.device_hash.clone(),
                    req.is_active,
                    req.os_version.clone(),
                    req.app_version.clone(),
                );
                
                // register new device
                match self.app_state.device.register_device(&new_device).await {
                    Ok(()) => {
                        info!("new device registered successfully: {}", req.device_hash);
                        Ok(Response::new(UpdateDeviceInfoResponse {
                            success: true,
                            return_message: "new device registered successfully".to_string(),
                        }))
                    },
                    Err(e) => {
                        error!("device registration failed: {}", e);
                        Ok(Response::new(UpdateDeviceInfoResponse {
                            success: false,
                            return_message: format!("device registration failed: {}", e),
                        }))
                    }
                }
            },
            Err(e) => {
                error!("device info lookup failed: {}", e);
                Ok(Response::new(UpdateDeviceInfoResponse {
                    success: false,
                    return_message: format!("device info lookup failed: {}", e),
                }))
            }
        }
    }
    
    /// List devices API handler
    pub async fn list_devices(&self, request: Request<ListDevicesRequest>) -> Result<Response<ListDevicesResponse>, Status> {
        let req = request.into_inner();
        
        // Validate auth token
        let account_hash = self.validate_auth_token(req.auth_token.as_str()).await?;
        
        // Get devices from service
        match self.app_state.device.list_devices(&account_hash).await {
            Ok(device_list) => {
                // Convert to proto device list
                let devices: Vec<sync::DeviceInfo> = device_list.iter()
                    .map(|device| sync::DeviceInfo {
                        account_hash: device.account_hash.clone(),
                        device_hash: device.device_hash.clone(),
                        is_active: device.is_active,
                        os_version: device.os_version.clone(),
                        app_version: device.app_version.clone(),
                        registered_at: Some(prost_types::Timestamp {
                            seconds: device.registered_at.timestamp(),
                            nanos: 0,
                        }),
                        last_sync_time: Some(prost_types::Timestamp {
                            seconds: device.last_sync.timestamp(),
                            nanos: 0,
                        }),
                    })
                    .collect();
                
                let response = ListDevicesResponse {
                    success: true,
                    devices,
                    return_message: String::new(),
                };
                
                Ok(Response::new(response))
            },
            Err(e) => {
                error!("Failed to get devices: {}", e);
                let response = ListDevicesResponse {
                    success: false,
                    devices: Vec::new(),
                    return_message: format!("Failed to get devices: {}", e),
                };
                
                Ok(Response::new(response))
            }
        }
    }
    
    /// Delete device API handler
    pub async fn delete_device(&self, request: Request<DeleteDeviceRequest>) -> Result<Response<DeleteDeviceResponse>, Status> {
        let req = request.into_inner();
        
        info!("Delete device request: account={}, device={}", 
              req.account_hash, req.device_hash);
        
        // Validate auth token
        match self.app_state.oauth.validate_token(&req.auth_token).await {
            Ok(account_hash) => {
                // Delete device - account_hash is the first parameter
                match self.app_state.device.delete_device(&account_hash, &req.device_hash).await {
                    Ok(()) => {
                        Ok(Response::new(DeleteDeviceResponse {
                            success: true,
                            return_message: "".to_string(),
                        }))
                    },
                    Err(e) => {
                        error!("Device deletion error: {}", e);
                        Ok(Response::new(DeleteDeviceResponse {
                            success: false,
                            return_message: format!("Device deletion failed: {}", e),
                        }))
                    }
                }
            },
            Err(e) => {
                warn!("Auth token validation failed: {}", e);
                Ok(Response::new(DeleteDeviceResponse {
                    success: false,
                    return_message: format!("Authentication failed: {}", e),
                }))
            }
        }
    }
}

#[async_trait]
impl Handler for DeviceHandler {
    // define streaming return type
    type SubscribeToAuthUpdatesStream = std::pin::Pin<Box<dyn futures::Stream<Item = Result<AuthUpdateNotification, Status>> + Send + 'static>>;
    type SubscribeToDeviceUpdatesStream = std::pin::Pin<Box<dyn futures::Stream<Item = Result<DeviceUpdateNotification, Status>> + Send + 'static>>;
    type SubscribeToEncryptionKeyUpdatesStream = std::pin::Pin<Box<dyn futures::Stream<Item = Result<EncryptionKeyUpdateNotification, Status>> + Send + 'static>>;
    type SubscribeToFileUpdatesStream = std::pin::Pin<Box<dyn futures::Stream<Item = Result<FileUpdateNotification, Status>> + Send + 'static>>;
    type SubscribeToWatcherPresetUpdatesStream = std::pin::Pin<Box<dyn futures::Stream<Item = Result<WatcherPresetUpdateNotification, Status>> + Send + 'static>>;
    type SubscribeToWatcherGroupUpdatesStream = std::pin::Pin<Box<dyn futures::Stream<Item = Result<WatcherGroupUpdateNotification, Status>> + Send + 'static>>;
    type SubscribeToVersionUpdatesStream = std::pin::Pin<Box<dyn futures::Stream<Item = Result<VersionUpdateNotification, Status>> + Send + 'static>>;

    async fn handle_register_device(
        &self,
        request: Request<RegisterDeviceRequest>,
    ) -> Result<Response<RegisterDeviceResponse>, Status> {
        self.register_device(request).await
    }
    
    async fn handle_list_devices(
        &self,
        request: Request<ListDevicesRequest>,
    ) -> Result<Response<ListDevicesResponse>, Status> {
        self.list_devices(request).await
    }
    
    async fn handle_delete_device(
        &self,
        request: Request<DeleteDeviceRequest>,
    ) -> Result<Response<DeleteDeviceResponse>, Status> {
        self.delete_device(request).await
    }
    
    async fn handle_update_device_info(
        &self,
        request: Request<UpdateDeviceInfoRequest>,
    ) -> Result<Response<UpdateDeviceInfoResponse>, Status> {
        self.update_device_info(request).await
    }
    
    async fn handle_request_encryption_key(
        &self,
        request: Request<RequestEncryptionKeyRequest>,
    ) -> Result<Response<RequestEncryptionKeyResponse>, Status> {
        let req = request.into_inner();
        
        debug!("Encryption key request: device={}", req.device_hash);
        
        // Process key request through encryption service
        match self.app_state.encryption.request_encryption_key(&req.account_hash, &req.device_hash).await {
            Ok(key) => {
                Ok(Response::new(RequestEncryptionKeyResponse {
                    success: true,
                    encryption_key: key,
                    return_message: String::new(),
                }))
            },
            Err(e) => {
                Ok(Response::new(RequestEncryptionKeyResponse {
                    success: false,
                    encryption_key: String::new(),
                    return_message: format!("Failed to get encryption key: {}", e),
                }))
            }
        }
    }
}
