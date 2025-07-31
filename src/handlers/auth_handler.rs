use async_trait::async_trait;
use tonic::{Request, Response, Status};
use crate::sync::{
    OAuthExchangeRequest, OAuthExchangeResponse,
    ValidateTokenRequest, ValidateTokenResponse,
    CheckAuthStatusRequest, CheckAuthStatusResponse,
    AuthSuccessNotification, AuthNotificationResponse,
    LoginRequest, LoginResponse,
    VerifyLoginRequest, VerifyLoginResponse,
    AuthUpdateNotification, DeviceUpdateNotification,
    EncryptionKeyUpdateNotification, FileUpdateNotification,
    WatcherPresetUpdateNotification, WatcherGroupUpdateNotification,
    VersionUpdateNotification,
    GetAccountInfoRequest, GetAccountInfoResponse
};
use crate::server::app_state::{AppState, AuthSession};
use std::sync::{Arc, Mutex};
use tracing::{info, warn, error, debug};
use crate::auth::oauth::process_oauth_code;
use std::collections::HashMap;
use chrono::{Utc, Duration};
use uuid;

/// Authentication-related request handler
pub struct AuthHandler {
    pub app_state: Arc<AppState>,
    // Map to store ongoing authentication sessions - REMOVED (now uses AppState)
}

impl AuthHandler {
    /// Create a new authentication handler
    pub fn new(app_state: Arc<AppState>) -> Self {
        Self { 
            app_state,
            // auth_sessions: Arc::new(Mutex::new(HashMap::new())), // REMOVED
        }
    }
    
    /// Update OAuth session (used in callback)
    pub fn update_session(&self, device_hash: &str, auth_token: &str, account_hash: &str, encryption_key: &str) -> Result<(), String> {
        debug!("Attempting to update auth session for device_hash: {}", device_hash);
        
        let mut sessions = self.app_state.auth_sessions.lock()
            .map_err(|e| format!("Failed to acquire session lock: {}", e))?;
        
        // log current session list
        let session_ids: Vec<String> = sessions.keys().cloned().collect();
        debug!("Current active sessions: {:?}", session_ids);
        
        if let Some(session) = sessions.get_mut(device_hash) {
            // found existing session - update
            info!("Found existing session for device_hash: {}, updating with auth data", device_hash);
            info!("Before update - Session state: auth_token_present={}, account_hash_present={}, encryption_key_present={}", 
                session.auth_token.is_some(),
                session.account_hash.is_some(),
                session.encryption_key.is_some()
            );
            
            session.auth_token = Some(auth_token.to_string());
            session.account_hash = Some(account_hash.to_string());
            session.encryption_key = Some(encryption_key.to_string());
            
            info!("After update - Session state: auth_token_present={}, account_hash_present={}, encryption_key_present={}", 
                session.auth_token.is_some(),
                session.account_hash.is_some(),
                session.encryption_key.is_some()
            );
            
            info!("Found and updated existing session: device_hash={}, account={}", 
                device_hash, 
                account_hash
            );
            debug!("Session details updated: client_id={}, auth_token_len={}", 
                session.client_id,
                auth_token.len()
            );
        } else {
            // no existing session found - create new session
            info!("No existing session found for device_hash: {} - creating new session", device_hash);
            
            let now = Utc::now();
            let new_session = AuthSession {
                device_hash: device_hash.to_string(),
                client_id: format!("client_{}", uuid::Uuid::new_v4()),
                auth_token: Some(auth_token.to_string()),
                account_hash: Some(account_hash.to_string()),
                encryption_key: Some(encryption_key.to_string()),
                created_at: now,
                expires_at: now + chrono::Duration::hours(24),
            };
            
            sessions.insert(device_hash.to_string(), new_session);
            info!("Created new authenticated session for device_hash: {}", device_hash);
        }
        
        // check session status after update
        let session_exists = sessions.contains_key(device_hash);
        info!("After update - session exists: {}", session_exists);
        if session_exists {
            if let Some(session) = sessions.get(device_hash) {
                info!("Final verification - Session has complete auth data: auth_token={}, account_hash={}, encryption_key={}", 
                    session.auth_token.is_some(),
                    session.account_hash.is_some(),
                    session.encryption_key.is_some()
                );
            }
        }
        
        Ok(())
    }
    
