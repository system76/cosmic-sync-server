use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// user account info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    /// account hash (email based)
    pub account_hash: String,
    /// unique ID
    pub id: String,
    /// email address
    pub email: String,
    /// user name
    pub name: String,
    /// user type
    pub user_type: String,
    /// password hash
    pub password_hash: String,
    /// password salt
    pub salt: String,
    /// active status
    pub is_active: bool,
    /// account creation time
    pub created_at: DateTime<Utc>,
    /// last login time
    pub last_login: DateTime<Utc>,
    /// update time
    pub updated_at: DateTime<Utc>,
    /// existing user_id field (for compatibility)
    pub user_id: String,
}

/// authentication token
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimpleAuthToken {
    /// authentication token value
    pub token: String,
    /// connected account hash
    pub account_hash: String,
    /// token creation time
    pub created_at: DateTime<Utc>,
    /// token expiration time
    pub expires_at: DateTime<Utc>,
}
