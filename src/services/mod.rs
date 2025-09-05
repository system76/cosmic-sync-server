// Module declarations
pub mod auth_service;
pub mod device_service;
pub mod encryption_service;
pub mod file_service;
pub mod version_service;

// Public re-exports
pub use auth_service::AuthService;
pub use device_service::DeviceService;
pub use encryption_service::EncryptionService;
pub use file_service::FileService;
pub use version_service::{VersionService, VersionServiceImpl};

use async_trait::async_trait;
use tonic::{Request, Response, Status};
// sync 모듈을 사용
use crate::sync::*;
use crate::sync::{CheckFileExistsRequest, CheckFileExistsResponse};

/// 상태를 사용자 정의 오류로 확장
pub trait GrpcErrorExt {
    fn invalid_argument(message: &str) -> Status;
    fn unauthenticated(message: &str) -> Status;
    fn not_found(message: &str) -> Status;
    fn internal_error(message: &str) -> Status;
}

impl GrpcErrorExt for Status {
    fn invalid_argument(message: &str) -> Status {
        Status::invalid_argument(message)
    }

    fn unauthenticated(message: &str) -> Status {
        Status::unauthenticated(message)
    }

    fn not_found(message: &str) -> Status {
        Status::not_found(message)
    }

    fn internal_error(message: &str) -> Status {
        Status::internal(message)
    }
}