    /// Clean expired sessions
    fn clean_expired_sessions(&self) -> Result<(), String> {
        let mut sessions = self.app_state.auth_sessions.lock()
            .map_err(|e| format!("Failed to acquire session lock during cleanup: {}", e))?;
        let now = Utc::now();
        
        // Identify expired sessions
        let expired_ids: Vec<String> = sessions.iter()
            .filter(|(_, session)| session.expires_at < now)
            .map(|(id, _)| id.clone())
            .collect();
            
        // Remove expired sessions
        for id in expired_ids {
            debug!("Removing expired session: {}", id);
            sessions.remove(&id);
        }
        Ok(())
    }
    
    /// Handle check auth status request - new OAuth flow
    pub async fn check_auth_status(&self, request: Request<CheckAuthStatusRequest>) -> Result<Response<CheckAuthStatusResponse>, Status> {
        let req = request.into_inner();
        let device_hash = req.device_hash;
        info!("Check auth status request for device_hash: {}", device_hash);
        
        // Clean expired sessions first
        if let Err(e) = self.clean_expired_sessions() {
            warn!("Failed to clean expired sessions: {}", e);
        }
        
        // log current session list
        {
            match self.app_state.auth_sessions.lock() {
                Ok(sessions) => {
                    let session_ids: Vec<String> = sessions.keys().cloned().collect();
                    debug!("Current active sessions during check: {:?}", session_ids);
                    debug!("Does requested device_hash exist in sessions? {}", sessions.contains_key(&device_hash));
                }
                Err(e) => {
                    error!("Failed to acquire session lock for logging: {}", e);
                }
            }
        }
        
        // Look up authentication session in the session map
        let session = match self.app_state.auth_sessions.lock() {
            Ok(sessions) => sessions.get(&device_hash).cloned(),
            Err(e) => {
                error!("Failed to acquire session lock for lookup: {}", e);
                return Err(Status::internal("Session access error"));
            }
        };
        
        match session {
            Some(session) => {
                debug!("Found existing session for device_hash: {}", device_hash);
                
                // Log detailed session information
                info!("Session details for device_hash {}: client_id={}, auth_token_present={}, account_hash_present={}, encryption_key_present={}", 
                    device_hash,
                    session.client_id,
                    session.auth_token.is_some(),
                    session.account_hash.is_some(),
                    session.encryption_key.is_some()
                );
                
                // Check if authentication is complete
                let is_complete = session.auth_token.is_some();
                info!("Authentication completion check for device_hash {}: is_complete={}", device_hash, is_complete);
                
                if is_complete {
                    info!("Authentication is complete for device_hash: {}", device_hash);
                    // Get expiration time in seconds
                    let now = Utc::now();
                    let expires_in = if session.expires_at > now {
                        (session.expires_at - now).num_seconds()
                    } else {
                        0
                    };
                    
                    let auth_token = session.auth_token.clone().unwrap_or_default();
                    let account_hash = session.account_hash.unwrap_or_default();
                    let encryption_key = session.encryption_key.unwrap_or_default();
                    
                    info!("Returning complete auth status for device_hash {}: token_length={}, account_hash={}, key_length={}", 
                        device_hash, auth_token.len(), account_hash, encryption_key.len());
                    
                    // Authentication is complete - return full information
                    let resp = CheckAuthStatusResponse {
                        is_complete: true,
                        success: true,
                        auth_token,
                        account_hash,
                        encryption_key,
                        expires_in: expires_in,
                        return_message: String::new(),
                        session_id: session.device_hash.clone(),
                    };
                    Ok(Response::new(resp))
                } else {
                    info!("Authentication not yet complete for device_hash: {}", device_hash);
                    // Authentication not yet complete
                    let resp = CheckAuthStatusResponse {
                        is_complete: false,
                        success: true,
                        auth_token: String::new(),
                        account_hash: String::new(),
                        encryption_key: String::new(),
                        expires_in: 0,
                        return_message: "Authentication not yet complete".to_string(),
                        session_id: String::new(),
                    };
                    Ok(Response::new(resp))
                }
            },
            None => {
                // Session not found
                if !device_hash.is_empty() {
                    // device hash provided but no session found - create new session
                    info!("Creating new auth session for device_hash: {}", device_hash);
                    let mut sessions = match self.app_state.auth_sessions.lock() {
                        Ok(sessions) => sessions,
                        Err(e) => {
                            error!("Failed to acquire session lock for new session creation: {}", e);
                            return Err(Status::internal("Session management error"));
                        }
                    };
                    
                    // create new session (24 hours valid)
                    let now = Utc::now();
                    let new_session = AuthSession {
                        device_hash: device_hash.clone(),
                        client_id: format!("client_{}", uuid::Uuid::new_v4()),
                        auth_token: None,
                        account_hash: None,
                        encryption_key: None,
                        created_at: now,
                        expires_at: now + chrono::Duration::hours(24),
                    };
                    
                    sessions.insert(device_hash.clone(), new_session);
                    debug!("New unauthenticated session created successfully");
                }
                
                // notify that session is not yet complete
                let resp = CheckAuthStatusResponse {
                    is_complete: false,
                    success: true, // session not found but not an error
                    auth_token: String::new(),
                    account_hash: String::new(),
                    encryption_key: String::new(),
                    expires_in: 0,
                    return_message: "Session not found or not yet complete".to_string(),
                    session_id: device_hash,
                };
                Ok(Response::new(resp))
            }
        }
    }
    


