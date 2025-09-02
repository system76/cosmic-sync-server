use tonic::{Response, Status};
use tracing::debug;

use crate::sync::{CheckFileExistsRequest, CheckFileExistsResponse};
use super::super::file_handler::FileHandler;

pub async fn handle_check_file_exists(handler: &FileHandler, req: CheckFileExistsRequest) -> Result<Response<CheckFileExistsResponse>, Status> {
    debug!("CheckFileExists request: account={}, file_id={}", req.account_hash, req.file_id);

    match handler.app_state.oauth.verify_token(&req.auth_token).await {
        Ok(v) if v.valid => {}
        _ => {
            return Ok(Response::new(CheckFileExistsResponse {
                success: false,
                exists: false,
                is_deleted: false,
                return_message: "Authentication failed".to_string(),
            }));
        }
    }

    match handler.app_state.file.check_file_exists(req.file_id).await {
        Ok((exists, is_deleted)) => {
            debug!("File existence check: file_id={}, exists={}, is_deleted={}", req.file_id, exists, is_deleted);
            Ok(Response::new(CheckFileExistsResponse {
                success: true,
                exists,
                is_deleted,
                return_message: if exists {
                    if is_deleted { "File exists but is deleted".to_string() } else { "File exists and is active".to_string() }
                } else { "File does not exist".to_string() },
            }))
        }
        Err(e) => Ok(Response::new(CheckFileExistsResponse { success: false, exists: false, is_deleted: false, return_message: format!("Failed to check file existence: {}", e) })),
    }
}


















