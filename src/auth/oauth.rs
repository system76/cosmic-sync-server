use tracing::{debug, error, info, warn};
use reqwest::Client;
use serde::Deserialize;
use crate::{
    models::auth::AuthToken,
    models::account::Account,
    storage::Storage,
    utils::crypto::{generate_account_hash_from_email, generate_account_hash_from_email_only, test_account_hash_generation},
};
use uuid::Uuid;
use sha2::{Sha256, Digest};
use rand::{RngCore, rngs::OsRng};
use std::sync::Arc;
use chrono::{DateTime, Utc};
use hex;
use std::time::{SystemTime, UNIX_EPOCH};
use rand::Rng;
use crate::auth::token::{generate_auth_token, generate_state_token, generate_session_token, extract_account_hash};
use crate::models::device::Device;
use crate::auth::{AuthError, Result};
use std::collections::HashMap;
use tokio::sync::RwLock;

// OAuth token response structure
#[derive(Debug, Deserialize)]
struct OAuthTokenResponse {
    access_token: String,
    token_type: String,
    expires_in: u64,
    #[serde(default)]
    refresh_token: Option<String>,
}

/// OAuth user info - System76 API structure
#[derive(Debug, Deserialize, Clone)]
pub struct OAuthUserInfo {
    pub id: String,
    pub user_id: String,
    pub name: String,
}

/// System76 API settings response structure
#[derive(Debug, Deserialize)]
struct System76UserSettings {
    pub user: Option<UserInfo>,
}

#[derive(Debug, Deserialize)]
struct UserInfo {
    #[serde(default)]
    pub id: Option<i32>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub first_name: Option<String>,
    #[serde(default)]
    pub last_name: Option<String>,
    #[serde(default)]
    pub company_name: Option<String>,
    #[serde(default)]
    pub newsletter: Option<bool>,
    #[serde(default)]
    pub staff: Option<bool>,
    #[serde(default)]
    pub stripe_id: Option<String>,
    #[serde(default)]
    pub third_party_login: Option<bool>,
    #[serde(default)]
    pub two_factor_enabled: Option<bool>,
    #[serde(default)]
    pub r#type: Option<String>,
    #[serde(default)]
    pub notification_preferences: Option<NotificationPreferences>,
    #[serde(default)]
    pub phone_number: Option<String>,
}

#[derive(Debug, Deserialize)]
struct NotificationPreferences {
    #[serde(default)]
    pub two_factor: Option<String>,
}

/// Token verification result
#[derive(Debug)]
pub struct VerificationResult {
    pub valid: bool,
    pub account_hash: String,
    pub expiry: Option<DateTime<Utc>>,
}

/// OAuth authentication service
#[derive(Clone)]
pub struct OAuthService {
    storage: Arc<dyn Storage>,
    client_id: String,
    client_secret: String,
    redirect_uri: String,
    auth_url: String,
    token_url: String,
    user_info_url: String,
    scope: String,
}

impl OAuthService {
    /// create new OAuth service
    pub fn new(storage: Arc<dyn Storage>) -> Self {
        let client_id = std::env::var("OAUTH_CLIENT_ID")
            .unwrap_or_else(|_| "cosmic-sync".to_string());
            
        let client_secret = std::env::var("OAUTH_CLIENT_SECRET")
            .unwrap_or_else(|_| "cosmicsecretsocmicsecret".to_string());
            
        let redirect_uri = std::env::var("OAUTH_REDIRECT_URI")
            .unwrap_or_else(|_| "http://localhost:8080/oauth/callback".to_string());
            
        let auth_url = std::env::var("OAUTH_AUTH_URL")
            .unwrap_or_else(|_| "https://localhost:4000/oauth/authorize".to_string());
            
        let token_url = std::env::var("OAUTH_TOKEN_URL")
            .unwrap_or_else(|_| "https://localhost:4000/oauth/token".to_string());
            
        let user_info_url = std::env::var("OAUTH_USER_INFO_URL")
            .unwrap_or_else(|_| "https://localhost:4000/userinfo".to_string());
            
        let scope = std::env::var("OAUTH_SCOPE")
            .unwrap_or_else(|_| "profile:read".to_string());
        
        // 디버깅을 위한 로그 추가
        info!("🔧 OAuth 서비스 초기화:");
        info!("  OAUTH_CLIENT_ID: {}", client_id);
        info!("  OAUTH_CLIENT_SECRET: {}...", &client_secret[..std::cmp::min(8, client_secret.len())]);
        info!("  OAUTH_REDIRECT_URI: {}", redirect_uri);
        info!("  OAUTH_AUTH_URL: {}", auth_url);
        info!("  OAUTH_TOKEN_URL: {}", token_url);
        info!("  OAUTH_USER_INFO_URL: {}", user_info_url);
        info!("  OAUTH_SCOPE: {}", scope);
        
        Self { 
            storage,
            client_id,
            client_secret,
            redirect_uri,
            auth_url,
            token_url,
            user_info_url,
            scope,
        }
    }

