use std::sync::Arc;
use std::net::SocketAddr;
//use tonic::transport::Server;
use actix_web::{
    web, App, HttpServer, 
    HttpResponse, Responder,
    get, post, rt
};
use serde::{Deserialize, Serialize};
use tracing::{info, error, warn, debug};

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
}

/// Health check response structure
#[derive(Debug, Serialize)]
pub struct HealthCheckResponse {
    pub status: String,
    pub version: String,
}

/// OAuth login handler - redirects to OAuth provider
#[get("/oauth/login")]
pub async fn handle_oauth_login(
    query: web::Query<OAuthLoginParams>,
    state: web::Data<Arc<AppState>>,
) -> impl Responder {
    info!("OAuth login request received");
    
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
    HttpResponse::Found()
        .append_header(("Location", oauth_url))
        .finish()
}

/// OAuth callback handler
#[get("/oauth/callback")]
pub async fn handle_oauth_callback(
    query: web::Query<OAuthCallback>,
    state: web::Data<Arc<AppState>>,
    auth_handler: web::Data<AuthHandler>,
) -> impl Responder {
    // check for error
    if let Some(error) = &query.error {
        error!("OAuth callback error: {}", error);
        return HttpResponse::BadRequest().body(format!(
            "<html><body><h1>Authentication Failed</h1><p>Error: {}</p></body></html>",
            error
        ));
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
            
            HttpResponse::Ok()
                .content_type("text/html; charset=utf-8")
                .body(html_response)
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
            
            HttpResponse::BadRequest()
                .content_type("text/html; charset=utf-8")
                .body(html_response)
        }
    }
}

/// Handle session registration request
#[post("/api/auth/register_session")]
pub async fn handle_register_session(
    request: web::Json<SessionRegistrationRequest>,
    state: web::Data<Arc<AppState>>,
    auth_handler: web::Data<AuthHandler>,
) -> impl Responder {
    info!("Session registration request for device_hash: {}", request.device_hash);
    
    let device_hash = &request.device_hash;
    let client_id = &request.client_id;
    
    // Create session in advance to ensure it exists when OAuth callback is received
    {
        let mut sessions = match state.auth_sessions.lock() {
            Ok(sessions) => sessions,
            Err(e) => {
                error!("Failed to acquire session lock during registration: {}", e);
                return HttpResponse::InternalServerError().json(SessionRegistrationResponse {
                    success: false,
                    message: "Session management error".to_string(),
                    session_id: None,
                    auth_url: None,
                });
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
    
    let response = SessionRegistrationResponse {
        success: true,
        message: "Session registered successfully".to_string(),
        session_id: Some(device_hash.clone()),
        auth_url: Some(auth_url),
    };
    
    info!("Session registration successful for device_hash: {}", device_hash);
    HttpResponse::Ok().json(response)
}

/// Handle check auth status request
#[get("/api/auth/check_auth_status")]
pub async fn handle_check_auth_status(
    query: web::Query<CheckAuthStatusQuery>,
    auth_handler: web::Data<AuthHandler>,
) -> impl Responder {
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
                AuthStatusResponse {
                    authenticated: true,
                    token: Some(resp.auth_token),
                    error: None,
                }
            } else {
                AuthStatusResponse {
                    authenticated: false,
                    token: None,
                    error: None,
                }
            };
            
            debug!("Auth status check result for {}: authenticated={}", device_hash, response.authenticated);
            HttpResponse::Ok().json(response)
        },
        Err(e) => {
            error!("Auth status check failed for {}: {}", device_hash, e);
            let response = AuthStatusResponse {
                authenticated: false,
                token: None,
                error: Some(e.message().to_string()),
            };
            HttpResponse::Ok().json(response)
        }
    }
}

/// Health check endpoint
#[get("/health")]
pub async fn handle_health_check() -> impl Responder {
    debug!("Health check requested");
    
    let response = HealthCheckResponse {
        status: "SERVING".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    };
    
    HttpResponse::Ok().json(response)
} 