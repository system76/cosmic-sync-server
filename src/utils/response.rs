use crate::sync::{
    UploadFileResponse, DownloadFileResponse, DeleteFileResponse,
    RegisterDeviceResponse,
};

/// Create error response for file upload
pub fn file_upload_error(message: impl Into<String>) -> UploadFileResponse {
    UploadFileResponse {
        success: false,
        file_id: 0,
        new_revision: 0,
        return_message: message.into(),
    }
}

/// Create success response for file upload
pub fn file_upload_success(file_id: u64, new_revision: i64) -> UploadFileResponse {
    UploadFileResponse {
        success: true,
        file_id,
        new_revision,
        return_message: "OK".to_string(),
    }
}

/// Create error response for file download
pub fn file_download_error(message: impl Into<String>) -> DownloadFileResponse {
    DownloadFileResponse {
        success: false,
        file_data: Vec::new(),
        file_hash: String::new(),
        is_encrypted: false,
        return_message: message.into(),
        filename: String::new(),
        file_path: String::new(),
        updated_time: None,
    }
}

/// Create error response for file deletion
pub fn file_delete_error(message: impl Into<String>) -> DeleteFileResponse {
    DeleteFileResponse {
        success: false,
        return_message: message.into(),
    }
}

/// Create success response for file deletion
pub fn file_delete_success(message: impl Into<String>) -> DeleteFileResponse {
    DeleteFileResponse {
        success: true,
        return_message: message.into(),
    }
}

/// Create error response for device registration
pub fn device_register_error(message: impl Into<String>) -> RegisterDeviceResponse {
    RegisterDeviceResponse {
        success: false,
        device_hash: String::new(),
        return_message: message.into(),
    }
}

/// Create success response for device registration
pub fn device_register_success(device_hash: String, message: impl Into<String>) -> RegisterDeviceResponse {
    RegisterDeviceResponse {
        success: true,
        device_hash,
        return_message: message.into(),
    }
} 