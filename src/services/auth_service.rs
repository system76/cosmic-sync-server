use std::sync::Arc;
use tracing::{error, debug};
use crate::models::auth::AuthToken;
use crate::models::account::Account;
use crate::storage::{Storage, StorageError};
use crate::sync::{LoginResponse, VerifyLoginResponse, OAuthExchangeResponse, AuthSuccessNotification, AuthNotificationResponse};
use uuid::Uuid;
use chrono::{Utc, Duration};
use sha2::{Sha256, Digest};
use rand::Rng;
use base64::{Engine as _, engine::general_purpose};

/// Authentication service
pub struct AuthService {
    /// Storage backend
    storage: Arc<dyn Storage>,
}

impl AuthService {
    /// Create a new authentication service with the given storage backend
    pub fn new(storage: Arc<dyn Storage>) -> Self {
        Self { storage }
    }
    
    /// Generate a random salt for password hashing
    pub fn generate_salt(&self) -> String {
        let mut salt = [0u8; 16];
        rand::thread_rng().fill(&mut salt);
        general_purpose::STANDARD.encode(salt)
    }
    
    /// Hash a password with the given salt
    pub fn hash_password(&self, password: &str, salt: &str) -> String {
        let salted = format!("{}{}", password, salt);
        let mut hasher = Sha256::new();
        hasher.update(salted.as_bytes());
        let result = hasher.finalize();
        hex::encode(result)
    }
    
    /// Verify a password against a stored hash and salt
    pub fn verify_password(&self, password: &str, hash: &str, salt: &str) -> bool {
        let calculated_hash = self.hash_password(password, salt);
        calculated_hash == hash
    }
    
    /// Generate a token for the given account hash
    pub fn generate_token(&self, _account_hash: &str) -> String {
        let token_uuid = Uuid::new_v4();
        token_uuid.to_string()
    }
    
    /// Create a new auth token for the given account hash
    pub fn create_auth_token(&self, account_hash: &str) -> AuthToken {
        let token = self.generate_token(account_hash);
        let now = Utc::now();
        let expires_at = now + Duration::days(30);
        
        AuthToken {
            token_id: Uuid::new_v4().to_string(),
            access_token: token,
            account_hash: account_hash.to_string(),
            token_type: "Bearer".to_string(),
            expires_at: expires_at,
            created_at: now,
            refresh_token: None,
            is_valid: true,
            scope: None,
        }
    }
    
    /// Validate token against storage
    pub async fn validate_token(&self, token: &str, account_hash: &str) -> bool {
        debug!("AuthService: Validating token for account {}", account_hash);
        
        let storage = self.storage.clone();
        
        // Check if token is valid for this account
        match storage.as_ref().validate_auth_token(token, account_hash).await {
            Ok(valid) => valid,
            Err(e) => {
                error!("Error validating token: {}", e);
                false
            }
        }
    }
    
    /// Save authentication token to storage
    pub async fn save_token(&self, auth_token: &AuthToken) -> Result<(), StorageError> {
        debug!("AuthService: Saving token for account {}", auth_token.account_hash);
        
        let storage = self.storage.clone();
        
        // Save token to storage
        match storage.as_ref().create_auth_token(auth_token).await {
            Ok(_) => Ok(()),
            Err(e) => {
                error!("Error saving token: {}", e);
                Err(e)
            }
        }
    }

    /// Delete an auth token from storage
    pub async fn delete_token(&self, token: &str) -> Result<(), StorageError> {
        debug!("AuthService: Deleting token");
        
        let storage = self.storage.clone();
        
        // Delete token from storage
        match storage.as_ref().delete_auth_token(token).await {
            Ok(_) => Ok(()),
            Err(e) => {
                error!("Error deleting token: {}", e);
                Err(e)
            }
        }
    }

    /// Authenticate user with credentials
    pub async fn authenticate(&self, email: &str, password: &str) -> Result<(Account, String), StorageError> {
        debug!("AuthService: Authenticating user {}", email);
        
        let storage = self.storage.clone();
        
        // Get account by email
        let account = match storage.as_ref().get_account_by_email(email).await {
            Ok(Some(account)) => account,
            Ok(None) => {
                return Err(StorageError::NotFound(format!("Account not found for email: {}", email)));
            },
            Err(e) => {
                error!("Error getting account: {}", e);
                return Err(e);
            }
        };
        
        // Verify password
        let is_valid = self.verify_password(password, &account.password_hash, &account.salt);
        
        if !is_valid {
            return Err(StorageError::PermissionDenied("Invalid credentials".to_string()));
        }
        
        // Generate token
        let token = self.generate_token(&account.account_hash);
        
        Ok((account, token))
    }
} 