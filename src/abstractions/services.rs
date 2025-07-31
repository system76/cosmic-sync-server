// Service abstractions for enhanced modularity and testability
// Provides high-level service interfaces that can be implemented
// by different backends and easily mocked for testing

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use serde::{Serialize, Deserialize};
use crate::{
    error::{Result, SyncError},
    abstractions::{PluginConfig, HealthStatus, Observable, Configurable},
};

/// Base service trait that all services should implement
#[async_trait]
pub trait Service: Send + Sync + Observable + Configurable {
    /// Get service name
    fn name(&self) -> &'static str;
    
    /// Get service version
    fn version(&self) -> &'static str;
    
    /// Get service dependencies
    fn dependencies(&self) -> Vec<&'static str> {
        Vec::new()
    }
    
    /// Initialize the service
    async fn initialize(&mut self) -> Result<()>;
    
    /// Start the service
    async fn start(&mut self) -> Result<()>;
    
    /// Stop the service
    async fn stop(&mut self) -> Result<()>;
    
    /// Get service status
    async fn status(&self) -> Result<ServiceStatus>;
}

/// Service status information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceStatus {
    pub name: String,
    pub health: HealthStatus,
    pub uptime_seconds: u64,
    pub last_error: Option<String>,
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Account management service interface
#[async_trait]
pub trait AccountService: Service {
    /// Create a new account
    async fn create_account(&self, request: CreateAccountRequest) -> Result<Account>;
    
    /// Get account by ID
    async fn get_account(&self, account_id: &str) -> Result<Option<Account>>;
    
    /// Get account by email
    async fn get_account_by_email(&self, email: &str) -> Result<Option<Account>>;
    
    /// Update account information
    async fn update_account(&self, account: Account) -> Result<Account>;
    
    /// Delete account
    async fn delete_account(&self, account_id: &str) -> Result<()>;
    
    /// List accounts with pagination
    async fn list_accounts(&self, request: ListAccountsRequest) -> Result<ListAccountsResponse>;
    
    /// Verify account credentials
    async fn verify_credentials(&self, email: &str, password: &str) -> Result<Account>;
}

/// Device management service interface
#[async_trait]
pub trait DeviceService: Service {
    /// Register a new device
    async fn register_device(&self, request: RegisterDeviceRequest) -> Result<Device>;
    
    /// Get device by ID
    async fn get_device(&self, device_id: &str) -> Result<Option<Device>>;
    
    /// List devices for an account
    async fn list_devices(&self, account_id: &str) -> Result<Vec<Device>>;
    
    /// Update device information
    async fn update_device(&self, device: Device) -> Result<Device>;
    
    /// Delete device
    async fn delete_device(&self, device_id: &str) -> Result<()>;
    
    /// Update device last seen timestamp
    async fn update_device_activity(&self, device_id: &str) -> Result<()>;
    
    /// Get device statistics
    async fn get_device_stats(&self, device_id: &str) -> Result<DeviceStats>;
}

/// File management service interface
#[async_trait]
pub trait FileService: Service {
    /// Upload a file
    async fn upload_file(&self, request: UploadFileRequest) -> Result<FileMetadata>;
    
    /// Download a file
    async fn download_file(&self, request: DownloadFileRequest) -> Result<FileContent>;
    
    /// Get file metadata
    async fn get_file_metadata(&self, file_id: &str) -> Result<Option<FileMetadata>>;
    
    /// List files with filters
    async fn list_files(&self, request: ListFilesRequest) -> Result<ListFilesResponse>;
    
    /// Delete a file
    async fn delete_file(&self, file_id: &str) -> Result<()>;
    
    /// Search files
    async fn search_files(&self, request: SearchFilesRequest) -> Result<SearchFilesResponse>;
    
    /// Get file sharing information
    async fn get_file_sharing(&self, file_id: &str) -> Result<Option<FileSharing>>;
    
    /// Update file sharing settings
    async fn update_file_sharing(&self, request: UpdateFileSharingRequest) -> Result<FileSharing>;
}

/// Synchronization service interface
#[async_trait]
pub trait SyncService: Service {
    /// Start synchronization for a device
    async fn start_sync(&self, request: StartSyncRequest) -> Result<SyncSession>;
    
    /// Stop synchronization for a device
    async fn stop_sync(&self, session_id: &str) -> Result<()>;
    
    /// Get synchronization status
    async fn get_sync_status(&self, session_id: &str) -> Result<Option<SyncStatus>>;
    
    /// Resolve sync conflicts
    async fn resolve_conflicts(&self, request: ResolveConflictsRequest) -> Result<ConflictResolution>;
    
    /// Get sync history
    async fn get_sync_history(&self, request: GetSyncHistoryRequest) -> Result<SyncHistoryResponse>;
    
