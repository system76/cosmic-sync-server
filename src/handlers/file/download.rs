use tonic::{Response, Status};
use tracing::{error, info};

use crate::sync::{DownloadFileRequest, DownloadFileResponse};
use crate::utils::response;
use super::super::file_handler::FileHandler;

pub async fn handle_download_file(handler: &FileHandler, req: DownloadFileRequest) -> Result<Response<DownloadFileResponse>, Status> {
    let file_id = req.file_id;

    // Verify authentication
    match handler.app_state.oauth.verify_token(&req.auth_token).await {
        Ok(v) if v.valid => {}
        _ => return Ok(Response::new(response::file_download_error("Authentication failed"))),
    }

    // Get file info
    let file_info = match handler.app_state.file.get_file_info(file_id).await {
        Ok(Some(info)) => info,
        Ok(None) => {
            return Ok(Response::new(response::file_download_error("File not found")));
        }
        Err(e) => {
            error!("Failed to get file info: {}", e);
            return Ok(Response::new(response::file_download_error(format!("Failed to get file info: {}", e))));
        }
    };

    // Get file data
    match handler.app_state.file.get_file_data(file_id).await {
        Ok(Some(data)) => {
            info!("Sending file download response: file_id={}, filename={}, file_path= {}", file_id, file_info.filename, file_info.file_path);
            Ok(Response::new(DownloadFileResponse {
                success: true,
                file_data: data,
                file_hash: file_info.file_hash,
                is_encrypted: file_info.is_encrypted,
                return_message: "".to_string(),
                filename: file_info.filename.clone(),
                file_path: file_info.file_path.clone(),
                updated_time: Some(prost_types::Timestamp {
                    seconds: file_info.updated_time.seconds,
                    nanos: 0,
                }),
            }))
        }
        Ok(None) => Ok(Response::new(response::file_download_error("File data not found"))),
        Err(e) => {
            error!("Failed to get file data: {}", e);
            Ok(Response::new(response::file_download_error(format!("Failed to get file data: {}", e))))
        }
    }
}









