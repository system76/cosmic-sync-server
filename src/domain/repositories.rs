// Domain repositories - Abstract data access interfaces

use crate::{
    domain::{Account, Device, Repository},
    error::Result,
};
use async_trait::async_trait;

/// 계정 리포지토리 인터페이스
#[async_trait]
pub trait AccountRepository: Repository<Account> + Send + Sync {
    /// 이메일로 계정 조회
    async fn find_by_email(&self, email: &str) -> Result<Option<Account>>;

    /// 계정 해시로 계정 조회
    async fn find_by_account_hash(&self, account_hash: &str) -> Result<Option<Account>>;

    /// 활성 계정만 조회
    async fn find_active_accounts(&self) -> Result<Vec<Account>>;

    /// 계정 존재 여부 확인
    async fn exists_by_email(&self, email: &str) -> Result<bool>;

    /// 계정 해시 존재 여부 확인
    async fn exists_by_account_hash(&self, account_hash: &str) -> Result<bool>;
}

/// 디바이스 리포지토리 인터페이스
#[async_trait]
pub trait DeviceRepository: Repository<Device> + Send + Sync {
    /// 계정의 모든 디바이스 조회
    async fn find_by_account_hash(&self, account_hash: &str) -> Result<Vec<Device>>;

    /// 활성 디바이스만 조회
    async fn find_active_by_account_hash(&self, account_hash: &str) -> Result<Vec<Device>>;

    /// 디바이스 해시로 조회
    async fn find_by_device_hash(&self, device_hash: &str) -> Result<Option<Device>>;

    /// 특정 계정의 디바이스 개수
    async fn count_by_account_hash(&self, account_hash: &str) -> Result<u64>;

    /// 비활성 디바이스 정리
    async fn cleanup_inactive_devices(&self, days_threshold: u32) -> Result<u64>;

    /// 디바이스 존재 여부 확인
    async fn exists(&self, account_hash: &str, device_hash: &str) -> Result<bool>;
}

/// 파일 리포지토리 인터페이스 (향후 확장)
#[async_trait]
pub trait FileRepository: Send + Sync {
    /// 파일 정보 저장
    async fn save_file_info(&self, file_info: &crate::models::file::FileInfo) -> Result<u64>;

    /// 파일 정보 조회
    async fn find_file_info(&self, file_id: u64) -> Result<Option<crate::models::file::FileInfo>>;

    /// 경로로 파일 조회
    async fn find_by_path(
        &self,
        account_hash: &str,
        path: &str,
        group_id: i32,
    ) -> Result<Option<crate::models::file::FileInfo>>;

    /// 계정의 파일 목록
    async fn list_files(
        &self,
        account_hash: &str,
        group_id: i32,
        limit: Option<u32>,
        offset: Option<u64>,
    ) -> Result<Vec<crate::models::file::FileInfo>>;
}

/// 워처 그룹 리포지토리 인터페이스 (향후 확장)
#[async_trait]
pub trait WatcherGroupRepository: Send + Sync {
    /// 워처 그룹 저장
    async fn save_watcher_group(
        &self,
        account_hash: &str,
        watcher_group: &crate::models::watcher::WatcherGroup,
    ) -> Result<i32>;

    /// 워처 그룹 조회
    async fn find_watcher_group(
        &self,
        account_hash: &str,
        group_id: i32,
    ) -> Result<Option<crate::models::watcher::WatcherGroup>>;

    /// 계정의 모든 워처 그룹
    async fn list_watcher_groups(
        &self,
        account_hash: &str,
    ) -> Result<Vec<crate::models::watcher::WatcherGroup>>;
}

/// 범용 검색 기능을 위한 명세 패턴 구현
pub struct AccountSearchSpec {
    pub email_pattern: Option<String>,
    pub is_active: Option<bool>,
    pub created_after: Option<i64>,
    pub created_before: Option<i64>,
}