    /// Handle validate token request
    async fn handle_validate_token(
        &self,
        request: Request<ValidateTokenRequest>,
    ) -> Result<Response<ValidateTokenResponse>, Status> {
        let req = request.into_inner();
        let auth_token = req.token;
        debug!("Validate token request for token length: {}", auth_token.len());

        if auth_token.is_empty() {
            return Err(Status::invalid_argument("Token is required"));
        } else {
            match self.app_state.oauth.verify_token(&auth_token).await {
                Ok(result) => {
                    let response = ValidateTokenResponse {
                        is_valid: result.valid,
                        account_hash: result.account_hash,
                    };
                    Ok(Response::new(response))
                },
                Err(e) => {
                    error!("Token verification failed: {}", e);
                    let response = ValidateTokenResponse {
                        is_valid: false,
                        account_hash: String::new(),
                    };
                    Ok(Response::new(response))
                }
            }
        }
    }

    /// Handle login request - only OAuth is supported, direct login is not supported
    pub async fn login(&self, request: Request<LoginRequest>) -> Result<Response<LoginResponse>, Status> {
        warn!("Direct login method is deprecated. Please use OAuth authentication instead.");
        
        // response for unsupported feature
        let response = LoginResponse {
            success: false,
            auth_token: String::new(),
            account_hash: String::new(),
            return_message: "Direct login is not supported. Please use OAuth authentication.".to_string(),
        };
        
        Ok(Response::new(response))
    }
    
