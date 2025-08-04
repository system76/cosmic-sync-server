use tracing::{debug, error, info, warn};
use reqwest::Client;
use serde::Deserialize;
use crate::models::{Account, AuthToken};
use crate::storage::Storage;
use uuid::Uuid;
use sha2::{Sha256, Digest};
use rand::{RngCore, rngs::OsRng};
use std::sync::Arc;
use chrono::Utc;
use hex;
use std::time::{SystemTime, UNIX_EPOCH};
use crate::auth::{AuthError, Result};
use rand::Rng;
use crate::utils::crypto::generate_account_hash_from_email;
use crate::auth::token::{generate_auth_token, generate_state_token, generate_session_token, extract_account_hash};
use crate::models::device::Device;

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
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub first_name: Option<String>,
    #[serde(default)]
    pub last_name: Option<String>,
}

/// token verification result
#[derive(Debug, Clone)]
pub struct TokenVerificationResult {
    pub valid: bool,
    pub account_hash: String,
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
    pub async fn verify_token(&self, token: &str) -> Result<TokenVerificationResult> {
        match self.validate_token(token).await {
            Ok(account_hash) => {
                Ok(TokenVerificationResult {
                    valid: true,
                    account_hash,
                })
            },
            Err(_) => {
                Ok(TokenVerificationResult {
                    valid: false,
                    account_hash: String::new(),
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
                                let user_id = settings.email.clone()
                                    .or_else(|| settings.username.clone())
                                    .unwrap_or_else(|| "unknown_user".to_string());
                                
                                let name = settings.name.clone()
                                    .or_else(|| {
                                        match (settings.first_name.as_ref(), settings.last_name.as_ref()) {
                                            (Some(first), Some(last)) => Some(format!("{} {}", first, last)),
                                            (Some(first), None) => Some(first.clone()),
                                            (None, Some(last)) => Some(last.clone()),
                                            _ => None
                                        }
                                    })
                                    .unwrap_or_else(|| user_id.clone());
                                
                                let id = settings.id.unwrap_or_else(|| user_id.clone());
                                
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
) -> Result<(String, String, String)> {
    // 1. exchange oauth code to access token and get user info
    let (_access_token, user_info) = match oauth_service.exchange_oauth_code_and_authenticate(code).await {
        Ok(result) => result,
        Err(e) => {
            error!("OAuth authentication failed: {}", e);
            return Err(AuthError::ExternalServiceError(format!("OAuth authentication failed: {}", e)));
        }
    };
    
    info!("✅ User authentication completed: {} ({})", user_info.user_id, user_info.id);
    
    // 2. hash user info to generate account hash
    let account_hash = generate_account_hash_from_email(&user_info.user_id, &user_info.name);
    
    // 3. save or update user info to db
    let _account_obj = match oauth_service.storage.get_account_by_hash(&account_hash).await {
        Ok(Some(account)) => {
            info!("🔄 Existing account found, updating login time: account_hash={}", account_hash);
            // Update last login time
            if let Err(e) = oauth_service.storage.update_account(&account).await {
                error!("Error updating account last login: {}", e);
            }
            account
        },
        Ok(None) => {
            info!("✨ Creating new account: account_hash={}, user_id={}", account_hash, user_info.user_id);
            // Create new account if not exists
            let now = Utc::now();
            
            // Generate Email field - check if user_id is already in email format
            let email = if user_info.user_id.contains('@') {
                // Use user_id as-is if it's already email format
                user_info.user_id.clone()
            } else {
                // Add @example.com if user_id is not email
                format!("{}@example.com", user_info.user_id)
            };
            
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
            if let Err(e) = oauth_service.storage.create_account(&new_account).await {
                error!("❌ Failed to create account: {}", e);
            } else {
                info!("✅ New account created successfully: account_hash={}", account_hash);
            }
            
            new_account
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
                            let user_id = settings.email.clone()
                                .or_else(|| settings.username.clone())
                                .unwrap_or_else(|| "unknown_user".to_string());
                            
                            let name = settings.name.clone()
                                .or_else(|| {
                                    match (settings.first_name.as_ref(), settings.last_name.as_ref()) {
                                        (Some(first), Some(last)) => Some(format!("{} {}", first, last)),
                                        (Some(first), None) => Some(first.clone()),
                                        (None, Some(last)) => Some(last.clone()),
                                        _ => None
                                    }
                                })
                                .unwrap_or_else(|| user_id.clone());
                            
                            let id = settings.id.unwrap_or_else(|| user_id.clone());
                            
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