    // 디버깅을 위한 getter 메서드들 추가
    pub fn get_client_id(&self) -> &str {
        &self.client_id
    }

    pub fn get_auth_url(&self) -> &str {
        &self.auth_url
    }

    pub fn get_redirect_uri(&self) -> &str {
        &self.redirect_uri
    }
    
    /// System76 OAuth login URL
    pub fn generate_oauth_login_url(&self) -> String {
        // generate state token (CSRF prevention)
        let state = generate_state_token();
        
        // Log the state token (session ID)
        info!("Generated OAuth login URL with session ID: {}", state);
        
        // generate OAuth login URL
        format!(
            "{}?client_id={}&redirect_uri={}&response_type=code&scope={}&access_type=offline&state={}",
            self.auth_url, self.client_id, self.redirect_uri, self.scope, state
        )
    }
    
    /// OAuth login URL with custom device_hash
    pub fn generate_oauth_login_url_with_device(&self, device_hash: &str) -> String {
        info!("Generated OAuth login URL with custom device_hash: {}", device_hash);
        format!("{}?client_id={}&redirect_uri={}&response_type=code&scope={}&state={}",
            self.auth_url, self.client_id, self.redirect_uri, self.scope, device_hash)
    }
    
    /// Hash a token for storage
    fn hash_token(&self, token: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(token.as_bytes());
        hex::encode(hasher.finalize())
    }
    
    /// Get user info from external auth server using token
    async fn get_user_info_from_external_server(&self, token: &str) -> Result<OAuthUserInfo> {
        let client = Client::new();
        let auth_server_url = std::env::var("AUTH_SERVER_URL")
            .unwrap_or_else(|_| "http://10.17.89.63:4000".to_string());
        
        // 응답 상태 코드에 따른 오류 처리 개선
        let response = match client
            .get(&format!("{}/api/user", auth_server_url))
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await {
                Ok(resp) => resp,
                Err(e) => return Err(AuthError::ExternalServiceError(format!("Failed to connect to auth server: {}", e)))
        };
        
        // HTTP 상태 코드에 따른 명확한 오류 처리
        let status = response.status();
        if status.is_client_error() {
            return Err(AuthError::AuthenticationError(format!(
                "Authentication failed: {}",
                status
            )));
        } else if !status.is_success() {
            return Err(AuthError::ExternalServiceError(format!(
                "Auth server returned error: {}",
                status
            )));
        }
        
        let response_text = response.text().await
            .map_err(|e| AuthError::ExternalServiceError(format!("Failed to read response: {}", e)))?;
        
        info!("📋 Settings API response: {}", response_text);
        
        // Try to parse the response
        match serde_json::from_str::<System76UserSettings>(&response_text) {
            Ok(settings) => {
                // 사용자 데이터가 없는 경우 명시적으로 처리
                if settings.user.is_none() {
                    return Err(AuthError::UserNotFound("User not found or not available".to_string()));
                }
                
                let user = settings.user.as_ref().unwrap();
                
                // 필수 필드가 모두 있는지 확인
                let user_id = user.email.clone()
                    .ok_or_else(|| AuthError::MissingUserData("Email is required but not provided".to_string()))?;
                    
                let name = match (user.first_name.as_ref(), user.last_name.as_ref()) {
                    (Some(first), Some(last)) => format!("{} {}", first, last),
                    (Some(first), None) => first.clone(),
                    (None, Some(last)) => last.clone(),
                    _ => user_id.clone(),
                };
                
                let id = user.id.map_or(user_id.clone(), |id| id.to_string());
                
                info!("✅ Successfully obtained user info: user_id={}, name={}", user_id, name);
                Ok(OAuthUserInfo { id, user_id, name })
            },
            Err(e) => {
                // 디버깅을 위한 더 많은 컨텍스트 추가
                error!("Failed to parse user settings: {}, response: {}", e, response_text);
                Err(AuthError::InvalidResponseFormat(format!("Failed to parse user info: {}", e)))
            }
        }
    }
    