    /// Handle verify login request - only OAuth is supported, direct login is not supported
    pub async fn verify_login(&self, request: Request<VerifyLoginRequest>) -> Result<Response<VerifyLoginResponse>, Status> {
        warn!("Direct login verification is deprecated. Please use OAuth authentication instead.");
        
        // token verification is still needed, so the behavior is kept
        let req = request.into_inner();
        let auth_token = req.auth_token;
        
        debug!("Verify login request for token length: {}", auth_token.len());
        
        // basic token verification
        if auth_token.is_empty() {
            let response = VerifyLoginResponse {
                valid: false,
                account_hash: String::new(),
            };
            return Ok(Response::new(response));
        }
        
        // actual token verification
        match self.app_state.oauth.verify_token(&auth_token).await {
            Ok(result) => {
                let response = VerifyLoginResponse {
                    valid: result.valid,
                    account_hash: result.account_hash,
                };
                Ok(Response::new(response))
            },
            Err(_) => {
                let response = VerifyLoginResponse {
                    valid: false,
                    account_hash: String::new(),
                };
                Ok(Response::new(response))
            }
        }
    }

    /// Process auth success notification from client
    pub async fn process_auth_notification(&self, notification: &AuthSuccessNotification) -> Result<AuthNotificationResponse, String> {
        debug!("Processing auth success notification for session: {}", notification.session_id);
        
        // update session
        self.update_session(
            &notification.device_hash, 
            &notification.auth_token, 
            &notification.account_hash, 
            &notification.encryption_key
        );
        
        // directly process
        let token = crate::models::auth::AuthToken {
            token_id: notification.session_id.clone(),
            account_hash: notification.account_hash.clone(),
            access_token: notification.auth_token.clone(),
            refresh_token: None,
            created_at: chrono::Utc::now(),
            expires_at: chrono::Utc::now() + chrono::Duration::seconds(notification.expires_in),
            is_valid: true,
            token_type: "Bearer".to_string(),
            scope: None,
        };
        
        match self.app_state.storage.create_auth_token(&token).await {
            Ok(_) => {
                info!("Auth token saved successfully for session: {}", notification.session_id);
                Ok(AuthNotificationResponse {
                    success: true,
                    return_message: String::new(),
                })
            },
            Err(e) => {
                error!("Failed to save auth token: {}", e);
                Err(format!("Failed to save auth token: {}", e))
            }
        }
    }

    /// Method to automatically register or update device during OAuth process
    async fn auto_register_or_update_device(
        &self,
        account_hash: &str,
        device_hash: &str,
        os_version: &str,
        app_version: &str,
    ) -> Result<(), String> {
        use crate::models::device::Device;
        
        info!("Auto-registering/updating device: device_hash={}, account_hash={}", device_hash, account_hash);
        
        // current design: register as new device on format/reinstall (correct approach)
        // reason:
        // 1. data isolation: safe data separation before/after format
        // 2. sync history preservation: maintain sync history of previous devices
        // 3. incremental recovery: user can selectively recover only needed data
        
        // TODO: future improvement for user device management
        // 1. device management UI:
        //    - show active/inactive device list
        //    - automatically deactivate based on last sync time
        //    - user manual device removal/cleanup function
        // 2. smart notifications:
        //    - notify when new device_hash is detected on the same OS/hardware
        //    - provide "deactivate previous device?" option
        // 3. data migration:
        //    - copy settings from previous device to new device
        //    - automatically transfer watcher groups and sync settings
        
        // check if existing device exists (currently: check only by device_hash)
        match self.app_state.storage.get_device(account_hash, device_hash).await {
            Ok(Some(mut existing_device)) => {
                // update existing device info
                info!("Found existing device, updating info: device_hash={}", device_hash);
                
                existing_device.update_info(
                    Some(true), // activate
                    Some(os_version.to_string()),
                    Some(app_version.to_string()),
                );
                
                // update device info (register_device is upsert)
                self.app_state.storage.register_device(&existing_device).await
                    .map_err(|e| format!("Failed to update existing device: {}", e))?;
                
                info!("Successfully updated existing device: device_hash={}", device_hash);
            },
            Ok(None) => {
                // create new device (new device_hash)
                // naturally registered as new device on format/reinstall
                info!("Creating new device (format/reinstall/new installation): device_hash={}", device_hash);
                
                let new_device = Device::new(
                    account_hash.to_string(),
                    device_hash.to_string(),
                    true, // activate
                    os_version.to_string(),
                    app_version.to_string(),
                );
                
                // register new device
                self.app_state.storage.register_device(&new_device).await
                    .map_err(|e| format!("Failed to register new device: {}", e))?;
                
                info!("Successfully registered new device: device_hash={}", device_hash);
                
                // TODO: future improvement - automatically deactivate old devices
                // 1. automatically deactivate old devices
                // 2. provide option to copy settings from previous device to new device
                // 3. send device registration notification
            },
            Err(e) => {
                return Err(format!("Failed to check existing device: {}", e));
            }
        }
        
        Ok(())
    }

