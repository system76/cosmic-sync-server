// Minimal gRPC scenario test (English only in code)

use tonic::Request;

use cosmic_sync_server::handlers::auth_handler::AuthHandler;
use cosmic_sync_server::sync::VerifyLoginRequest;

mod common;

#[tokio::test]
async fn verify_login_with_invalid_token_returns_invalid() {
    let app_state = common::app_state_with_memory().await;
    let handler = AuthHandler::new(app_state.clone());

    let request = Request::new(VerifyLoginRequest {
        auth_token: "invalid".to_string(),
    });

    let response = handler.verify_login(request).await.unwrap();
    let inner = response.into_inner();

    assert!(!inner.valid);
}
