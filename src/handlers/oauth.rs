use tonic::{Request, Response, Status};
use tracing::{info, error, warn, debug};

use crate::sync::{OAuthExchangeRequest, OAuthExchangeResponse};
use crate::server::service::SyncServiceImpl;

// define OAuthHandler trait
#[tonic::async_trait]
pub trait OAuthHandler {
    async fn handle_oauth_exchange(
        &self,
        request: Request<OAuthExchangeRequest>,
    ) -> Result<Response<OAuthExchangeResponse>, Status>;
}

/// OAuth handler implementation for SyncServiceImpl
#[tonic::async_trait]
impl OAuthHandler for SyncServiceImpl {
    /// Handle OAuth code exchange request
    async fn handle_oauth_exchange(
        &self,
        request: Request<OAuthExchangeRequest>,
    ) -> Result<Response<OAuthExchangeResponse>, Status> {
        let req = request.into_inner();
        
        info!("Received OAuth code exchange request");
        
        // process OAuth code exchange through authentication service
        match self.app_state.oauth.exchange_oauth_code(&req.code).await {
            Ok(account_hash) => {
                info!("OAuth code exchange successful for account: {}", account_hash);
                
                // create success response
                Ok(Response::new(OAuthExchangeResponse {
                    success: true,
                    account_hash,
                    auth_token: req.auth_token.clone(),
                    encryption_key: None,
                    return_message: String::new(),
                }))
            },
            Err(error) => {
                error!("OAuth code exchange failed: {}", error);
                
                // create failure response
                Ok(Response::new(OAuthExchangeResponse {
                    success: false,
                    account_hash: String::new(),
                    auth_token: String::new(),
                    encryption_key: None,
                    return_message: error.to_string(),
                }))
            }
        }
    }
}

// HTTP handler functions
use actix_web::{web, HttpRequest, HttpResponse, Result as ActixResult, get};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use crate::server::app_state::{AppState, AuthSession};
use crate::auth::oauth::process_oauth_code;
use crate::handlers::auth_handler::AuthHandler;

