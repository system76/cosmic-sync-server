use tonic::{Response, Status};
use tracing::{error, info};

use super::super::file_handler::FileHandler;
use crate::sync::{DownloadFileRequest, DownloadFileResponse};
use crate::utils::response;
use base64::Engine as _;

fn parse_account_key(s: &str) -> Option<[u8; 32]> {
    if let Ok(bytes) = hex::decode(s) {
        if bytes.len() == 32 {
            return Some(bytes.try_into().ok()?);
        }
    }
    if let Ok(bytes) = base64::engine::general_purpose::STANDARD_NO_PAD.decode(s) {
        if bytes.len() == 32 {
            return Some(bytes.try_into().ok()?);
        }
    }
    if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(s) {
        if bytes.len() == 32 {
            return Some(bytes.try_into().ok()?);
        }
    }
    None
}

pub async fn handle_download_file(
    handler: &FileHandler,
    req: DownloadFileRequest,
) -> Result<Response<DownloadFileResponse>, Status> {
    let file_id = req.file_id;

    // Verify authentication
    match handler.app_state.oauth.verify_token(&req.auth_token).await {
        Ok(v) if v.valid => {}
        _ => {
            return Ok(Response::new(response::file_download_error(
                "Authentication failed",
            )))
        }
    }

    // Get file info
    let file_info = match handler.app_state.file.get_file_info(file_id).await {
        Ok(Some(info)) => info,
        Ok(None) => {
            return Ok(Response::new(response::file_download_error(
                "File not found",
            )));
        }
        Err(e) => {
            error!("Failed to get file info: {}", e);
            return Ok(Response::new(response::file_download_error(format!(
                "Failed to get file info: {}",
                e
            ))));
        }
    };

    // Get account transport key
    let account_key: Option<[u8; 32]> =
        if handler.app_state.config.features.transport_encrypt_metadata {
            match handler
                .app_state
                .storage
                .get_encryption_key(&file_info.account_hash)
                .await
            {
                Ok(Some(k)) => parse_account_key(&k),
                _ => None,
            }
        } else {
            None
        };
    let aad = format!("{}:{}", file_info.account_hash, req.device_hash);

    // Get file data
    match handler.app_state.file.get_file_data(file_id).await {
        Ok(Some(data)) => {
            let (enc_path, enc_name) = if let Some(key) = account_key.as_ref() {
                let ct_path = crate::utils::crypto::aead_encrypt(
                    key,
                    file_info.file_path.as_bytes(),
                    aad.as_bytes(),
                );
                let ct_name = crate::utils::crypto::aead_encrypt(
                    key,
                    file_info.filename.as_bytes(),
                    aad.as_bytes(),
                );
                (
                    base64::engine::general_purpose::STANDARD_NO_PAD.encode(ct_path),
                    base64::engine::general_purpose::STANDARD_NO_PAD.encode(ct_name),
                )
            } else {
                (file_info.file_path.clone(), file_info.filename.clone())
            };
            info!(
                "Sending file download response: file_id={}, filename={}, file_path= {}",
                file_id, enc_name, enc_path
            );
            Ok(Response::new(DownloadFileResponse {
                success: true,
                file_data: data,
                file_hash: file_info.file_hash,
                is_encrypted: file_info.is_encrypted,
                return_message: "".to_string(),
                filename: enc_name,
                file_path: enc_path,
                updated_time: Some(prost_types::Timestamp {
                    seconds: file_info.updated_time.seconds,
                    nanos: 0,
                }),
                file_size: file_info.size,
                key_id: file_info.key_id.clone().unwrap_or_default(),
            }))
        }
        Ok(None) => Ok(Response::new(response::file_download_error(
            "File data not found",
        ))),
        Err(e) => {
            error!("Failed to get file data: {}", e);
            Ok(Response::new(response::file_download_error(format!(
                "Failed to get file data: {}",
                e
            ))))
        }
    }
}