impl AccountSearchSpec {
    pub fn new() -> Self {
        Self {
            email_pattern: None,
            is_active: None,
            created_after: None,
            created_before: None,
        }
    }

    pub fn with_email_pattern(mut self, pattern: String) -> Self {
        self.email_pattern = Some(pattern);
        self
    }

    pub fn with_active_status(mut self, is_active: bool) -> Self {
        self.is_active = Some(is_active);
        self
    }

    pub fn with_created_after(mut self, timestamp: i64) -> Self {
        self.created_after = Some(timestamp);
        self
    }

    pub fn with_created_before(mut self, timestamp: i64) -> Self {
        self.created_before = Some(timestamp);
        self
    }
}

impl crate::domain::Specification<Account> for AccountSearchSpec {
    fn is_satisfied_by(&self, account: &Account) -> bool {
        if let Some(ref pattern) = self.email_pattern {
            if !account.email.contains(pattern) {
                return false;
            }
        }

        if let Some(is_active) = self.is_active {
            if account.is_active != is_active {
                return false;
            }
        }

        if let Some(created_after) = self.created_after {
            if account.created_at <= created_after {
                return false;
            }
        }

        if let Some(created_before) = self.created_before {
            if account.created_at >= created_before {
                return false;
            }
        }

        true
    }
}

/// 디바이스 검색 명세
pub struct DeviceSearchSpec {
    pub account_hash: Option<String>,
    pub device_name_pattern: Option<String>,
    pub is_active: Option<bool>,
    pub last_seen_after: Option<i64>,
}

impl DeviceSearchSpec {
    pub fn new() -> Self {
        Self {
            account_hash: None,
            device_name_pattern: None,
            is_active: None,
            last_seen_after: None,
        }
    }

    pub fn with_account_hash(mut self, account_hash: String) -> Self {
        self.account_hash = Some(account_hash);
        self
    }

    pub fn with_device_name_pattern(mut self, pattern: String) -> Self {
        self.device_name_pattern = Some(pattern);
        self
    }

    pub fn with_active_status(mut self, is_active: bool) -> Self {
        self.is_active = Some(is_active);
        self
    }

    pub fn with_last_seen_after(mut self, timestamp: i64) -> Self {
        self.last_seen_after = Some(timestamp);
        self
    }
}

impl crate::domain::Specification<Device> for DeviceSearchSpec {
    fn is_satisfied_by(&self, device: &Device) -> bool {
        if let Some(ref account_hash) = self.account_hash {
            if device.account_hash != *account_hash {
                return false;
            }
        }

        if let Some(ref pattern) = self.device_name_pattern {
            if !device.device_name.contains(pattern) {
                return false;
            }
        }

        if let Some(is_active) = self.is_active {
            if device.is_active != is_active {
                return false;
            }
        }

        if let Some(last_seen_after) = self.last_seen_after {
            if device.last_seen <= last_seen_after {
                return false;
            }
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Specification;
    use crate::domain::{Account, DeviceType};

    #[test]
    fn test_account_search_spec() {
        let account =
            Account::create("test@example.com".to_string(), "hash123".to_string()).unwrap();

        let spec = AccountSearchSpec::new()
            .with_email_pattern("test".to_string())
            .with_active_status(true);

        assert!(spec.is_satisfied_by(&account));

        let spec = AccountSearchSpec::new().with_email_pattern("notfound".to_string());

        assert!(!spec.is_satisfied_by(&account));
    }

    #[test]
    fn test_device_search_spec() {
        let device = Device::register(
            "device123".to_string(),
            "account123".to_string(),
            "My Laptop".to_string(),
            DeviceType::Laptop,
        )
        .unwrap();

        let spec = DeviceSearchSpec::new()
            .with_account_hash("account123".to_string())
            .with_device_name_pattern("Laptop".to_string())
            .with_active_status(true);

        assert!(spec.is_satisfied_by(&device));

        let spec = DeviceSearchSpec::new().with_account_hash("different_account".to_string());

        assert!(!spec.is_satisfied_by(&device));
    }
}
