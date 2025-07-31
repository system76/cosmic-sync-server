// Middleware system for request/response processing pipeline
// Provides a flexible middleware architecture for intercepting and modifying
// requests and responses in a composable manner

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use serde_json::Value;
use crate::error::{Result, SyncError};

/// Request context that flows through the middleware pipeline
#[derive(Debug, Clone)]
pub struct RequestContext {
    pub request_id: String,
    pub user_id: Option<String>,
    pub device_id: Option<String>,
    pub headers: HashMap<String, String>,
    pub metadata: HashMap<String, Value>,
    pub timestamp: i64,
}

impl RequestContext {
    /// Create a new request context
    pub fn new(request_id: String) -> Self {
        Self {
            request_id,
            user_id: None,
            device_id: None,
            headers: HashMap::new(),
            metadata: HashMap::new(),
            timestamp: chrono::Utc::now().timestamp(),
        }
    }
    
    /// Set user ID
    pub fn with_user_id(mut self, user_id: String) -> Self {
        self.user_id = Some(user_id);
        self
    }
    
    /// Set device ID
    pub fn with_device_id(mut self, device_id: String) -> Self {
        self.device_id = Some(device_id);
        self
    }
    
    /// Add header
    pub fn add_header(&mut self, key: String, value: String) {
        self.headers.insert(key, value);
    }
    
    /// Add metadata
    pub fn add_metadata(&mut self, key: String, value: Value) {
        self.metadata.insert(key, value);
    }
    
    /// Get metadata value
    pub fn get_metadata(&self, key: &str) -> Option<&Value> {
        self.metadata.get(key)
    }
}

/// Response context for middleware processing
#[derive(Debug, Clone)]
pub struct ResponseContext {
    pub status_code: u16,
    pub headers: HashMap<String, String>,
    pub metadata: HashMap<String, Value>,
    pub processing_time_ms: u64,
}

impl ResponseContext {
    /// Create a new response context
    pub fn new(status_code: u16) -> Self {
        Self {
            status_code,
            headers: HashMap::new(),
            metadata: HashMap::new(),
            processing_time_ms: 0,
        }
    }
    
    /// Add header
    pub fn add_header(&mut self, key: String, value: String) {
        self.headers.insert(key, value);
    }
    
    /// Add metadata
    pub fn add_metadata(&mut self, key: String, value: Value) {
        self.metadata.insert(key, value);
    }
}

/// Middleware trait for processing requests and responses
#[async_trait]
pub trait Middleware: Send + Sync {
    /// Get middleware name for debugging and logging
    fn name(&self) -> &'static str;
    
    /// Get middleware priority (lower numbers execute first)
    fn priority(&self) -> i32 {
        100
    }
    
    /// Process request before it reaches the handler
    async fn process_request(
        &self,
        context: &mut RequestContext,
        request: &mut dyn MiddlewareRequest,
    ) -> Result<MiddlewareAction>;
    
    /// Process response after it comes from the handler
    async fn process_response(
        &self,
        context: &RequestContext,
        response: &mut dyn MiddlewareResponse,
        response_context: &mut ResponseContext,
    ) -> Result<MiddlewareAction>;
    
    /// Handle errors that occur during processing
    async fn handle_error(
        &self,
        context: &RequestContext,
        error: &SyncError,
    ) -> Result<ErrorHandlingAction> {
        Ok(ErrorHandlingAction::Continue)
    }
}

/// Actions that middleware can take
#[derive(Debug, Clone, PartialEq)]
pub enum MiddlewareAction {
    /// Continue to the next middleware or handler
    Continue,
    /// Stop processing and return early
    Stop,
    /// Skip remaining middleware but continue to handler
    Skip,
}

/// Error handling actions
#[derive(Debug, Clone, PartialEq)]
pub enum ErrorHandlingAction {
    /// Continue error propagation
    Continue,
    /// Handle the error and continue processing
    Handle,
    /// Transform the error into a different error
    Transform(SyncError),
}

/// Trait for middleware request objects
pub trait MiddlewareRequest: Send + Sync {
    /// Get request method
    fn method(&self) -> &str;
    
    /// Get request path
    fn path(&self) -> &str;
    
    /// Get request body as bytes
    fn body(&self) -> &[u8];
    
    /// Set request body
    fn set_body(&mut self, body: Vec<u8>);
    
    /// Get request headers
    fn headers(&self) -> &HashMap<String, String>;
    
    /// Set request header
    fn set_header(&mut self, key: String, value: String);
}