    /// Method to validate token internally (used in SyncServiceImpl::validate_auth)
    pub async fn validate_token_internal(&self, auth_token: &str, account_hash: &str) -> Result<(), Status> {
        // validate authentication token
        match self.app_state.oauth.verify_token(auth_token).await {
            Ok(auth_result) => {
                if !auth_result.valid || auth_result.account_hash != account_hash {
                    return Err(Status::unauthenticated("Invalid authentication token"));
                }
                Ok(())
            },
            Err(e) => {
                error!("Token validation error: {}", e);
                Err(Status::unauthenticated(format!("Token validation failed: {}", e)))
            }
        }
    }

    /// Get account info - get account info using authentication token
    pub async fn get_account_info(&self, request: Request<GetAccountInfoRequest>) -> Result<Response<GetAccountInfoResponse>, Status> {
        let req = request.into_inner();
        let auth_token = req.auth_token;
        
        if auth_token.is_empty() {
            return Err(Status::invalid_argument("Auth token is required"));
        }
        
        // verify token and get account hash
        match self.app_state.oauth.verify_token(&auth_token).await {
            Ok(result) => {
                if !result.valid {
                    return Err(Status::unauthenticated("Invalid authentication token"));
                }
                
                let account_hash = result.account_hash.clone();
                
                // get encryption key
                let encryption_key = match self.app_state.storage.get_encryption_key(&account_hash).await {
                    Ok(Some(key)) => key,
                    Ok(None) => String::new(),
                    Err(e) => {
                        error!("Failed to get encryption key: {}", e);
                        String::new()
                    }
                };
                
                let response = GetAccountInfoResponse {
                    success: true,
                    account_hash,
                    encryption_key,
                    return_message: String::new(),
                };
                
                Ok(Response::new(response))
            },
            Err(e) => {
                error!("Token verification failed: {}", e);
                
                Err(Status::unauthenticated(format!("Authentication failed: {}", e)))
            }
        }
    }
}

#[async_trait]
impl crate::services::Handler for AuthHandler {
    // define streaming return type
    type SubscribeToAuthUpdatesStream = std::pin::Pin<Box<dyn futures::Stream<Item = Result<AuthUpdateNotification, Status>> + Send + 'static>>;
    type SubscribeToDeviceUpdatesStream = std::pin::Pin<Box<dyn futures::Stream<Item = Result<DeviceUpdateNotification, Status>> + Send + 'static>>;
    type SubscribeToEncryptionKeyUpdatesStream = std::pin::Pin<Box<dyn futures::Stream<Item = Result<EncryptionKeyUpdateNotification, Status>> + Send + 'static>>;
    type SubscribeToFileUpdatesStream = std::pin::Pin<Box<dyn futures::Stream<Item = Result<FileUpdateNotification, Status>> + Send + 'static>>;
    type SubscribeToWatcherPresetUpdatesStream = std::pin::Pin<Box<dyn futures::Stream<Item = Result<WatcherPresetUpdateNotification, Status>> + Send + 'static>>;
    type SubscribeToWatcherGroupUpdatesStream = std::pin::Pin<Box<dyn futures::Stream<Item = Result<WatcherGroupUpdateNotification, Status>> + Send + 'static>>;
    type SubscribeToVersionUpdatesStream = std::pin::Pin<Box<dyn futures::Stream<Item = Result<VersionUpdateNotification, Status>> + Send + 'static>>;