    /// validate token and return account hash
    pub async fn validate_token(&self, token: &str) -> Result<String> {
        if token.is_empty() {
            error!("❌ Token is empty");
            return Err(AuthError::InvalidToken("Token is empty".to_string()));
        }
        
        debug!("🔍 Validating auth token: token_prefix={}", &token[..std::cmp::min(10, token.len())]);
        
        // Special handling for test tokens only (distinguish from real OAuth users)
        if cfg!(debug_assertions) && token == "test_token" {
            warn!("🧪 Special test token detected, returning test account hash");
            return Ok("test_account_hash".to_string());
        }
        
        // Query token from storage
        match self.storage.get_auth_token(token).await {
            Ok(Some(token_obj)) => {
                debug!("🔍 Token object found: account_hash={}, expires_at={}", 
                       token_obj.account_hash, token_obj.expires_at);
                
                // Check token expiration
                let now = Utc::now();
                if token_obj.expires_at < now {
                    error!("❌ Token has expired: expires_at={}, now={}", token_obj.expires_at, now);
                    return Err(AuthError::InvalidToken("Token has expired".to_string()));
                }
                
                if !token_obj.is_valid {
                    error!("❌ Token is deactivated: account_hash={}", token_obj.account_hash);
                    return Err(AuthError::InvalidToken("Token is deactivated".to_string()));
                }
                
                debug!("✅ Token validation successful: account_hash={}", token_obj.account_hash);
                Ok(token_obj.account_hash)
            },
            Ok(None) => {
                error!("❌ Token not found: token_prefix={}", &token[..std::cmp::min(10, token.len())]);
                Err(AuthError::InvalidToken("Token not found".to_string()))
            },
            Err(e) => {
                error!("❌ Database error during token lookup: {}", e);
                Err(AuthError::DatabaseError(e.to_string()))
            }
        }
    }
    
    /// validate token and return result (used in handler)
    pub async fn verify_token(&self, token: &str) -> Result<VerificationResult> {
        match self.validate_token(token).await {
            Ok(account_hash) => {
                debug!("✅ Token validation successful: account_hash={}", account_hash);
                
                // Check if account exists in local DB
                match self.storage.get_account_by_hash(&account_hash).await {
                    Ok(Some(_)) => {
                        // Account exists, proceed normally
                        debug!("✅ Account exists in local DB: account_hash={}", account_hash);
                    },
                    Ok(None) => {
                        // Account doesn't exist in local DB, try to fetch from external auth server
                        info!("🔄 Account not found in local DB, attempting to fetch from external auth server: account_hash={}", account_hash);
                        
                        // Try to get user info from external auth server using the token
                        match self.get_user_info_from_external_server(token).await {
                            Ok(user_info) => {
                                // Create account in local DB
                                let email = if user_info.user_id.contains('@') {
                                    user_info.user_id.clone()
                                } else {
                                    format!("{}@example.com", user_info.user_id)
                                };
                                
                                // 중요: 외부 인증 서버의 계정 해시를 사용
                                // 이 부분이 핵심입니다 - 클라이언트가 제공한 account_hash를 그대로 사용
                                
                                let now = Utc::now();
                                let new_account = Account {
                                    account_hash: account_hash.clone(), // 클라이언트의 account_hash 사용
                                    user_id: user_info.user_id.clone(),
                                    name: user_info.name.clone(),
                                    email,
                                    id: Uuid::new_v4().to_string(),
                                    user_type: "oauth".to_string(),
                                    password_hash: String::new(),
                                    salt: String::new(),
                                    is_active: true,
                                    created_at: now,
                                    updated_at: now,
                                    last_login: now,
                                };
                                
                                match self.storage.create_account(&new_account).await {
                                    Ok(_) => {
                                        info!("✅ Account auto-created from external auth server: account_hash={}", account_hash);
                                    },
                                    Err(e) => {
                                        error!("❌ Failed to auto-create account: {}", e);
                                        // 계속 진행 - 계정은 외부 서버에 있을 수 있음
                                    }
                                }
                            },
                            Err(e) => {
                                warn!("⚠️ Could not fetch user info from external server: {}. Proceeding with token validation anyway.", e);
                                // 계속 진행 - 계정은 외부 서버에 있을 수 있음
                            }
                        }
                    }
                    Err(e) => {
                        error!("❌ Error checking account existence: {}", e);
                    }
                }
                
                Ok(VerificationResult {
                    valid: true,
                    account_hash,
                    expiry: None,
                })
            },
            Err(_) => {
                Ok(VerificationResult {
                    valid: false,
                    account_hash: String::new(),
                    expiry: None,
                })
            }
        }
    }
    
