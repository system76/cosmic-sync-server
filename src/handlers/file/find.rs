use tonic::{Response, Status};
use tracing::{debug, error};

use crate::sync::{FindFileRequest, FindFileResponse};
use super::super::file_handler::FileHandler;

pub async fn handle_find_file_by_criteria(handler: &FileHandler, req: FindFileRequest) -> Result<Response<FindFileResponse>, Status> {
    debug!("FindFileByCriteria request: account={}, file_path={}, file_name={}", req.account_hash, req.file_path, req.file_name);

    match handler.app_state.oauth.verify_token(&req.auth_token).await {
        Ok(v) if v.valid => {}
        _ => {
            return Ok(Response::new(FindFileResponse {
                success: false,
                return_message: "Authentication failed".to_string(),
                file_id: 0,
                revision: 0,
                file_info: None,
            }));
        }
    }

    let (server_group_id, server_watcher_id) = if req.group_id > 0 && req.watcher_id > 0 {
        match handler.app_state.file.convert_client_ids_to_server(&req.account_hash, req.group_id, req.watcher_id).await {
            Ok(Some((gid, wid))) => (gid, wid),
            Ok(None) => {
                return Ok(Response::new(FindFileResponse {
                    success: false,
                    return_message: "Group or watcher not found".to_string(),
                    file_id: 0,
                    revision: 0,
                    file_info: None,
                }));
            }
            Err(e) => {
                error!("Failed to convert IDs: {}", e);
                return Ok(Response::new(FindFileResponse {
                    success: false,
                    return_message: format!("Failed to convert IDs: {}", e),
                    file_id: 0,
                    revision: 0,
                    file_info: None,
                }));
            }
        }
    } else { (0, 0) };

    let normalized_file_path = crate::utils::helpers::normalize_path_preserve_tilde(&req.file_path);
    debug!("Find - Original path: {}, Normalized path: {}", req.file_path, normalized_file_path);

    match handler.app_state.file.find_file_by_criteria(&req.account_hash, server_group_id, server_watcher_id, &normalized_file_path, &req.file_name).await {
        Ok(Some(file_info)) => {
            let proto_file_info = crate::sync::FileInfo {
                file_id: file_info.file_id,
                filename: file_info.filename.clone(),
                file_hash: file_info.file_hash.clone(),
                device_hash: file_info.device_hash.clone(),
                group_id: req.group_id,
                watcher_id: req.watcher_id,
                is_encrypted: file_info.is_encrypted,
                file_path: file_info.file_path.clone(),
                updated_time: Some(prost_types::Timestamp { seconds: file_info.updated_time.seconds, nanos: 0 }),
                revision: file_info.revision,
                file_size: file_info.size,
            };
            Ok(Response::new(FindFileResponse { success: true, return_message: "File found".to_string(), file_id: file_info.file_id, revision: file_info.revision, file_info: Some(proto_file_info) }))
        }
        Ok(None) => Ok(Response::new(FindFileResponse { success: false, return_message: "File not found".to_string(), file_id: 0, revision: 0, file_info: None })),
        Err(e) => Ok(Response::new(FindFileResponse { success: false, return_message: format!("File search error: {}", e), file_id: 0, revision: 0, file_info: None })),
    }
}