    async fn handle_oauth_exchange(
        &self,
        req: Request<OAuthExchangeRequest>,
    ) -> Result<Response<OAuthExchangeResponse>, Status> {
        let req = req.into_inner();
        info!("OAuth code exchange request received, code length: {}", req.code.len());
        
        if req.code.is_empty() {
            warn!("OAuth code is empty");
            return Err(Status::invalid_argument("OAuth code is required"));
        }
        
        // check device_hash info
        let device_info_provided = !req.device_hash.is_empty();
        if device_info_provided {
            info!("Client device info received - device_hash: {}, os_version: {}, app_version: {}",
                  req.device_hash, req.os_version, req.app_version);
        }
        
        // Process OAuth code
        match process_oauth_code(&req.code, Arc::new(self.app_state.oauth.clone())).await {
            Ok((auth_token, account_hash, encryption_key)) => {
                info!("OAuth authentication successful: account={}", account_hash);
                
                // if device info is provided, automatically register/update device
                if device_info_provided {
                    if let Err(e) = self.auto_register_or_update_device(
                        &account_hash,
                        &req.device_hash,
                        &req.os_version,
                        &req.app_version
                    ).await {
                        warn!("Failed to auto-register/update device during OAuth: {}", e);
                        // device registration failure does not block OAuth success
                    } else {
                        info!("Device automatically registered/updated during OAuth for account: {}", account_hash);
                    }
                }
                
                // Create success response
                let resp = OAuthExchangeResponse {
                    success: true,
                    auth_token,
                    account_hash,
                    encryption_key: Some(encryption_key),
                    return_message: String::new(),
                };
                
                info!("OAuth exchange completed successfully for account: {}", resp.account_hash);
                Ok(Response::new(resp))
            },
            Err(e) => {
                error!("OAuth authentication error: {}", e);
                
                // Create error response
                let resp = OAuthExchangeResponse {
                    success: false,
                    auth_token: String::new(),
                    account_hash: String::new(),
                    encryption_key: None,
                    return_message: format!("OAuth code exchange failed: {}", e),
                };
                
                Ok(Response::new(resp))
            }
        }
    }
    
    async fn handle_login(
        &self,
        request: Request<LoginRequest>,
    ) -> Result<Response<LoginResponse>, Status> {
        self.login(request).await
    }
    
    async fn handle_verify_login(
        &self,
        request: Request<VerifyLoginRequest>,
    ) -> Result<Response<VerifyLoginResponse>, Status> {
        self.verify_login(request).await
    }
    
    async fn handle_get_account_info(
        &self,
        request: Request<GetAccountInfoRequest>,
    ) -> Result<Response<GetAccountInfoResponse>, Status> {
        self.get_account_info(request).await
    }
}

// HTTP handler functions
use actix_web::{web, HttpRequest, HttpResponse, Result as ActixResult};
use serde_json::json;

