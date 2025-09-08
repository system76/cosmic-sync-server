use tonic::{Request, Response, Status};
use tracing::{debug, error, info, warn};

use crate::server::service::SyncServiceImpl;
use crate::sync::{OAuthExchangeRequest, OAuthExchangeResponse};

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
                info!(
                    "OAuth code exchange successful for account: {}",
                    account_hash
                );

                // create success response
                Ok(Response::new(OAuthExchangeResponse {
                    success: true,
                    account_hash,
                    auth_token: req.auth_token.clone(),
                    encryption_key: None,
                    return_message: String::new(),
                }))
            }
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
use crate::auth::oauth::process_oauth_code;
use crate::handlers::auth_handler::AuthHandler;
use crate::server::app_state::{AppState, AuthSession};
use actix_web::{get, web, HttpRequest, HttpResponse, Result as ActixResult};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

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
            info!(
                "Generating OAuth URL with provided device_hash: {}",
                device_hash
            );
            state
                .oauth
                .generate_oauth_login_url_with_device(device_hash)
        }
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
    req: HttpRequest,
) -> ActixResult<HttpResponse> {
    // check for error
    if let Some(error) = &query.error {
        error!("OAuth callback error: {}", error);
        return Ok(HttpResponse::BadRequest().body(format!(
            "<html><body><h1>Authentication Failed</h1><p>Error: {}</p></body></html>",
            error
        )));
    }

    // Get device_hash from state parameter (CSRF token) or headers/cookies as fallback
    let device_hash = match &query.state {
        Some(token) => {
            info!("OAuth callback received with state/device_hash: {}", token);
            token.clone()
        }
        None => {
            // Try request headers first
            let header_device_hash = req
                .headers()
                .get("X-Device-Hash")
                .or_else(|| req.headers().get("X-Client-Device-Hash"))
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string());

            if let Some(h) = header_device_hash {
                info!(
                    "OAuth callback without state; using device_hash from header: {}",
                    h
                );
                h
            } else if let Some(cookie) = req.cookie("device_hash") {
                let v = cookie.value().to_string();
                info!(
                    "OAuth callback without state; using device_hash from cookie: {}",
                    v
                );
                v
            } else {
                warn!("OAuth callback without state token or device identifiers - generating temporary device_hash");
                let temp_device_hash = format!("temp_{}", chrono::Utc::now().timestamp());
                info!("Generated temporary device_hash: {}", temp_device_hash);
                temp_device_hash
            }
        }
    };

    info!(
        "Received OAuth callback with code: {} and device_hash: {}",
        query.code, device_hash
    );

    // 클라이언트 account_hash 추출 - 여러 방법 시도
    let client_account_hash = {
        // 1. 헤더에서 확인
        let from_header = req
            .headers()
            .get("X-Client-Account-Hash")
            .or_else(|| req.headers().get("X-Account-Hash"))
            .or_else(|| req.headers().get("Account-Hash"))
            .and_then(|v| v.to_str().ok());

        if let Some(hash) = from_header {
            info!("✅ Client account_hash from header: {}", hash);
            Some(hash.to_string())
        } else {
            // 2. 쿼리 파라미터에서 확인 (state에 포함되어 있을 수 있음)
            if let Some(state) = &query.state {
                // state가 account_hash일 수도 있음 (64자 16진수 문자열인지 확인)
                if state.len() == 64 && state.chars().all(|c| c.is_ascii_hexdigit()) {
                    info!("✅ Client account_hash from state parameter: {}", state);
                    Some(state.clone())
                } else {
                    None
                }
            } else {
                None
            }
        }
    };

    // Check if session exists before OAuth processing
    let session_exists_before = {
        match state.auth_sessions.lock() {
            Ok(sessions) => {
                let exists = sessions.contains_key(&device_hash);
                info!(
                    "Session exists before OAuth processing for device_hash {}: {}",
                    device_hash, exists
                );
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

    // OAuth 처리
    let oauth = Arc::new(state.oauth.clone());

    match process_oauth_code(&query.code, oauth, client_account_hash.as_deref()).await {
        Ok((auth_token, account_hash, encryption_key)) => {
            info!(
                "✅ OAuth authentication successful for account: {}",
                account_hash
            );

            // 모든 세션 리스트 가져오기 (디버깅용)
            let sessions = {
                let sessions = state.auth_sessions.lock().unwrap();
                let session_ids: Vec<String> = sessions.keys().cloned().collect();
                session_ids
            };
            info!("All active sessions before update: {:?}", sessions);

            // 기본적으로 콜백으로 받은 device_hash 세션 업데이트
            info!(
                "Attempting to update session for device_hash: {}",
                device_hash
            );
            match auth_handler.update_session(
                &device_hash,
                &auth_token,
                &account_hash,
                &encryption_key,
            ) {
                Ok(()) => {
                    info!(
                        "Successfully updated session for device_hash: {}",
                        device_hash
                    );

                    // Verify session was updated correctly
                    if let Ok(sessions) = state.auth_sessions.lock() {
                        if let Some(updated_session) = sessions.get(&device_hash) {
                            info!("Verification - Updated session has auth_token: {}, account_hash: {}", 
                                updated_session.auth_token.is_some(),
                                updated_session.account_hash.is_some()
                            );
                        } else {
                            error!(
                                "Session not found after update for device_hash: {}",
                                device_hash
                            );
                        }
                    }
                }
                Err(e) => {
                    error!(
                        "Failed to update session for device_hash {}: {}",
                        device_hash, e
                    );
                }
            }

            // 모든 미인증 세션에 대해 인증 정보 업데이트
            if device_hash.starts_with("temp_") {
                info!(
                    "Temporary device_hash detected. Attempting to update all incomplete sessions."
                );

                let pending_sessions = match state.auth_sessions.lock() {
                    Ok(sessions) => sessions
                        .iter()
                        .filter(|(id, session)| {
                            !id.starts_with("temp_") && session.auth_token.is_none()
                        })
                        .map(|(id, _)| id.clone())
                        .collect::<Vec<String>>(),
                    Err(e) => {
                        error!("Failed to acquire session lock for pending sessions: {}", e);
                        Vec::new()
                    }
                };

                info!(
                    "Found {} pending sessions to update",
                    pending_sessions.len()
                );

                for session_id in pending_sessions {
                    info!("Updating session with ID: {}", session_id);
                    if let Err(e) = auth_handler.update_session(
                        &session_id,
                        &auth_token,
                        &account_hash,
                        &encryption_key,
                    ) {
                        error!("Failed to update pending session {}: {}", session_id, e);
                    }
                }
            }

            // 인증 데이터를 JSON으로 포함한 성공 페이지 반환
            let html_response = format!(
                r#"<!DOCTYPE html>
                <html>
                <head>
                    <title>Authentication Successful</title>
                    <meta charset="utf-8">
                    <style>
                        body {{ 
                            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
                            display: flex;
                            align-items: center;
                            justify-content: center;
                            min-height: 100vh;
                            margin: 0;
                            background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
                        }}
                        .container {{
                            background: white;
                            padding: 40px;
                            border-radius: 12px;
                            box-shadow: 0 20px 60px rgba(0,0,0,0.3);
                            text-align: center;
                            max-width: 400px;
                        }}
                        .success-icon {{
                            width: 80px;
                            height: 80px;
                            margin: 0 auto 20px;
                            background: #4CAF50;
                            border-radius: 50%;
                            display: flex;
                            align-items: center;
                            justify-content: center;
                        }}
                        .checkmark {{
                            color: white;
                            font-size: 48px;
                        }}
                        h1 {{
                            color: #333;
                            margin-bottom: 10px;
                        }}
                        p {{
                            color: #666;
                            line-height: 1.6;
                        }}
                        .auth-data {{
                            background: #f5f5f5;
                            padding: 15px;
                            border-radius: 8px;
                            margin-top: 20px;
                            font-family: monospace;
                            font-size: 12px;
                            word-break: break-all;
                            text-align: left;
                        }}
                        .close-btn {{
                            margin-top: 20px;
                            padding: 10px 20px;
                            background: #667eea;
                            color: white;
                            border: none;
                            border-radius: 6px;
                            cursor: pointer;
                            font-size: 16px;
                        }}
                        .close-btn:hover {{
                            background: #5a67d8;
                        }}
                    </style>
                </head>
                <body>
                    <div class="container">
                        <div class="success-icon">
                            <div class="checkmark">✓</div>
                        </div>
                        <h1>Authentication Successful!</h1>
                        <p>Your account has been successfully authenticated. You can now close this window and return to the application.</p>
                        
                        <div class="auth-data">
                            <strong>Debug Information:</strong><br>
                            Account Hash: {}<br>
                            Token: {}...<br>
                            Device Hash: {}
                        </div>
                        
                        <button class="close-btn" onclick="window.close()">Close Window</button>
                    </div>
                    
                    <script>
                        // Store auth data for potential use by the application
                        const authData = {{
                            "auth_token": "{}",
                            "account_hash": "{}",
                            "encryption_key": "{}",
                            "device_hash": "{}"
                        }};
                        
                        console.log('Authentication successful:', authData);
                        
                        // Try to communicate with parent window if opened as popup
                        if (window.opener && !window.opener.closed) {{
                            window.opener.postMessage({{
                                type: 'oauth-success',
                                data: authData
                            }}, '*');
                        }}
                        
                        // Auto-close after 5 seconds
                        setTimeout(() => {{
                            window.close();
                        }}, 5000);
                    </script>
                </body>
                </html>"#,
                account_hash,
                &auth_token[..20.min(auth_token.len())],
                device_hash,
                auth_token,
                account_hash,
                encryption_key,
                device_hash
            );

            Ok(HttpResponse::Ok()
                .content_type("text/html; charset=utf-8")
                .body(html_response))
        }
        Err(e) => {
            error!("❌ OAuth authentication failed: {}", e);

            // Error HTML response
            let html_response = format!(
                r#"<!DOCTYPE html>
                <html>
                <head>
                    <title>Authentication Failed</title>
                    <meta charset="utf-8">
                    <style>
                        body {{ 
                            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
                            display: flex;
                            align-items: center;
                            justify-content: center;
                            min-height: 100vh;
                            margin: 0;
                            background: linear-gradient(135deg, #f093fb 0%, #f5576c 100%);
                        }}
                        .container {{
                            background: white;
                            padding: 40px;
                            border-radius: 12px;
                            box-shadow: 0 20px 60px rgba(0,0,0,0.3);
                            text-align: center;
                            max-width: 400px;
                        }}
                        .error-icon {{
                            width: 80px;
                            height: 80px;
                            margin: 0 auto 20px;
                            background: #f44336;
                            border-radius: 50%;
                            display: flex;
                            align-items: center;
                            justify-content: center;
                        }}
                        .error-mark {{
                            color: white;
                            font-size: 48px;
                        }}
                        h1 {{
                            color: #333;
                            margin-bottom: 10px;
                        }}
                        p {{
                            color: #666;
                            line-height: 1.6;
                        }}
                        .error-details {{
                            background: #fff3e0;
                            padding: 15px;
                            border-radius: 8px;
                            margin-top: 20px;
                            border-left: 4px solid #ff9800;
                            text-align: left;
                        }}
                        .retry-btn {{
                            margin-top: 20px;
                            padding: 10px 20px;
                            background: #f5576c;
                            color: white;
                            border: none;
                            border-radius: 6px;
                            cursor: pointer;
                            font-size: 16px;
                        }}
                        .retry-btn:hover {{
                            background: #e04865;
                        }}
                    </style>
                </head>
                <body>
                    <div class="container">
                        <div class="error-icon">
                            <div class="error-mark">✕</div>
                        </div>
                        <h1>Authentication Failed</h1>
                        <p>We couldn't complete the authentication process.</p>
                        
                        <div class="error-details">
                            <strong>Error Details:</strong><br>
                            {}
                        </div>
                        
                        <button class="retry-btn" onclick="window.location.href='/oauth/login'">Try Again</button>
                    </div>
                    
                    <script>
                        // Notify parent window of failure
                        if (window.opener && !window.opener.closed) {{
                            window.opener.postMessage({{
                                type: 'oauth-error',
                                error: '{}'
                            }}, '*');
                        }}
                    </script>
                </body>
                </html>"#,
                e, e
            );

            Ok(HttpResponse::InternalServerError()
                .content_type("text/html; charset=utf-8")
                .body(html_response))
        }
    }
}