/// Trait for middleware response objects
pub trait MiddlewareResponse: Send + Sync {
    /// Get response status code
    fn status_code(&self) -> u16;
    
    /// Set response status code
    fn set_status_code(&mut self, code: u16);
    
    /// Get response body as bytes
    fn body(&self) -> &[u8];
    
    /// Set response body
    fn set_body(&mut self, body: Vec<u8>);
    
    /// Get response headers
    fn headers(&self) -> &HashMap<String, String>;
    
    /// Set response header
    fn set_header(&mut self, key: String, value: String);
}

/// Middleware pipeline for executing middleware in order
pub struct MiddlewarePipeline {
    middleware: Vec<Arc<dyn Middleware>>,
}

impl MiddlewarePipeline {
    /// Create a new middleware pipeline
    pub fn new() -> Self {
        Self {
            middleware: Vec::new(),
        }
    }
    
    /// Add middleware to the pipeline
    pub fn add_middleware(&mut self, middleware: Arc<dyn Middleware>) {
        self.middleware.push(middleware);
        // Sort by priority (lower numbers first)
        self.middleware.sort_by_key(|m| m.priority());
    }
    
    /// Execute the middleware pipeline for a request
    pub async fn execute_request(
        &self,
        mut context: RequestContext,
        request: &mut dyn MiddlewareRequest,
    ) -> Result<(RequestContext, MiddlewareAction)> {
        for middleware in &self.middleware {
            match middleware.process_request(&mut context, request).await? {
                MiddlewareAction::Continue => continue,
                MiddlewareAction::Stop => return Ok((context, MiddlewareAction::Stop)),
                MiddlewareAction::Skip => break,
            }
        }
        
        Ok((context, MiddlewareAction::Continue))
    }
    
    /// Execute the middleware pipeline for a response
    pub async fn execute_response(
        &self,
        context: &RequestContext,
        response: &mut dyn MiddlewareResponse,
        mut response_context: ResponseContext,
    ) -> Result<(ResponseContext, MiddlewareAction)> {
        // Execute response middleware in reverse order
        for middleware in self.middleware.iter().rev() {
            match middleware.process_response(context, response, &mut response_context).await? {
                MiddlewareAction::Continue => continue,
                MiddlewareAction::Stop => return Ok((response_context, MiddlewareAction::Stop)),
                MiddlewareAction::Skip => break,
            }
        }
        
        Ok((response_context, MiddlewareAction::Continue))
    }
    
    /// Handle errors through the middleware pipeline
    pub async fn handle_error(
        &self,
        context: &RequestContext,
        mut error: SyncError,
    ) -> Result<SyncError> {
        for middleware in &self.middleware {
            match middleware.handle_error(context, &error).await? {
                ErrorHandlingAction::Continue => continue,
                ErrorHandlingAction::Handle => return Ok(error),
                ErrorHandlingAction::Transform(new_error) => error = new_error,
            }
        }
        
        Ok(error)
    }
}

impl Default for MiddlewarePipeline {
    fn default() -> Self {
        Self::new()
    }
}

/// Authentication middleware for verifying user credentials
pub struct AuthenticationMiddleware {
    auth_service: Arc<dyn AuthenticationService>,
}

impl AuthenticationMiddleware {
    pub fn new(auth_service: Arc<dyn AuthenticationService>) -> Self {
        Self { auth_service }
    }
}

#[async_trait]
impl Middleware for AuthenticationMiddleware {
    fn name(&self) -> &'static str {
        "AuthenticationMiddleware"
    }
    
    fn priority(&self) -> i32 {
        10 // High priority - runs early
    }
    
    async fn process_request(
        &self,
        context: &mut RequestContext,
        request: &mut dyn MiddlewareRequest,
    ) -> Result<MiddlewareAction> {
        // Extract authorization header
        let auth_header = request.headers()
            .get("Authorization")
            .or_else(|| request.headers().get("authorization"));
        
        if let Some(auth_value) = auth_header {
            // Validate token
            match self.auth_service.validate_token(auth_value).await {
                Ok(user_info) => {
                    context.user_id = Some(user_info.user_id);
                    context.device_id = user_info.device_id;
                    context.add_metadata("auth_type".to_string(), Value::String("token".to_string()));
                    Ok(MiddlewareAction::Continue)
                }
                Err(_) => {
                    Err(SyncError::Authorization("Invalid authentication token".to_string()))
                }
            }
        } else {
            // Check if authentication is required for this path
            if self.requires_authentication(request.path()) {
                Err(SyncError::Authorization("Authentication required".to_string()))
            } else {
                Ok(MiddlewareAction::Continue)
            }
        }
    }
    
    async fn process_response(
        &self,
        _context: &RequestContext,
        _response: &mut dyn MiddlewareResponse,
        _response_context: &mut ResponseContext,
    ) -> Result<MiddlewareAction> {
        Ok(MiddlewareAction::Continue)
    }
}