    /// exchange oauth code and authenticate
    pub async fn exchange_oauth_code_and_authenticate(&self, code: &str) -> Result<(String, OAuthUserInfo)> {
        // 1. exchange code to access token
        let token = self.exchange_oauth_code(code).await?;
        
        // 2. get user info with access token
        let user_info = self.get_user_info_with_token(&token).await?;
        
        Ok((token, user_info))
    }
    
    /// exchange oauth code to access token
    pub async fn exchange_oauth_code(&self, code: &str) -> Result<String> {
        // create http client
        let client = Client::new();
        
        // token request parameters
        let params = [
            ("code", code),
            ("client_id", &self.client_id),
            ("client_secret", &self.client_secret),
            ("redirect_uri", &self.redirect_uri),
            ("grant_type", "authorization_code"),
        ];
        
        #[derive(Deserialize)]
        struct TokenResponse {
            access_token: String,
            #[serde(default)]
            refresh_token: Option<String>,
            #[serde(default)]
            expires_in: Option<u64>,
        }
        
        // send token request
        info!("🔄 Starting OAuth token exchange request");
        
        // send token request to OAuth token URL
        match client.post(&self.token_url)
            .form(&params)
            .send()
            .await {
                Ok(response) => {
                    if response.status().is_success() {
                        match response.json::<TokenResponse>().await {
                            Ok(token_data) => {
                                info!("✅ OAuth token exchange successful");
                                Ok(token_data.access_token)
                            },
                            Err(e) => {
                                error!("❌ Failed to parse OAuth token response: {}", e);
                                Err(AuthError::ExternalServiceError(format!("Failed to parse token response: {}", e)))
                            }
                        }
                    } else {
                        let status = response.status();
                        let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
                        error!("❌ OAuth token exchange failed: {} - {}", status, error_text);
                        Err(AuthError::ExternalServiceError(format!("OAuth token exchange failed: {} - {}", status, error_text)))
                    }
                },
                Err(e) => {
                    error!("❌ Failed to send OAuth token request: {}", e);
                    Err(AuthError::ExternalServiceError(format!("Failed to send token request: {}", e)))
                }
            }
    }
    
    /// get user info with access token
    async fn get_user_info_with_token(&self, access_token: &str) -> Result<OAuthUserInfo> {
        // create http client
        let client = Client::new();
        
        info!("🔄 Starting user settings info request (/api/settings)");
        
        // Request user settings information
        match client.get(&self.user_info_url)
            .header("Authorization", format!("Bearer {}", access_token))
            .send()
            .await {
                Ok(response) => {
                    if response.status().is_success() {
                        // First check the response text
                        let response_text = response.text().await.unwrap_or_else(|_| "Unable to read response".to_string());
                        info!("📋 Settings API response: {}", response_text);
                        
                        // Try JSON parsing
                        match serde_json::from_str::<System76UserSettings>(&response_text) {
                            Ok(settings) => {
                                // Extract user info from settings
                                let user = settings.user.as_ref();
                                
                                // 이메일을 우선으로 user_id 설정
                                let user_id = user.as_ref()
                                    .and_then(|u| u.email.clone())
                                    .unwrap_or_else(|| "unknown_user@example.com".to_string());
                                
                                // first_name과 last_name을 조합하여 이름 생성
                                let name = user.as_ref()
                                    .and_then(|u| {
                                        match (u.first_name.as_ref(), u.last_name.as_ref()) {
                                            (Some(first), Some(last)) => Some(format!("{} {}", first, last)),
                                            (Some(first), None) => Some(first.clone()),
                                            (None, Some(last)) => Some(last.clone()),
                                            _ => None,
                                        }
                                    })
                                    .unwrap_or(user_id.clone());
                                
                                // ID 설정
                                let id = user.as_ref()
                                    .map(|u| u.id.map_or_else(|| user_id.clone(), |id| id.to_string()))
                                    .unwrap_or_else(|| user_id.clone());
                                
                                info!("✅ Successfully obtained user info: user_id={}, name={}", user_id, name);
                                
                                Ok(OAuthUserInfo {
                                    id,
                                    user_id,
                                    name,
                                })
                            },
                            Err(e) => {
                                error!("❌ Failed to parse settings response: {}", e);
                                Err(AuthError::ExternalServiceError(format!("Failed to parse settings response: {}", e)))
                            }
                        }
                    } else {
                        let status = response.status();
                        let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
                        error!("❌ Settings info request failed: {} - {}", status, error_text);
                        Err(AuthError::ExternalServiceError(format!("Failed to fetch settings: {} - {}", status, error_text)))
                    }
                },
                Err(e) => {
                    error!("❌ Failed to send settings request: {}", e);
                    Err(AuthError::ExternalServiceError(format!("Failed to send settings request: {}", e)))
                }
            }
    }
}

