use sha2::{Sha256, Digest};
use rand::{random, RngCore, rngs::OsRng};
use rand::Rng;
use chrono::Utc;
use rand::thread_rng;
use std::time::{SystemTime, UNIX_EPOCH};
use hex;

/// generate auth token
pub fn generate_auth_token() -> String {
    let mut buffer = [0u8; 32];
    OsRng.fill_bytes(&mut buffer);
    
    let now = Utc::now().timestamp().to_string();
    let input = format!("{}:{}", hex::encode(buffer), now);
    
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let result = hasher.finalize();
    
    hex::encode(result)
}

/// generate state token (CSRF prevention)
pub fn generate_state_token() -> String {
    let mut buffer = [0u8; 16];
    OsRng.fill_bytes(&mut buffer);
    
    hex::encode(buffer)
}

/// generate temporary token
pub fn generate_session_token() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let random = thread_rng().gen::<u64>();
    format!("token_{}_{}", now, random)
}

/// extract account hash from device hash
pub fn extract_account_hash(device_hash: &str) -> String {
    device_hash.split('_').next().unwrap_or(device_hash).to_string()
}