impl AuthenticationMiddleware {
    fn requires_authentication(&self, path: &str) -> bool {
        // Define paths that require authentication
        let protected_paths = [
            "/api/files",
            "/api/devices",
            "/api/sync",
            "/api/account",
        ];
        
        protected_paths.iter().any(|&protected| path.starts_with(protected))
    }
}

/// Logging middleware for request/response logging
pub struct LoggingMiddleware {
    log_requests: bool,
    log_responses: bool,
}

impl LoggingMiddleware {
    pub fn new(log_requests: bool, log_responses: bool) -> Self {
        Self {
            log_requests,
            log_responses,
        }
    }
}

#[async_trait]
impl Middleware for LoggingMiddleware {
    fn name(&self) -> &'static str {
        "LoggingMiddleware"
    }
    
    fn priority(&self) -> i32 {
        1 // Very high priority - logs everything
    }
    
    async fn process_request(
        &self,
        context: &mut RequestContext,
        request: &mut dyn MiddlewareRequest,
    ) -> Result<MiddlewareAction> {
        if self.log_requests {
            tracing::info!(
                request_id = %context.request_id,
                method = %request.method(),
                path = %request.path(),
                user_id = ?context.user_id,
                "Incoming request"
            );
        }
        
        // Store start time for response processing
        context.add_metadata(
            "start_time".to_string(),
            Value::Number(serde_json::Number::from(chrono::Utc::now().timestamp_millis())),
        );
        
        Ok(MiddlewareAction::Continue)
    }
    
    async fn process_response(
        &self,
        context: &RequestContext,
        response: &mut dyn MiddlewareResponse,
        response_context: &mut ResponseContext,
    ) -> Result<MiddlewareAction> {
        if self.log_responses {
            // Calculate processing time
            if let Some(start_time) = context.get_metadata("start_time") {
                if let Some(start_ms) = start_time.as_i64() {
                    let now_ms = chrono::Utc::now().timestamp_millis();
                    response_context.processing_time_ms = (now_ms - start_ms) as u64;
                }
            }
            
            tracing::info!(
                request_id = %context.request_id,
                status_code = %response.status_code(),
                processing_time_ms = %response_context.processing_time_ms,
                user_id = ?context.user_id,
                "Request completed"
            );
        }
        
        Ok(MiddlewareAction::Continue)
    }
    
    async fn handle_error(
        &self,
        context: &RequestContext,
        error: &SyncError,
    ) -> Result<ErrorHandlingAction> {
        tracing::error!(
            request_id = %context.request_id,
            error = %error,
            user_id = ?context.user_id,
            "Request failed with error"
        );
        
        Ok(ErrorHandlingAction::Continue)
    }
}

/// Rate limiting middleware
pub struct RateLimitingMiddleware {
    rate_limiter: Arc<dyn RateLimiter>,
}

impl RateLimitingMiddleware {
    pub fn new(rate_limiter: Arc<dyn RateLimiter>) -> Self {
        Self { rate_limiter }
    }
}

#[async_trait]
impl Middleware for RateLimitingMiddleware {
    fn name(&self) -> &'static str {
        "RateLimitingMiddleware"
    }
    
    fn priority(&self) -> i32 {
        20 // Runs after authentication
    }
    
    async fn process_request(
        &self,
        context: &mut RequestContext,
        _request: &mut dyn MiddlewareRequest,
    ) -> Result<MiddlewareAction> {
        let key = context.user_id.as_ref()
            .unwrap_or(&context.request_id);
        
        if self.rate_limiter.is_allowed(key).await? {
            Ok(MiddlewareAction::Continue)
        } else {
            Err(SyncError::RateLimit("Rate limit exceeded".to_string()))
        }
    }
    
    async fn process_response(
        &self,
        _context: &RequestContext,
        response: &mut dyn MiddlewareResponse,
        response_context: &mut ResponseContext,
    ) -> Result<MiddlewareAction> {
        // Add rate limit headers
        response.set_header("X-RateLimit-Limit".to_string(), "100".to_string());
        response.set_header("X-RateLimit-Remaining".to_string(), "99".to_string());
        
        Ok(MiddlewareAction::Continue)
    }
}