/// gRPC 요청 핸들러 트레이트
#[allow(unused_variables)]
#[async_trait]
pub trait Handler: Send + Sync + 'static {
    // OAuth 관련 메서드
    async fn handle_oauth_exchange(
        &self,
        request: Request<OAuthExchangeRequest>,
    ) -> Result<Response<OAuthExchangeResponse>, Status> {
        Err(Status::unimplemented(
            "handle_oauth_exchange method is not implemented",
        ))
    }

    // 장치 관련 메서드
    async fn handle_register_device(
        &self,
        request: Request<RegisterDeviceRequest>,
    ) -> Result<Response<RegisterDeviceResponse>, Status> {
        Err(Status::unimplemented(
            "handle_register_device method is not implemented",
        ))
    }

    async fn handle_list_devices(
        &self,
        request: Request<ListDevicesRequest>,
    ) -> Result<Response<ListDevicesResponse>, Status> {
        Err(Status::unimplemented(
            "handle_list_devices method is not implemented",
        ))
    }

    async fn handle_delete_device(
        &self,
        request: Request<DeleteDeviceRequest>,
    ) -> Result<Response<DeleteDeviceResponse>, Status> {
        Err(Status::unimplemented(
            "handle_delete_device method is not implemented",
        ))
    }

    async fn handle_update_device_info(
        &self,
        request: Request<UpdateDeviceInfoRequest>,
    ) -> Result<Response<UpdateDeviceInfoResponse>, Status> {
        Err(Status::unimplemented(
            "handle_update_device_info method is not implemented",
        ))
    }

    // 암호화 키 관련 메서드
    async fn handle_request_encryption_key(
        &self,
        request: Request<RequestEncryptionKeyRequest>,
    ) -> Result<Response<RequestEncryptionKeyResponse>, Status> {
        Err(Status::unimplemented(
            "handle_request_encryption_key method is not implemented",
        ))
    }

    // 파일 관련 메서드
    async fn handle_upload_file(
        &self,
        request: Request<UploadFileRequest>,
    ) -> Result<Response<UploadFileResponse>, Status> {
        Err(Status::unimplemented(
            "handle_upload_file method is not implemented",
        ))
    }

    async fn handle_download_file(
        &self,
        request: Request<DownloadFileRequest>,
    ) -> Result<Response<DownloadFileResponse>, Status> {
        Err(Status::unimplemented(
            "handle_download_file method is not implemented",
        ))
    }

    async fn handle_list_files(
        &self,
        request: Request<ListFilesRequest>,
    ) -> Result<Response<ListFilesResponse>, Status> {
        Err(Status::unimplemented(
            "handle_list_files method is not implemented",
        ))
    }

    async fn handle_delete_file(
        &self,
        request: Request<DeleteFileRequest>,
    ) -> Result<Response<DeleteFileResponse>, Status> {
        Err(Status::unimplemented(
            "handle_delete_file method is not implemented",
        ))
    }

    /// Handle file find by criteria request
    async fn handle_find_file_by_criteria(
        &self,
        request: Request<FindFileRequest>,
    ) -> Result<Response<FindFileResponse>, Status> {
        Err(Status::unimplemented(
            "handle_find_file_by_criteria method is not implemented",
        ))
    }

    /// Handle check file exists request
    async fn handle_check_file_exists(
        &self,
        request: Request<CheckFileExistsRequest>,
    ) -> Result<Response<CheckFileExistsResponse>, Status> {
        Err(Status::unimplemented(
            "handle_check_file_exists method is not implemented",
        ))
    }

    // WatcherPreset 관련 메서드
    async fn handle_register_watcher_preset(
        &self,
        request: Request<RegisterWatcherPresetRequest>,
    ) -> Result<Response<RegisterWatcherPresetResponse>, Status> {
        Err(Status::unimplemented(
            "handle_register_watcher_preset method is not implemented",
        ))
    }

    async fn handle_update_watcher_preset(
        &self,
        request: Request<UpdateWatcherPresetRequest>,
    ) -> Result<Response<UpdateWatcherPresetResponse>, Status> {
        Err(Status::unimplemented(
            "handle_update_watcher_preset method is not implemented",
        ))
    }

    async fn handle_get_watcher_preset(
        &self,
        request: Request<GetWatcherPresetRequest>,
    ) -> Result<Response<GetWatcherPresetResponse>, Status> {
        Err(Status::unimplemented(
            "handle_get_watcher_preset method is not implemented",
        ))
    }

    // WatcherGroup 관련 메서드
    async fn handle_register_watcher_group(
        &self,
        request: Request<RegisterWatcherGroupRequest>,
    ) -> Result<Response<RegisterWatcherGroupResponse>, Status> {
        Err(Status::unimplemented(
            "handle_register_watcher_group method is not implemented",
        ))
    }

    async fn handle_update_watcher_group(
        &self,
        request: Request<UpdateWatcherGroupRequest>,
    ) -> Result<Response<UpdateWatcherGroupResponse>, Status> {
        Err(Status::unimplemented(
            "handle_update_watcher_group method is not implemented",
        ))
    }

    async fn handle_delete_watcher_group(
        &self,
        request: Request<DeleteWatcherGroupRequest>,
    ) -> Result<Response<DeleteWatcherGroupResponse>, Status> {
        Err(Status::unimplemented(
            "handle_delete_watcher_group method is not implemented",
        ))
    }

    async fn handle_get_watcher_group(
        &self,
        request: Request<GetWatcherGroupRequest>,
    ) -> Result<Response<GetWatcherGroupResponse>, Status> {
        Err(Status::unimplemented(
            "handle_get_watcher_group method is not implemented",
        ))
    }

    async fn handle_get_watcher_groups(
        &self,
        request: Request<GetWatcherGroupsRequest>,
    ) -> Result<Response<GetWatcherGroupsResponse>, Status> {
        Err(Status::unimplemented(
            "handle_get_watcher_groups method is not implemented",
        ))
    }

    // 헬스 체크 메서드
    async fn handle_health_check(
        &self,
        request: Request<HealthCheckRequest>,
    ) -> Result<Response<HealthCheckResponse>, Status> {
        Err(Status::unimplemented(
            "handle_health_check method is not implemented",
        ))
    }

    async fn handle_validate_token(
        &self,
        request: Request<ValidateTokenRequest>,
    ) -> Result<Response<ValidateTokenResponse>, Status> {
        Err(Status::unimplemented(
            "handle_validate_token method is not implemented",
        ))
    }

    async fn handle_check_auth_status(
        &self,
        request: Request<CheckAuthStatusRequest>,
    ) -> Result<Response<CheckAuthStatusResponse>, Status> {
        Err(Status::unimplemented(
            "handle_check_auth_status method is not implemented",
        ))
    }

    /// 통합 설정 동기화 핸들러
    async fn handle_sync_configuration(
        &self,
        request: Request<SyncConfigurationRequest>,
    ) -> Result<Response<SyncConfigurationResponse>, Status> {
        Err(Status::unimplemented(
            "handle_sync_configuration method is not implemented",
        ))
    }

    /// 계정 정보 조회 핸들러
    async fn handle_get_account_info(
        &self,
        request: Request<GetAccountInfoRequest>,
    ) -> Result<Response<GetAccountInfoResponse>, Status> {
        Err(Status::unimplemented(
            "handle_get_account_info method is not implemented",
        ))
    }

    /// Login handler
    async fn handle_login(
        &self,
        request: Request<LoginRequest>,
    ) -> Result<Response<LoginResponse>, Status> {
        Err(Status::unimplemented(
            "handle_login method is not implemented",
        ))
    }

    async fn handle_verify_login(
        &self,
        request: Request<VerifyLoginRequest>,
    ) -> Result<Response<VerifyLoginResponse>, Status> {
        Err(Status::unimplemented(
            "handle_verify_login method is not implemented",
        ))
    }

    // 스트리밍 구독 관련 메서드
    async fn handle_subscribe_to_auth_updates(
        &self,
        request: Request<SubscribeRequest>,
    ) -> Result<Response<Self::SubscribeToAuthUpdatesStream>, Status> {
        Err(Status::unimplemented(
            "handle_subscribe_to_auth_updates method is not implemented",
        ))
    }

    async fn handle_subscribe_to_device_updates(
        &self,
        request: Request<SubscribeRequest>,
    ) -> Result<Response<Self::SubscribeToDeviceUpdatesStream>, Status> {
        Err(Status::unimplemented(
            "handle_subscribe_to_device_updates method is not implemented",
        ))
    }

    async fn handle_subscribe_to_encryption_key_updates(
        &self,
        request: Request<SubscribeRequest>,
    ) -> Result<Response<Self::SubscribeToEncryptionKeyUpdatesStream>, Status> {
        Err(Status::unimplemented(
            "handle_subscribe_to_encryption_key_updates method is not implemented",
        ))
    }

    async fn handle_subscribe_to_file_updates(
        &self,
        request: Request<SubscribeRequest>,
    ) -> Result<Response<Self::SubscribeToFileUpdatesStream>, Status> {
        Err(Status::unimplemented(
            "handle_subscribe_to_file_updates method is not implemented",
        ))
    }

    async fn handle_subscribe_to_watcher_preset_updates(
        &self,
        request: Request<SubscribeRequest>,
    ) -> Result<Response<Self::SubscribeToWatcherPresetUpdatesStream>, Status> {
        Err(Status::unimplemented(
            "handle_subscribe_to_watcher_preset_updates method is not implemented",
        ))
    }

    async fn handle_subscribe_to_watcher_group_updates(
        &self,
        request: Request<SubscribeRequest>,
    ) -> Result<Response<Self::SubscribeToWatcherGroupUpdatesStream>, Status> {
        Err(Status::unimplemented(
            "handle_subscribe_to_watcher_group_updates method is not implemented",
        ))
    }

    async fn handle_subscribe_to_version_updates(
        &self,
        request: Request<SubscribeRequest>,
    ) -> Result<Response<Self::SubscribeToVersionUpdatesStream>, Status> {
        Err(Status::unimplemented(
            "handle_subscribe_to_version_updates method is not implemented",
        ))
    }

    // Version management methods
    async fn handle_get_file_history(
        &self,
        request: Request<GetFileHistoryRequest>,
    ) -> Result<Response<GetFileHistoryResponse>, Status> {
        Err(Status::unimplemented(
            "handle_get_file_history method is not implemented",
        ))
    }

    async fn handle_restore_file_version(
        &self,
        request: Request<RestoreFileVersionRequest>,
    ) -> Result<Response<RestoreFileVersionResponse>, Status> {
        Err(Status::unimplemented(
            "handle_restore_file_version method is not implemented",
        ))
    }

    async fn handle_broadcast_file_restore(
        &self,
        request: Request<BroadcastFileRestoreRequest>,
    ) -> Result<Response<BroadcastFileRestoreResponse>, Status> {
        Err(Status::unimplemented(
            "handle_broadcast_file_restore method is not implemented",
        ))
    }

    // 스트리밍 반환 타입 정의
    type SubscribeToAuthUpdatesStream;
    type SubscribeToDeviceUpdatesStream;
    type SubscribeToEncryptionKeyUpdatesStream;
    type SubscribeToFileUpdatesStream;
    type SubscribeToWatcherPresetUpdatesStream;
    type SubscribeToWatcherGroupUpdatesStream;
    type SubscribeToVersionUpdatesStream;
}
