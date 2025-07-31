// Domain services - Complex business logic that doesn't belong to entities

use async_trait::async_trait;
use crate::{
    domain::{DomainService, Account, Device, DeviceType},
    error::{Result, SyncError},
};

/// 계정 도메인 서비스
/// 계정 관련 복잡한 비즈니스 로직을 처리
#[derive(Clone)]
pub struct AccountDomainService;

impl AccountDomainService {
    pub fn new() -> Self {
        Self
    }
    
    /// 이메일 중복 확인 및 계정 생성
    pub async fn create_account_with_validation(
        &self,
        email: String,
        account_hash: String,
    ) -> Result<Account> {
        // 이메일 형식 검증
        if !self.is_valid_email(&email) {
            return Err(SyncError::Validation("Invalid email format".to_string()));
        }
        
        // 계정 해시 유효성 검증
        if !self.is_valid_account_hash(&account_hash) {
            return Err(SyncError::Validation("Invalid account hash format".to_string()));
        }
        
        Account::create(email, account_hash)
    }
    
    /// 이메일 유효성 검증
    fn is_valid_email(&self, email: &str) -> bool {
        // 간단한 이메일 검증 로직
        email.contains('@') && email.contains('.') && email.len() > 5
    }
    
    /// 계정 해시 유효성 검증
    fn is_valid_account_hash(&self, hash: &str) -> bool {
        // 해시 길이 및 형식 검증
        hash.len() >= 32 && hash.chars().all(|c| c.is_ascii_alphanumeric())
    }
}

#[async_trait]
impl DomainService for AccountDomainService {
    fn name(&self) -> &'static str {
        "AccountDomainService"
    }
    
    async fn health_check(&self) -> Result<bool> {
        // 서비스 상태 확인 로직
        Ok(true)
    }
}

/// 디바이스 도메인 서비스
/// 디바이스 관련 복잡한 비즈니스 로직을 처리
#[derive(Clone)]
pub struct DeviceDomainService;

impl DeviceDomainService {
    pub fn new() -> Self {
        Self
    }
    
    /// 디바이스 타입 추론
    pub fn infer_device_type(&self, device_name: &str, user_agent: Option<&str>) -> DeviceType {
        let name_lower = device_name.to_lowercase();
        
        // User-Agent 기반 추론
        if let Some(ua) = user_agent {
            let ua_lower = ua.to_lowercase();
            if ua_lower.contains("mobile") || ua_lower.contains("android") || ua_lower.contains("iphone") {
                return DeviceType::Mobile;
            }
            if ua_lower.contains("tablet") || ua_lower.contains("ipad") {
                return DeviceType::Tablet;
            }
        }
        
        // 디바이스 이름 기반 추론
        if name_lower.contains("desktop") || name_lower.contains("pc") {
            DeviceType::Desktop
        } else if name_lower.contains("laptop") || name_lower.contains("notebook") {
            DeviceType::Laptop
        } else if name_lower.contains("server") {
            DeviceType::Server
        } else if name_lower.contains("mobile") || name_lower.contains("phone") {
            DeviceType::Mobile
        } else if name_lower.contains("tablet") || name_lower.contains("pad") {
            DeviceType::Tablet
        } else {
            DeviceType::Other(device_name.to_string())
        }
    }
    
    /// 안전한 디바이스 등록
    pub async fn register_device_safely(
        &self,
        device_hash: String,
        account_hash: String,
        device_name: String,
        user_agent: Option<&str>,
    ) -> Result<Device> {
        // 디바이스 해시 유효성 검증
        if !self.is_valid_device_hash(&device_hash) {
            return Err(SyncError::Validation("Invalid device hash format".to_string()));
        }
        
        // 디바이스 이름 검증
        if device_name.trim().is_empty() {
            return Err(SyncError::Validation("Device name cannot be empty".to_string()));
        }
        
        // 디바이스 타입 추론
        let device_type = self.infer_device_type(&device_name, user_agent);
        
        Device::register(device_hash, account_hash, device_name, device_type)
    }
    
    /// 디바이스 해시 유효성 검증
    fn is_valid_device_hash(&self, hash: &str) -> bool {
        hash.len() >= 32 && hash.chars().all(|c| c.is_ascii_alphanumeric())
    }
    
    /// 디바이스 중복 등록 방지
    pub async fn can_register_device(
        &self,
        account_hash: &str,
        device_hash: &str,
    ) -> Result<bool> {
        // 실제 구현에서는 repository를 통해 중복 확인
        // 여기서는 기본 검증만 수행
        if account_hash.is_empty() || device_hash.is_empty() {
            return Ok(false);
        }
        
        Ok(true)
    }
}