/// exchange oauth code and process user authentication and account info
pub async fn process_oauth_code(
    code: &str,
    oauth_service: Arc<OAuthService>,
    client_account_hash: Option<&str>, // 클라이언트가 제공한 account_hash (있는 경우)
) -> Result<(String, String, String)> {
    info!("🚀 Starting process_oauth_code");
    info!("  OAuth code: {}", code);
    info!("  Client account_hash: {:?}", client_account_hash);
    
    // 1. exchange oauth code to access token and get user info
    let (_access_token, user_info) = match oauth_service.exchange_oauth_code_and_authenticate(code).await {
        Ok(result) => {
            info!("✅ OAuth exchange successful");
            result
        },
        Err(e) => {
            error!("❌ OAuth authentication failed: {}", e);
            return Err(AuthError::ExternalServiceError(format!("OAuth authentication failed: {}", e)));
        }
    };
    
    info!("✅ User authentication completed: user_id={}, name={}, id={}", 
         user_info.user_id, user_info.name, user_info.id);
    
    // 2. 이메일만으로 account_hash 생성 (클라이언트와 호환성 유지)
    let email = if user_info.user_id.contains('@') {
        user_info.user_id.clone()
    } else {
        format!("{}@example.com", user_info.user_id)
    };
    
    // 클라이언트의 해시 생성 방식을 파악하기 위한 테스트
    test_account_hash_generation(&email, &user_info.name, &user_info.user_id);
    
    // 두 가지 방식으로 account_hash 생성 (호환성 유지)
    let account_hash_legacy = generate_account_hash_from_email(&user_info.user_id, &user_info.name);
    let account_hash_email = generate_account_hash_from_email_only(&email);
    
    // 클라이언트가 제공한 account_hash가 있으면 우선 사용
    let account_hash = if let Some(client_hash) = client_account_hash {
        info!("🔑 Using client-provided account_hash: {}", client_hash);
        client_hash.to_string()
    } else {
        info!("🔑 No client account_hash provided, using email-based hash: {}", account_hash_email);
        account_hash_email.clone()
    };
    
    info!("📊 Hash comparison:");
    info!("  Client expected: 209f313bf330cf40fe89fae938babbeba7ec95d31237f77cf19de418c0d50a0a");
    info!("  Server legacy: {}", account_hash_legacy);
    info!("  Server email: {}", account_hash_email);
    info!("  Using: {}", account_hash);
    
    // 3. 계정 조회 및 생성/업데이트
    let account_obj = match oauth_service.storage.get_account_by_hash(&account_hash).await {
        Ok(Some(account)) => {
            info!("🔄 Existing account found with hash: account_hash={}", account_hash);
            // Update last login time
            let mut updated_account = account.clone();
            updated_account.last_login = Utc::now();
            if let Err(e) = oauth_service.storage.update_account(&updated_account).await {
                error!("Error updating account last login: {}", e);
            }
            updated_account
        },
        Ok(None) => {
            // 이메일 기반 해시로 계정 조회 시도
            match oauth_service.storage.get_account_by_hash(&account_hash_email).await {
                Ok(Some(email_account)) => {
                    info!("🔄 Existing account found with email hash, updating to client hash");
                    
                    // 클라이언트 해시로 계정 업데이트
                    let mut updated_account = email_account.clone();
                    updated_account.account_hash = account_hash.clone();
                    updated_account.last_login = Utc::now();
                    
                    // 계정 업데이트
                    if let Err(e) = oauth_service.storage.update_account(&updated_account).await {
                        error!("Error updating account hash: {}", e);
                    } else {
                        info!("✅ Successfully updated account from email hash to client hash");
                    }
                    
                    updated_account
                },
                _ => {
                    // 레거시 해시로 계정 조회 시도
                    match oauth_service.storage.get_account_by_hash(&account_hash_legacy).await {
                        Ok(Some(legacy_account)) => {
                            info!("🔄 Existing account found with legacy hash, updating to client hash");
                            
                            // 클라이언트 해시로 계정 업데이트
                            let mut updated_account = legacy_account.clone();
                            updated_account.account_hash = account_hash.clone();
                            updated_account.last_login = Utc::now();
                            
                            // 계정 업데이트
                            if let Err(e) = oauth_service.storage.update_account(&updated_account).await {
                                error!("Error updating account hash: {}", e);
                            } else {
                                info!("✅ Successfully updated account from legacy hash to client hash");
                            }
                            
                            updated_account
                        },
                        _ => {
                            info!("✨ Creating new account: account_hash={}, email={}", account_hash, email);
                            // Create new account if not exists
                            let now = Utc::now();
                            
                            let new_account = Account {
                                account_hash: account_hash.clone(),
                                user_id: user_info.user_id.clone(),
                                name: user_info.name.clone(),
                                email,
                                id: Uuid::new_v4().to_string(),
                                user_type: "oauth".to_string(),
                                password_hash: String::new(),
                                salt: String::new(),
                                is_active: true,
                                created_at: now,
                                updated_at: now,
                                last_login: now,
                            };
                            
                            // Save account
                            match oauth_service.storage.create_account(&new_account).await {
                                Ok(_) => {
                                    info!("✅ New account created successfully: account_hash={}", account_hash);
                                    
                                    // 계정 생성 확인
                                    match oauth_service.storage.get_account_by_hash(&account_hash).await {
                                        Ok(Some(_)) => {
                                            info!("✅ Verified account was saved to database: account_hash={}", account_hash);
                                        },
                                        Ok(None) => {
                                            error!("❌ Account creation verification failed: account not found in database after creation");
                                        },
                                        Err(e) => {
                                            error!("❌ Error verifying account creation: {}", e);
                                        }
                                    }
                                },
                                Err(e) => {
                                    error!("❌ Failed to create account: {}", e);
                                }
                            }
                            
                            new_account
                        }
                    }
                }
            }
        },
        Err(e) => {
            error!("Error getting account: {}", e);
            return Err(AuthError::DatabaseError(e.to_string()));
        }
    };
    
    // 4. generate auth token
    let auth_token = generate_auth_token();
    
    // Create and save auth token
    let token_obj = AuthToken {
        token_id: Uuid::new_v4().to_string(),
        account_hash: account_hash.clone(),
        access_token: auth_token.clone(),
        token_type: "Bearer".to_string(),
        refresh_token: None,
        created_at: Utc::now(),
        expires_at: Utc::now() + chrono::Duration::hours(24),
        is_valid: true,
        scope: None,
    };
    
    // Save auth token
    if let Err(e) = oauth_service.storage.create_auth_token(&token_obj).await {
        error!("Error saving auth token: {}", e);
        return Err(AuthError::DatabaseError(format!("Error saving auth token: {}", e)));
    }
    
    // 5. generate or get user's encryption key
    let encryption_key = match get_encryption_key(&account_hash, oauth_service.storage.clone()).await {
        Ok(Some(key)) => key,
        Ok(None) => String::new(),
        Err(e) => {
            error!("Failed to get encryption key: {}", e);
            String::new()
        }
    };
    
    Ok((auth_token, account_hash, encryption_key))
}