    /// Pause synchronization
    async fn pause_sync(&self, session_id: &str) -> Result<()>;
    
    /// Resume synchronization
    async fn resume_sync(&self, session_id: &str) -> Result<()>;
}

/// Notification service interface
#[async_trait]
pub trait NotificationService: Service {
    /// Send notification to user
    async fn send_notification(&self, request: SendNotificationRequest) -> Result<NotificationResult>;
    
    /// Send notification to device
    async fn send_device_notification(&self, request: SendDeviceNotificationRequest) -> Result<NotificationResult>;
    
    /// Get notification preferences
    async fn get_preferences(&self, user_id: &str) -> Result<NotificationPreferences>;
    
    /// Update notification preferences
    async fn update_preferences(&self, request: UpdateNotificationPreferencesRequest) -> Result<NotificationPreferences>;
    
    /// Get notification history
    async fn get_notification_history(&self, request: GetNotificationHistoryRequest) -> Result<NotificationHistoryResponse>;
    
    /// Mark notifications as read
    async fn mark_as_read(&self, notification_ids: Vec<String>) -> Result<()>;
}

/// Authentication service interface
#[async_trait]
pub trait AuthenticationService: Service {
    /// Authenticate user with credentials
    async fn authenticate(&self, request: AuthenticationRequest) -> Result<AuthenticationResult>;
    
    /// Refresh authentication token
    async fn refresh_token(&self, refresh_token: &str) -> Result<AuthenticationResult>;
    
    /// Revoke authentication token
    async fn revoke_token(&self, token: &str) -> Result<()>;
    
    /// Validate authentication token
    async fn validate_token(&self, token: &str) -> Result<TokenValidation>;
    
    /// Get user permissions
    async fn get_permissions(&self, user_id: &str) -> Result<Vec<Permission>>;
    
    /// Generate API key
    async fn generate_api_key(&self, request: GenerateApiKeyRequest) -> Result<ApiKey>;
    
    /// Revoke API key
    async fn revoke_api_key(&self, api_key_id: &str) -> Result<()>;
}

/// Encryption service interface
#[async_trait]
pub trait EncryptionService: Service {
    /// Encrypt data
    async fn encrypt(&self, request: EncryptRequest) -> Result<EncryptResult>;
    
    /// Decrypt data
    async fn decrypt(&self, request: DecryptRequest) -> Result<DecryptResult>;
    
    /// Generate encryption key
    async fn generate_key(&self, key_type: KeyType) -> Result<EncryptionKey>;
    
    /// Get encryption key
    async fn get_key(&self, key_id: &str) -> Result<Option<EncryptionKey>>;
    
    /// Rotate encryption key
    async fn rotate_key(&self, key_id: &str) -> Result<EncryptionKey>;
    
    /// Delete encryption key
    async fn delete_key(&self, key_id: &str) -> Result<()>;
}

/// Storage service interface
#[async_trait]
pub trait StorageService: Service {
    /// Store data
    async fn store(&self, request: StoreRequest) -> Result<StoreResult>;
    
    /// Retrieve data
    async fn retrieve(&self, request: RetrieveRequest) -> Result<Option<RetrieveResult>>;
    
    /// Delete data
    async fn delete(&self, key: &str) -> Result<()>;
    
    /// List stored items
    async fn list(&self, request: ListStorageRequest) -> Result<ListStorageResponse>;
    
    /// Get storage statistics
    async fn get_stats(&self) -> Result<StorageStats>;
    
    /// Backup data
    async fn backup(&self, request: BackupRequest) -> Result<BackupResult>;
    
    /// Restore data
    async fn restore(&self, request: RestoreRequest) -> Result<RestoreResult>;
}

