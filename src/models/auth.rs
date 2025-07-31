use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Authentication token
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthToken {
    /// Token unique identifier
    pub token_id: String,
    
    /// Associated account hash
    pub account_hash: String,
    
    /// Access token value
    pub access_token: String,
    
    /// Refresh token value (needed for OAuth flow)
    pub refresh_token: Option<String>,
    
    /// When the token was created
    pub created_at: DateTime<Utc>,
    
    /// When the token expires
    pub expires_at: DateTime<Utc>,
    
    /// Is the token currently valid
    pub is_valid: bool,
    
    /// Token type (Bearer, MAC, etc.)
    pub token_type: String,
    
    /// Scope information (optional)
    pub scope: Option<String>,
}

/// Authentication result from token verification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthResult {
    /// Whether the authentication is valid
    pub valid: bool,
    
    /// Account hash associated with the token
    pub account_hash: String,
    
    /// Error message if authentication failed
    pub error_message: Option<String>,
    
    /// Token expiration time
    pub expires_at: Option<DateTime<Utc>>,
} 