/// HTTP handler for checking auth status
pub async fn handle_check_auth_status(
    query: web::Query<crate::handlers::oauth::CheckAuthStatusQuery>,
    auth_handler: web::Data<AuthHandler>,
) -> ActixResult<HttpResponse> {
    let device_hash = &query.device_hash;
    debug!("Checking auth status for device_hash: {}", device_hash);
    
    // Create gRPC-style request to reuse existing logic
    use tonic::Request;
    use crate::sync::CheckAuthStatusRequest;
    
    let grpc_request = Request::new(CheckAuthStatusRequest {
        device_hash: device_hash.clone(),
        account_hash: String::new(), // 인증 확인 단계에서는 account_hash를 모르므로 빈 문자열 사용
    });
    
    match auth_handler.check_auth_status(grpc_request).await {
        Ok(grpc_response) => {
            let resp = grpc_response.into_inner();
            
            let response = if resp.is_complete && !resp.auth_token.is_empty() {
                info!("Authentication complete for device_hash: {}, returning full auth data", device_hash);
                crate::handlers::oauth::AuthStatusResponse {
                    authenticated: true,
                    token: Some(resp.auth_token.clone()),
                    error: None,
                    // 클라이언트의 auth.json 파일 생성을 위한 완전한 정보 제공
                    account_hash: Some(resp.account_hash.clone()),
                    encryption_key: Some(resp.encryption_key.clone()),
                    expires_in: Some(resp.expires_in),
                    session_id: Some(resp.session_id.clone()),
                }
            } else {
                debug!("Authentication not complete for device_hash: {}, is_complete={}, auth_token_empty={}", 
                    device_hash, resp.is_complete, resp.auth_token.is_empty());
                crate::handlers::oauth::AuthStatusResponse {
                    authenticated: false,
                    token: None,
                    error: Some("Authentication not complete".to_string()),
                    account_hash: None,
                    encryption_key: None,
                    expires_in: None,
                    session_id: None,
                }
            };
            
            debug!("Auth status check result for {}: authenticated={}", device_hash, response.authenticated);
            Ok(HttpResponse::Ok().json(response))
        },
        Err(e) => {
            error!("Failed to check auth status: {}", e);
            let response = crate::handlers::oauth::AuthStatusResponse {
                authenticated: false,
                token: None,
                error: Some(format!("Failed to check authentication status: {}", e)),
                account_hash: None,
                encryption_key: None,
                expires_in: None,
                session_id: None,
            };
            Ok(HttpResponse::Ok().json(response))
        }
    }
}

/// HTTP handler for registering auth session
pub async fn handle_register_session(
    request: web::Json<crate::handlers::oauth::SessionRegistrationRequest>,
    state: web::Data<Arc<AppState>>,
    auth_handler: web::Data<AuthHandler>,
) -> ActixResult<HttpResponse> {
    info!("Session registration request for device_hash: {}", request.device_hash);
    
    let device_hash = &request.device_hash;
    let client_id = &request.client_id;
    
    // Create session in advance to ensure it exists when OAuth callback is received
    {
        let mut sessions = match state.auth_sessions.lock() {
            Ok(sessions) => sessions,
            Err(e) => {
                error!("Failed to acquire session lock during registration: {}", e);
                return Ok(HttpResponse::InternalServerError().json(crate::handlers::oauth::SessionRegistrationResponse {
                    success: false,
                    message: "Session management error".to_string(),
                    session_id: None,
                    auth_url: None,
                }));
            }
        };
        
        // Check if session already exists
        if sessions.contains_key(device_hash) {
            info!("Session already exists for device_hash: {}", device_hash);
        } else {
            // Create new unauthenticated session
            let now = chrono::Utc::now();
            let new_session = crate::server::app_state::AuthSession {
                device_hash: device_hash.clone(),
                client_id: client_id.clone(),
                auth_token: None,
                account_hash: None,
                encryption_key: None,
                created_at: now,
                expires_at: now + chrono::Duration::hours(24),
            };
            
            sessions.insert(device_hash.clone(), new_session);
            info!("Pre-created unauthenticated session for device_hash: {}", device_hash);
        }
    }
    
    // Generate OAuth login URL with device_hash
    let auth_url = state.oauth.generate_oauth_login_url_with_device(device_hash);
    
    let response = crate::handlers::oauth::SessionRegistrationResponse {
        success: true,
        message: "Session registered successfully".to_string(),
        session_id: Some(device_hash.clone()),
        auth_url: Some(auth_url),
    };
    
    info!("Session registration successful for device_hash: {}", device_hash);
    Ok(HttpResponse::Ok().json(response))
}