/// get user info with access token
pub async fn get_oauth_user_info(access_token: &str) -> Result<OAuthUserInfo> {
    // create http client
    let client = Client::new();
    
    info!("🔄 Starting user settings info request (standalone function)");
    
    // Get user info URL from environment variable
    let user_info_url = std::env::var("OAUTH_USER_INFO_URL")
        .unwrap_or_else(|_| "http://10.17.89.63:4000/api/settings".to_string());
    
    // Request settings information
    match client.get(&user_info_url)
        .header("Authorization", format!("Bearer {}", access_token))
        .send()
        .await {
            Ok(response) => {
                if response.status().is_success() {
                    // Check response text
                    let response_text = response.text().await.unwrap_or_else(|_| "Unable to read response".to_string());
                    info!("📋 Settings API response (standalone): {}", response_text);
                    
                    // Try JSON parsing
                    match serde_json::from_str::<System76UserSettings>(&response_text) {
                        Ok(settings) => {
                            // Extract user info from settings
                            let user = settings.user.as_ref();
                            
                            // 이메일을 우선으로 user_id 설정
                            let user_id = user.as_ref()
                                .and_then(|u| u.email.clone())
                                .unwrap_or_else(|| "unknown_user@example.com".to_string());
                            
                            // first_name과 last_name을 조합하여 이름 생성
                            let name = user.as_ref()
                                .and_then(|u| {
                                    match (u.first_name.as_ref(), u.last_name.as_ref()) {
                                        (Some(first), Some(last)) => Some(format!("{} {}", first, last)),
                                        (Some(first), None) => Some(first.clone()),
                                        (None, Some(last)) => Some(last.clone()),
                                        _ => None,
                                    }
                                })
                                .unwrap_or(user_id.clone());
                            
                            // ID 설정
                            let id = user.as_ref()
                                .map(|u| u.id.map_or_else(|| user_id.clone(), |id| id.to_string()))
                                .unwrap_or_else(|| user_id.clone());
                            
                            info!("✅ Successfully obtained user info (standalone): user_id={}, name={}", user_id, name);
                            
                            Ok(OAuthUserInfo {
                                id,
                                user_id,
                                name,
                            })
                        },
                        Err(e) => {
                            error!("❌ Failed to parse settings response (standalone): {}", e);
                            Err(AuthError::ExternalServiceError(format!("Failed to parse settings response: {}", e)))
                        }
                    }
                } else {
                    let status = response.status();
                    let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
                    error!("❌ Settings info request failed (standalone): {} - {}", status, error_text);
                    Err(AuthError::ExternalServiceError(format!("Settings request failed: {} - {}", status, error_text)))
                }
            },
            Err(e) => {
                error!("❌ Failed to send settings request (standalone): {}", e);
                Err(AuthError::ExternalServiceError(format!("Failed to send settings request: {}", e)))
            }
        }
}