/// OAuth callback parameter structure
#[derive(Debug, Deserialize)]
pub struct OAuthCallback {
    code: String,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

/// OAuth Login query parameters
#[derive(Debug, Deserialize)]
pub struct OAuthLoginParams {
    #[serde(default)]
    device_hash: Option<String>,
}

/// Check auth status query parameters
#[derive(Debug, Deserialize)]
pub struct CheckAuthStatusQuery {
    pub device_hash: String,
}

/// Auth status response
#[derive(Debug, Serialize)]
pub struct AuthStatusResponse {
    pub authenticated: bool,
    pub token: Option<String>,
    pub error: Option<String>,
    // 클라이언트의 auth.json 파일 생성을 위한 추가 필드들
    pub account_hash: Option<String>,
    pub encryption_key: Option<String>,
    pub expires_in: Option<i64>,
    pub session_id: Option<String>,
}

/// Session registration request
#[derive(Debug, Deserialize)]
pub struct SessionRegistrationRequest {
    pub device_hash: String,
    pub client_id: String,
}

/// Session registration response
#[derive(Debug, Serialize)]
pub struct SessionRegistrationResponse {
    pub success: bool,
    pub message: String,
    pub session_id: Option<String>,
    pub auth_url: Option<String>,
}

/// HTTP handler for OAuth login initiation
pub async fn handle_oauth_login(
    query: web::Query<OAuthLoginParams>,
    state: web::Data<Arc<AppState>>,
) -> ActixResult<HttpResponse> {
    println!("🚨🚨🚨 OAUTH LOGIN HANDLER CALLED!!! 🚨🚨🚨");
    info!("🚨🚨🚨 OAUTH LOGIN HANDLER CALLED!!! 🚨🚨🚨");
    info!("OAuth login request received");
    
    // 🔧 debug : OAuth service settings
    info!("🔧 OAuth service settings:");
    info!("  실제 client_id: {}", state.oauth.get_client_id());
    info!("  실제 auth_url: {}", state.oauth.get_auth_url());
    info!("  실제 redirect_uri: {}", state.oauth.get_redirect_uri());
    
    let oauth_url = match &query.device_hash {
        Some(device_hash) => {
            // 디바이스 해시가 제공된 경우 해당 해시를 state로 사용
            info!("Generating OAuth URL with provided device_hash: {}", device_hash);
            state.oauth.generate_oauth_login_url_with_device(device_hash)
        },
        None => {
            // 디바이스 해시가 없는 경우 새로 생성
            info!("Generating OAuth URL with new state token");
            state.oauth.generate_oauth_login_url()
        }
    };
    
    debug!("Redirecting to OAuth URL: {}", oauth_url);
    
    // OAuth 페이지로 리다이렉트
    Ok(HttpResponse::Found()
        .append_header(("Location", oauth_url))
        .finish())
}

/// HTTP handler for OAuth callback
pub async fn handle_oauth_callback(
    query: web::Query<OAuthCallback>,
    state: web::Data<Arc<AppState>>,
    auth_handler: web::Data<AuthHandler>,
) -> ActixResult<HttpResponse> {
    // check for error
    if let Some(error) = &query.error {
        error!("OAuth callback error: {}", error);
        return Ok(HttpResponse::BadRequest().body(format!(
            "<html><body><h1>Authentication Failed</h1><p>Error: {}</p></body></html>",
            error
        )));
    }
    
    // Get session ID from state parameter
    let device_hash = match &query.state {
        Some(token) => {
            info!("OAuth callback received with state/device_hash: {}", token);
            token.clone()
        },
        None => {
            // Generate a temporary session ID if missing
            warn!("OAuth callback without state token/device_hash - generating temporary device_hash");
            let temp_device_hash = format!("temp_{}", chrono::Utc::now().timestamp());
            info!("Generated temporary device_hash: {}", temp_device_hash);
            temp_device_hash
        }
    };
    
    // log authentication code
    info!("Received OAuth callback with code: {} and device_hash: {}", query.code, device_hash);
    
    // Check if session exists before OAuth processing
    let session_exists_before = {
        match state.auth_sessions.lock() {
            Ok(sessions) => {
                let exists = sessions.contains_key(&device_hash);
                info!("Session exists before OAuth processing for device_hash {}: {}", device_hash, exists);
                if exists {
                    if let Some(session) = sessions.get(&device_hash) {
                        info!("Existing session details: client_id={}, auth_token_present={}, account_hash_present={}", 
                            session.client_id,
                            session.auth_token.is_some(),
                            session.account_hash.is_some()
                        );
                    }
                }
                exists
            }
            Err(e) => {
                error!("Failed to check session existence: {}", e);
                false
            }
        }
    };

    // get OAuth service
    let oauth = Arc::new(state.oauth.clone());
    
    // process OAuth code
    match process_oauth_code(
        &query.code, 
        oauth
    ).await {
        Ok((auth_token, account_hash, encryption_key)) => {
            info!("OAuth authentication successful for account: {}", account_hash);
            
            // 모든 세션 리스트 가져오기 (디버깅용)
            let sessions = {
                let sessions = state.auth_sessions.lock().unwrap();
                let session_ids: Vec<String> = sessions.keys().cloned().collect();
                session_ids
            };
            info!("All active sessions before update: {:?}", sessions);
            
            // 기본적으로 콜백으로 받은 device_hash 세션 업데이트
            info!("Attempting to update session for device_hash: {}", device_hash);
            match auth_handler.update_session(&device_hash, &auth_token, &account_hash, &encryption_key) {
                Ok(()) => {
                    info!("Successfully updated session for device_hash: {}", device_hash);
                    
                    // Verify session was updated correctly
                    if let Ok(sessions) = state.auth_sessions.lock() {
                        if let Some(updated_session) = sessions.get(&device_hash) {
                            info!("Verification - Updated session has auth_token: {}, account_hash: {}", 
                                updated_session.auth_token.is_some(),
                                updated_session.account_hash.is_some()
                            );
                        } else {
                            error!("Session not found after update for device_hash: {}", device_hash);
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to update session for device_hash {}: {}", device_hash, e);
                }
            }
            
            // 모든 미인증 세션에 대해 인증 정보 업데이트
            if device_hash.starts_with("temp_") {
                info!("Temporary device_hash detected. Attempting to update all incomplete sessions.");
                
                let pending_sessions = match state.auth_sessions.lock() {
                    Ok(sessions) => {
                        sessions.iter()
                            .filter(|(id, session)| 
                                !id.starts_with("temp_") && 
                                session.auth_token.is_none())
                            .map(|(id, _)| id.clone())
                            .collect::<Vec<String>>()
                    }
                    Err(e) => {
                        error!("Failed to acquire session lock for pending sessions: {}", e);
                        Vec::new()
                    }
                };
                
                info!("Found {} pending sessions to update", pending_sessions.len());
                
                for session_id in pending_sessions {
                    info!("Updating session with ID: {}", session_id);
                    if let Err(e) = auth_handler.update_session(&session_id, &auth_token, &account_hash, &encryption_key) {
                        error!("Failed to update pending session {}: {}", session_id, e);
                    }
                }
            }
            
            // 세션 데이터를 JSON 형식으로 표시 - 클라이언트가 스크립트로 가져갈 수 있게 함
            let auth_data_json = format!(
                r#"{{
                    "auth_token": "{}",
                    "account_hash": "{}",
                    "encryption_key": "{}",
                    "device_hash": "{}"
                }}"#,
                auth_token, account_hash, encryption_key, device_hash
            );
            
            // login successful HTML response
            let html_response = format!(
                r#"
                <html>
                <head>
                    <title>Authentication Successful</title>
                    <style>
                        body {{ font-family: Arial, sans-serif; text-align: center; padding: 50px; }}
                        .success {{ color: green; font-size: 24px; margin-bottom: 20px; }}
                        .data {{ background-color: #f5f5f5; border-radius: 5px; padding: 10px; display: inline-block; text-align: left; }}
                    </style>
                </head>
                <body>
                    <div class="success">Authentication Successful!</div>
                    <p>You can now close this window and return to the application.</p>
                    <p>Your authentication has been automatically processed.</p>
                    <script>
                        // This data can be used by any scripts that need it
                        const authData = {auth_data_json};
                        console.log('Auth data:', authData);
                        
                        // You can add a postMessage here if needed to communicate with the opener window
                        // window.opener && window.opener.postMessage({{ type: 'auth-success', data: authData }}, '*');
                    </script>
                </body>
                </html>
                "#
            );
            
            Ok(HttpResponse::Ok()
                .content_type("text/html; charset=utf-8")
                .body(html_response))
        },
        Err(e) => {
            error!("OAuth authentication failed: {}", e);
            
            // Error HTML response
            let html_response = format!(
                r#"
                <html>
                <head>
                    <title>Authentication Failed</title>
                    <style>
                        body {{ font-family: Arial, sans-serif; text-align: center; padding: 50px; }}
                        .error {{ color: red; font-size: 24px; margin-bottom: 20px; }}
                    </style>
                </head>
                <body>
                    <div class="error">Authentication Failed</div>
                    <p>Error: {}</p>
                    <p>Please close this window and try again.</p>
                </body>
                </html>
                "#,
                e
            );
            
            Ok(HttpResponse::InternalServerError()
                .content_type("text/html; charset=utf-8")
                .body(html_response))
        }
    }
} 