#[async_trait]
impl DomainService for DeviceDomainService {
    fn name(&self) -> &'static str {
        "DeviceDomainService"
    }
    
    async fn health_check(&self) -> Result<bool> {
        Ok(true)
    }
}

/// 파일 동기화 도메인 서비스
/// 파일 동기화 관련 복잡한 비즈니스 로직을 처리
#[derive(Clone)]
pub struct FileSyncDomainService;

impl FileSyncDomainService {
    pub fn new() -> Self {
        Self
    }
    
    /// 파일 충돌 해결 전략 결정
    pub fn determine_conflict_resolution_strategy(
        &self,
        local_timestamp: i64,
        remote_timestamp: i64,
        local_size: u64,
        remote_size: u64,
    ) -> ConflictResolutionStrategy {
        // 타임스탬프 기반 우선 해결
        if local_timestamp > remote_timestamp {
            ConflictResolutionStrategy::UseLocal
        } else if remote_timestamp > local_timestamp {
            ConflictResolutionStrategy::UseRemote
        } else {
            // 타임스탬프가 같은 경우 크기로 판단
            if local_size != remote_size {
                ConflictResolutionStrategy::CreateBothVersions
            } else {
                ConflictResolutionStrategy::UseLocal // 기본값
            }
        }
    }
    
    /// 파일 동기화 우선순위 계산
    pub fn calculate_sync_priority(
        &self,
        file_size: u64,
        file_age_seconds: u64,
        is_user_initiated: bool,
    ) -> SyncPriority {
        if is_user_initiated {
            return SyncPriority::High;
        }
        
        // 파일 크기와 나이를 기반으로 우선순위 결정
        match (file_size, file_age_seconds) {
            (size, _) if size > 100 * 1024 * 1024 => SyncPriority::Low, // 100MB 이상
            (_, age) if age < 300 => SyncPriority::High, // 5분 이내
            (_, age) if age < 3600 => SyncPriority::Medium, // 1시간 이내
            _ => SyncPriority::Low,
        }
    }
}

#[async_trait]
impl DomainService for FileSyncDomainService {
    fn name(&self) -> &'static str {
        "FileSyncDomainService"
    }
    
    async fn health_check(&self) -> Result<bool> {
        Ok(true)
    }
}

/// 충돌 해결 전략
#[derive(Debug, Clone, PartialEq)]
pub enum ConflictResolutionStrategy {
    UseLocal,
    UseRemote,
    CreateBothVersions,
    MergeChanges,
    AskUser,
}

/// 동기화 우선순위
#[derive(Debug, Clone, PartialEq)]
pub enum SyncPriority {
    Low,
    Medium,
    High,
    Critical,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_account_domain_service() {
        let service = AccountDomainService::new();
        
        // 유효한 계정 생성
        let result = service.create_account_with_validation(
            "test@example.com".to_string(),
            "abcd1234567890abcd1234567890abcd".to_string(),
        ).await;
        assert!(result.is_ok());
        
        // 무효한 이메일
        let result = service.create_account_with_validation(
            "invalid-email".to_string(),
            "abcd1234567890abcd1234567890abcd".to_string(),
        ).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_device_domain_service() {
        let service = DeviceDomainService::new();
        
        // 디바이스 타입 추론 테스트
        assert_eq!(
            service.infer_device_type("My Laptop", None),
            DeviceType::Laptop
        );
        
        assert_eq!(
            service.infer_device_type("Work Desktop", None),
            DeviceType::Desktop
        );
        
        // User-Agent 기반 추론
        assert_eq!(
            service.infer_device_type("Unknown", Some("Mozilla/5.0 (iPhone; CPU iPhone OS 14_0 like Mac OS X)")),
            DeviceType::Mobile
        );
    }

    #[test]
    fn test_file_sync_domain_service() {
        let service = FileSyncDomainService::new();
        
        // 충돌 해결 전략 테스트
        let strategy = service.determine_conflict_resolution_strategy(
            1000, 900, 100, 100
        );
        assert_eq!(strategy, ConflictResolutionStrategy::UseLocal);
        
        // 동기화 우선순위 테스트
        let priority = service.calculate_sync_priority(1000, 100, false);
        assert_eq!(priority, SyncPriority::High);
        
        let priority = service.calculate_sync_priority(200 * 1024 * 1024, 100, false);
        assert_eq!(priority, SyncPriority::Low);
    }
}
