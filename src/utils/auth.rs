use std::env;
use tonic::Status;
use tracing::{debug, error};

/// Check if development or test mode is enabled (from FeatureFlags)
pub fn is_dev_or_test_mode_from_flags(flags: &crate::config::settings::FeatureFlags) -> bool {
    if flags.dev_mode || flags.test_mode {
        debug!("Dev/Test mode enabled: skipping device validation");
        true
    } else {
        false
    }
}

/// Backward-compatible helper: reads from env if flags not available
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
