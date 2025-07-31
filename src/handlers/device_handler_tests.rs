#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::app_state::AppState;
    use crate::storage::memory::MemoryStorage;
    use std::sync::Arc;
    use tokio;
    use tonic::Request;

    async fn create_test_app_state() -> Arc<AppState> {
        let storage = Arc::new(MemoryStorage::new());
        AppState::new_with_storage(storage).await.unwrap()
    }

    #[tokio::test]
    async fn test_register_device_empty_account_hash() {
        let app_state = create_test_app_state().await;
        let handler = DeviceHandler::new(app_state);

        let request = Request::new(RegisterDeviceRequest {
            account_hash: String::new(), // Empty account hash
            device_hash: "test_device".to_string(),
            is_active: true,
            os_version: "Linux 5.4".to_string(),
            app_version: "1.0.0".to_string(),
            auth_token: "test_token".to_string(),
        });

        let response = handler.register_device(request).await.unwrap();
        let inner = response.into_inner();
        
        assert!(!inner.success);
        assert_eq!(inner.return_message, "Account hash cannot be empty");
    }

    #[tokio::test]
    async fn test_register_device_empty_device_hash() {
        let app_state = create_test_app_state().await;
        let handler = DeviceHandler::new(app_state);

        let request = Request::new(RegisterDeviceRequest {
            account_hash: "test_account".to_string(),
            device_hash: String::new(), // Empty device hash
            is_active: true,
            os_version: "Linux 5.4".to_string(),
            app_version: "1.0.0".to_string(),
            auth_token: "test_token".to_string(),
        });

        let response = handler.register_device(request).await.unwrap();
        let inner = response.into_inner();
        
        assert!(!inner.success);
        assert_eq!(inner.return_message, "Device hash cannot be empty");
    }

    #[tokio::test]
    async fn test_register_device_too_long_device_hash() {
        let app_state = create_test_app_state().await;
        let handler = DeviceHandler::new(app_state);

        let long_device_hash = "a".repeat(129); // Exceeds 128 character limit

        let request = Request::new(RegisterDeviceRequest {
            account_hash: "test_account".to_string(),
            device_hash: long_device_hash,
            is_active: true,
            os_version: "Linux 5.4".to_string(),
            app_version: "1.0.0".to_string(),
            auth_token: "test_token".to_string(),
        });

        let response = handler.register_device(request).await.unwrap();
        let inner = response.into_inner();
        
        assert!(!inner.success);
        assert_eq!(inner.return_message, "Device hash too long (max 128 characters)");
    }

    #[tokio::test]
    async fn test_register_device_empty_auth_token() {
        let app_state = create_test_app_state().await;
        let handler = DeviceHandler::new(app_state);

        let request = Request::new(RegisterDeviceRequest {
            account_hash: "test_account".to_string(),
            device_hash: "test_device".to_string(),
            is_active: true,
            os_version: "Linux 5.4".to_string(),
            app_version: "1.0.0".to_string(),
            auth_token: String::new(), // Empty auth token
        });

        let response = handler.register_device(request).await.unwrap();
        let inner = response.into_inner();
        
        assert!(!inner.success);
        assert_eq!(inner.return_message, "Authentication token required");
    }
}