// Request/Response DTOs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateAccountRequest {
    pub email: String,
    pub password: String,
    pub name: Option<String>,
    pub metadata: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub email: String,
    pub name: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub is_active: bool,
    pub metadata: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListAccountsRequest {
    pub limit: Option<u32>,
    pub offset: Option<u64>,
    pub filter: Option<AccountFilter>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountFilter {
    pub email_pattern: Option<String>,
    pub is_active: Option<bool>,
    pub created_after: Option<i64>,
    pub created_before: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListAccountsResponse {
    pub accounts: Vec<Account>,
    pub total_count: u64,
    pub has_more: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterDeviceRequest {
    pub account_id: String,
    pub device_name: String,
    pub device_type: String,
    pub device_info: DeviceInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Device {
    pub id: String,
    pub account_id: String,
    pub device_name: String,
    pub device_type: String,
    pub device_info: DeviceInfo,
    pub created_at: i64,
    pub last_seen: i64,
    pub is_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub platform: String,
    pub version: String,
    pub architecture: String,
    pub user_agent: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceStats {
    pub files_synced: u64,
    pub bytes_transferred: u64,
    pub last_sync: Option<i64>,
    pub sync_errors: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadFileRequest {
    pub account_id: String,
    pub device_id: String,
    pub file_path: String,
    pub file_name: String,
    pub file_size: u64,
    pub file_hash: String,
    pub content_type: Option<String>,
    pub metadata: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMetadata {
    pub id: String,
    pub account_id: String,
    pub file_path: String,
    pub file_name: String,
    pub file_size: u64,
    pub file_hash: String,
    pub content_type: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub metadata: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone)]
pub struct FileContent {
    pub metadata: FileMetadata,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadFileRequest {
    pub file_id: String,
    pub account_id: String,
    pub device_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListFilesRequest {
    pub account_id: String,
    pub device_id: Option<String>,
    pub path_prefix: Option<String>,
    pub limit: Option<u32>,
    pub offset: Option<u64>,
    pub filter: Option<FileFilter>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileFilter {
    pub content_type: Option<String>,
    pub min_size: Option<u64>,
    pub max_size: Option<u64>,
    pub created_after: Option<i64>,
    pub created_before: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListFilesResponse {
    pub files: Vec<FileMetadata>,
    pub total_count: u64,
    pub has_more: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchFilesRequest {
    pub account_id: String,
    pub query: String,
    pub filters: Option<FileFilter>,
    pub limit: Option<u32>,
    pub offset: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchFilesResponse {
    pub files: Vec<FileMetadata>,
    pub total_count: u64,
    pub has_more: bool,
    pub query_time_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSharing {
    pub file_id: String,
    pub is_public: bool,
    pub shared_with: Vec<String>,
    pub permissions: Vec<Permission>,
    pub expires_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateFileSharingRequest {
    pub file_id: String,
    pub is_public: Option<bool>,
    pub shared_with: Option<Vec<String>>,
    pub permissions: Option<Vec<Permission>>,
    pub expires_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Permission {
    pub resource: String,
    pub action: String,
    pub granted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartSyncRequest {
    pub account_id: String,
    pub device_id: String,
    pub sync_options: SyncOptions,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncOptions {
    pub bidirectional: bool,
    pub real_time: bool,
    pub conflict_resolution: ConflictResolutionStrategy,
    pub file_patterns: Vec<String>,
    pub exclude_patterns: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConflictResolutionStrategy {
    KeepLocal,
    KeepRemote,
    KeepBoth,
    AskUser,
    Automatic,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncSession {
    pub id: String,
    pub account_id: String,
    pub device_id: String,
    pub status: SyncSessionStatus,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SyncSessionStatus {
    Starting,
    Active,
    Paused,
    Stopping,
    Stopped,
    Error(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncStatus {
    pub session_id: String,
    pub status: SyncSessionStatus,
    pub progress: SyncProgress,
    pub last_update: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncProgress {
    pub files_processed: u64,
    pub files_total: u64,
    pub bytes_transferred: u64,
    pub bytes_total: u64,
    pub current_file: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolveConflictsRequest {
    pub session_id: String,
    pub conflicts: Vec<ConflictResolution>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictResolution {
    pub conflict_id: String,
    pub resolution: ConflictResolutionStrategy,
    pub keep_file_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetSyncHistoryRequest {
    pub account_id: String,
    pub device_id: Option<String>,
    pub limit: Option<u32>,
    pub offset: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncHistoryResponse {
    pub sessions: Vec<SyncSession>,
    pub total_count: u64,
    pub has_more: bool,
}

// Additional service-related types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthenticationRequest {
    pub email: String,
    pub password: String,
    pub device_info: Option<DeviceInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthenticationResult {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: u64,
    pub token_type: String,
    pub user: Account,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenValidation {
    pub is_valid: bool,
    pub user_id: Option<String>,
    pub expires_at: Option<i64>,
    pub permissions: Vec<Permission>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerateApiKeyRequest {
    pub user_id: String,
    pub name: String,
    pub permissions: Vec<Permission>,
    pub expires_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKey {
    pub id: String,
    pub key: String,
    pub name: String,
    pub user_id: String,
    pub permissions: Vec<Permission>,
    pub created_at: i64,
    pub expires_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptRequest {
    pub data: Vec<u8>,
    pub key_id: String,
    pub algorithm: EncryptionAlgorithm,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptResult {
    pub encrypted_data: Vec<u8>,
    pub iv: Vec<u8>,
    pub algorithm: EncryptionAlgorithm,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecryptRequest {
    pub encrypted_data: Vec<u8>,
    pub key_id: String,
    pub iv: Vec<u8>,
    pub algorithm: EncryptionAlgorithm,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecryptResult {
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum KeyType {
    Symmetric,
    Asymmetric,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EncryptionAlgorithm {
    Aes256Gcm,
    ChaCha20Poly1305,
    Rsa2048,
    Ed25519,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptionKey {
    pub id: String,
    pub key_type: KeyType,
    pub algorithm: EncryptionAlgorithm,
    pub created_at: i64,
    pub expires_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreRequest {
    pub key: String,
    pub data: Vec<u8>,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreResult {
    pub key: String,
    pub size: u64,
    pub checksum: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrieveRequest {
    pub key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrieveResult {
    pub key: String,
    pub data: Vec<u8>,
    pub metadata: HashMap<String, String>,
    pub size: u64,
    pub checksum: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListStorageRequest {
    pub prefix: Option<String>,
    pub limit: Option<u32>,
    pub offset: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListStorageResponse {
    pub items: Vec<StorageItem>,
    pub total_count: u64,
    pub has_more: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageItem {
    pub key: String,
    pub size: u64,
    pub checksum: String,
    pub created_at: i64,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageStats {
    pub total_items: u64,
    pub total_size: u64,
    pub available_space: Option<u64>,
    pub used_space_percentage: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupRequest {
    pub backup_name: String,
    pub include_patterns: Vec<String>,
    pub exclude_patterns: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupResult {
    pub backup_id: String,
    pub backup_name: String,
    pub size: u64,
    pub created_at: i64,
    pub checksum: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreRequest {
    pub backup_id: String,
    pub restore_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreResult {
    pub restored_items: u64,
    pub restored_size: u64,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendNotificationRequest {
    pub user_id: String,
    pub notification_type: String,
    pub title: String,
    pub message: String,
    pub data: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendDeviceNotificationRequest {
    pub device_id: String,
    pub notification_type: String,
    pub title: String,
    pub message: String,
    pub data: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationResult {
    pub id: String,
    pub sent_at: i64,
    pub delivery_status: DeliveryStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DeliveryStatus {
    Pending,
    Sent,
    Delivered,
    Failed(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationPreferences {
    pub user_id: String,
    pub email_enabled: bool,
    pub push_enabled: bool,
    pub sms_enabled: bool,
    pub notification_types: HashMap<String, bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateNotificationPreferencesRequest {
    pub user_id: String,
    pub email_enabled: Option<bool>,
    pub push_enabled: Option<bool>,
    pub sms_enabled: Option<bool>,
    pub notification_types: Option<HashMap<String, bool>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetNotificationHistoryRequest {
    pub user_id: String,
    pub limit: Option<u32>,
    pub offset: Option<u64>,
    pub notification_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationHistoryResponse {
    pub notifications: Vec<Notification>,
    pub total_count: u64,
    pub has_more: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    pub id: String,
    pub user_id: String,
    pub notification_type: String,
    pub title: String,
    pub message: String,
    pub data: HashMap<String, serde_json::Value>,
    pub sent_at: i64,
    pub read_at: Option<i64>,
    pub delivery_status: DeliveryStatus,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_account_serialization() {
        let account = Account {
            id: "test-123".to_string(),
            email: "test@example.com".to_string(),
            name: Some("Test User".to_string()),
            created_at: 1640995200,
            updated_at: 1640995200,
            is_active: true,
            metadata: HashMap::new(),
        };
        
        let json = serde_json::to_string(&account).unwrap();
        let deserialized: Account = serde_json::from_str(&json).unwrap();
        
        assert_eq!(account.id, deserialized.id);
        assert_eq!(account.email, deserialized.email);
        assert_eq!(account.name, deserialized.name);
    }

    #[test]
    fn test_device_info() {
        let device_info = DeviceInfo {
            platform: "Linux".to_string(),
            version: "5.4.0".to_string(),
            architecture: "x86_64".to_string(),
            user_agent: Some("COSMIC Sync/1.0".to_string()),
        };
        
        assert_eq!(device_info.platform, "Linux");
        assert_eq!(device_info.architecture, "x86_64");
        assert!(device_info.user_agent.is_some());
    }

    #[test]
    fn test_sync_options() {
        let options = SyncOptions {
            bidirectional: true,
            real_time: false,
            conflict_resolution: ConflictResolutionStrategy::KeepLocal,
            file_patterns: vec!["*.txt".to_string(), "*.md".to_string()],
            exclude_patterns: vec!["*.tmp".to_string()],
        };
        
        assert!(options.bidirectional);
        assert!(!options.real_time);
        assert_eq!(options.file_patterns.len(), 2);
        assert_eq!(options.exclude_patterns.len(), 1);
    }
}