/// get or generate user's encryption key
async fn get_encryption_key(account_hash: &str, storage: Arc<dyn Storage>) -> Result<Option<String>> {
    // get account encryption key
    match storage.get_encryption_key(account_hash).await {
        Ok(Some(key)) => {
            debug!("Found existing encryption key for account: {}", account_hash);
            return Ok(Some(key));
        },
        Ok(None) => {
            debug!("No encryption key found for account: {}, will generate a new one", account_hash);
        },
        Err(e) => {
            error!("Error fetching encryption key from storage: {}", e);
            // if error occurs, generate a new one
        }
    }
    
    // try to get encryption key from existing devices
    let devices = match storage.list_devices(account_hash).await {
        Ok(device_list) => device_list,
        Err(e) => {
            error!("Error getting devices for account {}: {}", account_hash, e);
            Vec::new()
        }
    };
    
    // if there are existing devices, print the first device id
    if !devices.is_empty() {
        let device_hash = &devices[0].device_hash;
        debug!("Using existing device for encryption key: {}", device_hash);
    }
    
    // generate a new encryption key
    let mut key = [0u8; 32];
    OsRng.fill_bytes(&mut key);
    let encryption_key = hex::encode(key);
    
    // store the new encryption key
    if let Err(e) = storage.store_encryption_key(account_hash, &encryption_key).await {
        error!("Failed to store new encryption key: {}", e);
    } else {
        debug!("Successfully stored new encryption key for account: {}", account_hash);
    }
    
    Ok(Some(encryption_key))
}