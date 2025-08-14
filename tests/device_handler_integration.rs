// Integration tests for DeviceHandler moved under tests/
// These tests use MemoryStorage to avoid MySQL side effects

use std::sync::Arc;
use tonic::Request;

use cosmic_sync_server::handlers::device_handler::DeviceHandler;
use cosmic_sync_server::server::app_state::AppState;
use cosmic_sync_server::storage::memory::MemoryStorage;
use cosmic_sync_server::sync::RegisterDeviceRequest;

#[tokio::test]
async fn test_register_device_empty_account_hash() {
    let storage = Arc::new(MemoryStorage::new());
    let server_cfg = cosmic_sync_server::config::settings::ServerConfig::default();
    let app_state = AppState::new_with_storage_and_server_config(storage, &server_cfg).await.unwrap();
    let handler = DeviceHandler::new(app_state.into());

    let request = Request::new(RegisterDeviceRequest {
        account_hash: String::new(),
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
    let storage = Arc::new(MemoryStorage::new());
    let server_cfg = cosmic_sync_server::config::settings::ServerConfig::default();
    let app_state = AppState::new_with_storage_and_server_config(storage, &server_cfg).await.unwrap();
    let handler = DeviceHandler::new(app_state.into());

    let request = Request::new(RegisterDeviceRequest {
        account_hash: "test_account".to_string(),
        device_hash: String::new(),
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
    let storage = Arc::new(MemoryStorage::new());
    let server_cfg = cosmic_sync_server::config::settings::ServerConfig::default();
    let app_state = AppState::new_with_storage_and_server_config(storage, &server_cfg).await.unwrap();
    let handler = DeviceHandler::new(app_state.into());

    let long_device_hash = "a".repeat(129);

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
    let storage = Arc::new(MemoryStorage::new());
    let server_cfg = cosmic_sync_server::config::settings::ServerConfig::default();
    let app_state = AppState::new_with_storage_and_server_config(storage, &server_cfg).await.unwrap();
    let handler = DeviceHandler::new(app_state.into());

    let request = Request::new(RegisterDeviceRequest {
        account_hash: "test_account".to_string(),
        device_hash: "test_device".to_string(),
        is_active: true,
        os_version: "Linux 5.4".to_string(),
        app_version: "1.0.0".to_string(),
        auth_token: String::new(),
    });

    let response = handler.register_device(request).await.unwrap();
    let inner = response.into_inner();

    assert!(!inner.success);
    assert_eq!(inner.return_message, "Authentication token required");
}
