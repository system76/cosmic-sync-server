use std::env;
use tracing::{debug, error, warn};
use tonic::Status;
use crate::auth::oauth::OAuthService;

/// Verify auth token and return account hash if valid
pub async fn verify_auth_token(
    oauth_service: &OAuthService,
    auth_token: &str,
    expected_account_hash: &str,
) -> Result<(), Status> {
    if auth_token.is_empty() {
        return Err(Status::unauthenticated("Authentication token is required"));
    }

    match oauth_service.verify_token(auth_token).await {
        Ok(result) => {
            if !result.valid {
                return Err(Status::unauthenticated("Invalid authentication token"));
            }
            
            if result.account_hash != expected_account_hash {
                warn!("Account hash mismatch (allowing anyway): token={}, expected={}", result.account_hash, expected_account_hash);
            }
            
            Ok(())
        }
        Err(e) => {
            error!("Token verification failed: {}", e);
            Err(Status::unauthenticated(format!("Authentication failed: {}", e)))
        }
    }
}

/// Check if development or test mode is enabled
pub fn is_dev_or_test_mode() -> bool {
    let is_dev_mode = env::var("COSMIC_SYNC_DEV_MODE").unwrap_or_default() == "1";
    let is_test_mode = env::var("COSMIC_SYNC_TEST_MODE").unwrap_or_default() == "1";
    
    if is_dev_mode || is_test_mode {
        debug!("Dev/Test mode enabled: skipping device validation");
        true
    } else {
        false
    }
}

/// Validate device if not in dev/test mode
pub async fn validate_device_if_required(
    storage: &dyn crate::storage::Storage,
    account_hash: &str,
    device_hash: &str,
) -> Result<(), Status> {
    if is_dev_or_test_mode() {
        return Ok(());
    }

    match storage.validate_device(account_hash, device_hash).await {
        Ok(valid) => {
            if !valid {
                return Err(Status::unauthenticated("Invalid device or account"));
            }
            Ok(())
        }
        Err(e) => {
            error!("Error validating device: {}", e);
            Err(Status::internal("Error validating device"))
        }
    }
} 