use tonic::{Response, Status};
use tracing::{error, info, debug};

use crate::sync::{DeleteFileRequest, DeleteFileResponse};
use crate::utils::response;
use super::super::file_handler::FileHandler;

pub async fn handle_delete_file(handler: &FileHandler, req: DeleteFileRequest) -> Result<Response<DeleteFileResponse>, Status> {
    info!("File deletion request received:");
    info!("   account_hash: {}", req.account_hash);
    info!("   file_id: {}", req.file_id);
    info!("   file_path: {}", req.file_path);
    info!("   filename: {}", req.filename);
    info!("   revision: {}", req.revision);

    match handler.app_state.oauth.verify_token(&req.auth_token).await {
        Ok(v) if v.valid => {}
        _ => return Ok(Response::new(response::file_delete_error("Authentication failed"))),
    }

    let file_id = if req.file_id > 0 {
        match handler.validate_file_for_deletion(req.file_id).await {
            Ok(id) => id,
            Err(resp) => return Ok(resp),
        }
    } else {
        match handler.find_file_by_path(&req).await {
            Ok(id) => id,
            Err(resp) => return Ok(resp),
        }
    };

    debug!("Executing file deletion: file_id={}", file_id);
    match handler.app_state.file.delete_file(file_id).await {
        Ok(_) => {
            info!("File deleted successfully: filename={}, file_id={}", req.filename, file_id);
            Ok(Response::new(response::file_delete_success("File deleted successfully")))
        }
        Err(e) => {
            error!("File deletion failed: file_id={}, error={}", file_id, e);
            Ok(Response::new(response::file_delete_error(format!("File deletion failed: {}", e))))
        }
    }
}