/// Authentication service trait
#[async_trait]
pub trait AuthenticationService: Send + Sync {
    /// Validate authentication token
    async fn validate_token(&self, token: &str) -> Result<UserInfo>;
}

/// User information from authentication
#[derive(Debug, Clone)]
pub struct UserInfo {
    pub user_id: String,
    pub device_id: Option<String>,
    pub permissions: Vec<String>,
}

/// Rate limiter trait
#[async_trait]
pub trait RateLimiter: Send + Sync {
    /// Check if request is allowed for the given key
    async fn is_allowed(&self, key: &str) -> Result<bool>;
    
    /// Get current rate limit status
    async fn get_status(&self, key: &str) -> Result<RateLimitStatus>;
}

/// Rate limit status information
#[derive(Debug, Clone)]
pub struct RateLimitStatus {
    pub limit: u32,
    pub remaining: u32,
    pub reset_time: i64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct MockRequest {
        method: String,
        path: String,
        body: Vec<u8>,
        headers: HashMap<String, String>,
    }

    impl MockRequest {
        fn new(method: String, path: String) -> Self {
            Self {
                method,
                path,
                body: Vec::new(),
                headers: HashMap::new(),
            }
        }
    }

    impl MiddlewareRequest for MockRequest {
        fn method(&self) -> &str {
            &self.method
        }
        
        fn path(&self) -> &str {
            &self.path
        }
        
        fn body(&self) -> &[u8] {
            &self.body
        }
        
        fn set_body(&mut self, body: Vec<u8>) {
            self.body = body;
        }
        
        fn headers(&self) -> &HashMap<String, String> {
            &self.headers
        }
        
        fn set_header(&mut self, key: String, value: String) {
            self.headers.insert(key, value);
        }
    }

    struct MockResponse {
        status_code: u16,
        body: Vec<u8>,
        headers: HashMap<String, String>,
    }

    impl MockResponse {
        fn new(status_code: u16) -> Self {
            Self {
                status_code,
                body: Vec::new(),
                headers: HashMap::new(),
            }
        }
    }

    impl MiddlewareResponse for MockResponse {
        fn status_code(&self) -> u16 {
            self.status_code
        }
        
        fn set_status_code(&mut self, code: u16) {
            self.status_code = code;
        }
        
        fn body(&self) -> &[u8] {
            &self.body
        }
        
        fn set_body(&mut self, body: Vec<u8>) {
            self.body = body;
        }
        
        fn headers(&self) -> &HashMap<String, String> {
            &self.headers
        }
        
        fn set_header(&mut self, key: String, value: String) {
            self.headers.insert(key, value);
        }
    }

    #[tokio::test]
    async fn test_middleware_pipeline() {
        let mut pipeline = MiddlewarePipeline::new();
        
        // Add logging middleware
        let logging_middleware = Arc::new(LoggingMiddleware::new(true, true));
        pipeline.add_middleware(logging_middleware);
        
        // Test request processing
        let context = RequestContext::new("test-123".to_string());
        let mut request = MockRequest::new("GET".to_string(), "/api/test".to_string());
        
        let (context, action) = pipeline.execute_request(context, &mut request).await.unwrap();
        assert_eq!(action, MiddlewareAction::Continue);
        assert_eq!(context.request_id, "test-123");
        
        // Test response processing
        let mut response = MockResponse::new(200);
        let response_context = ResponseContext::new(200);
        
        let (response_context, action) = pipeline.execute_response(&context, &mut response, response_context).await.unwrap();
        assert_eq!(action, MiddlewareAction::Continue);
        assert!(response_context.processing_time_ms >= 0);
    }

    #[test]
    fn test_request_context() {
        let mut context = RequestContext::new("test-123".to_string())
            .with_user_id("user-456".to_string())
            .with_device_id("device-789".to_string());
        
        context.add_header("Content-Type".to_string(), "application/json".to_string());
        context.add_metadata("custom".to_string(), Value::String("value".to_string()));
        
        assert_eq!(context.request_id, "test-123");
        assert_eq!(context.user_id, Some("user-456".to_string()));
        assert_eq!(context.device_id, Some("device-789".to_string()));
        assert_eq!(context.headers.get("Content-Type"), Some(&"application/json".to_string()));
        assert_eq!(context.get_metadata("custom"), Some(&Value::String("value".to_string())));
    }
}
