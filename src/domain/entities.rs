// Domain entities - Core business objects
// 기존 models를 도메인 엔티티로 점진적 마이그레이션

use serde::{Serialize, Deserialize};
use crate::{
    domain::{Entity, AggregateRoot, DomainEvent, ValueObject},
    error::{Result, SyncError},
};

/// 계정 도메인 엔티티
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub email: String,
    pub account_hash: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub is_active: bool,
    events: Vec<AccountEvent>,
}

impl Account {
    /// 새 계정 생성
    pub fn create(email: String, account_hash: String) -> Result<Self> {
        let now = chrono::Utc::now().timestamp();
        
        let mut account = Self {
            id: uuid::Uuid::new_v4().to_string(),
            email: email.clone(),
            account_hash: account_hash.clone(),
            created_at: now,
            updated_at: now,
            is_active: true,
            events: Vec::new(),
        };
        
        // 계정 생성 이벤트 추가
        account.events.push(AccountEvent::Created {
            account_id: account.id.clone(),
            email,
            timestamp: now,
        });
        
        account.validate()?;
        Ok(account)
    }
    
    /// 계정 비활성화
    pub fn deactivate(&mut self) -> Result<()> {
        if !self.is_active {
            return Err(SyncError::Validation("Account is already inactive".to_string()));
        }
        
        self.is_active = false;
        self.updated_at = chrono::Utc::now().timestamp();
        
        self.events.push(AccountEvent::Deactivated {
            account_id: self.id.clone(),
            timestamp: self.updated_at,
        });
        
        Ok(())
    }
    
    /// 이메일 업데이트
    pub fn update_email(&mut self, new_email: String) -> Result<()> {
        if new_email == self.email {
            return Ok(()); // 변경사항 없음
        }
        
        // 이메일 검증
        if !new_email.contains('@') {
            return Err(SyncError::Validation("Invalid email format".to_string()));
        }
        
        let old_email = self.email.clone();
        self.email = new_email.clone();
        self.updated_at = chrono::Utc::now().timestamp();
        
        self.events.push(AccountEvent::EmailUpdated {
            account_id: self.id.clone(),
            old_email,
            new_email,
            timestamp: self.updated_at,
        });
        
        Ok(())
    }
}

impl Entity for Account {
    type Id = String;
    
    fn id(&self) -> &Self::Id {
        &self.id
    }
    
    fn validate(&self) -> Result<()> {
        if self.id.is_empty() {
            return Err(SyncError::Validation("Account ID cannot be empty".to_string()));
        }
        
        if self.email.is_empty() || !self.email.contains('@') {
            return Err(SyncError::Validation("Invalid email address".to_string()));
        }
        
        if self.account_hash.is_empty() {
            return Err(SyncError::Validation("Account hash cannot be empty".to_string()));
        }
        
        if self.created_at <= 0 || self.updated_at <= 0 {
            return Err(SyncError::Validation("Invalid timestamps".to_string()));
        }
        
        if self.updated_at < self.created_at {
            return Err(SyncError::Validation("Updated time cannot be before created time".to_string()));
        }
        
        Ok(())
    }
}

impl AggregateRoot for Account {
    type Event = AccountEvent;
    
    fn events(&self) -> Vec<Self::Event> {
        self.events.clone()
    }
    
    fn clear_events(&mut self) {
        self.events.clear();
    }
}

/// 계정 관련 도메인 이벤트
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AccountEvent {
    Created {
        account_id: String,
        email: String,
        timestamp: i64,
    },
    EmailUpdated {
        account_id: String,
        old_email: String,
        new_email: String,
        timestamp: i64,
    },
    Deactivated {
        account_id: String,
        timestamp: i64,
    },
}

impl DomainEvent for AccountEvent {
    fn event_type(&self) -> &'static str {
        match self {
            AccountEvent::Created { .. } => "account.created",
            AccountEvent::EmailUpdated { .. } => "account.email_updated",
            AccountEvent::Deactivated { .. } => "account.deactivated",
        }
    }
    
    fn timestamp(&self) -> i64 {
        match self {
            AccountEvent::Created { timestamp, .. } => *timestamp,
            AccountEvent::EmailUpdated { timestamp, .. } => *timestamp,
            AccountEvent::Deactivated { timestamp, .. } => *timestamp,
        }
    }
    
    fn event_id(&self) -> String {
        uuid::Uuid::new_v4().to_string()
    }
}

/// 디바이스 도메인 엔티티
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Device {
    pub device_hash: String,
    pub account_hash: String,
    pub device_name: String,
    pub device_type: DeviceType,
    pub last_seen: i64,
    pub is_active: bool,
    pub created_at: i64,
    events: Vec<DeviceEvent>,
}

/// 디바이스 타입 값 객체
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DeviceType {
    Desktop,
    Laptop,
    Mobile,
    Tablet,
    Server,
    Other(String),
}

impl ValueObject for DeviceType {
    fn validate(&self) -> Result<()> {
        match self {
            DeviceType::Other(name) if name.is_empty() => {
                Err(SyncError::Validation("Device type name cannot be empty".to_string()))
            }
            _ => Ok(()),
        }
    }
}

impl Device {
    /// 새 디바이스 등록
    pub fn register(
        device_hash: String,
        account_hash: String,
        device_name: String,
        device_type: DeviceType,
    ) -> Result<Self> {
        let now = chrono::Utc::now().timestamp();
        
        device_type.validate()?;
        
        let mut device = Self {
            device_hash: device_hash.clone(),
            account_hash: account_hash.clone(),
            device_name: device_name.clone(),
            device_type: device_type.clone(),
            last_seen: now,
            is_active: true,
            created_at: now,
            events: Vec::new(),
        };
        
        device.events.push(DeviceEvent::Registered {
            device_hash,
            account_hash,
            device_name,
            device_type,
            timestamp: now,
        });
        
        device.validate()?;
        Ok(device)
    }
    
    /// 디바이스 활동 업데이트
    pub fn update_activity(&mut self) {
        self.last_seen = chrono::Utc::now().timestamp();
        
        self.events.push(DeviceEvent::ActivityUpdated {
            device_hash: self.device_hash.clone(),
            timestamp: self.last_seen,
        });
    }
    
    /// 디바이스 비활성화
    pub fn deactivate(&mut self) -> Result<()> {
        if !self.is_active {
            return Err(SyncError::Validation("Device is already inactive".to_string()));
        }
        
        self.is_active = false;
        let timestamp = chrono::Utc::now().timestamp();
        
        self.events.push(DeviceEvent::Deactivated {
            device_hash: self.device_hash.clone(),
            timestamp,
        });
        
        Ok(())
    }
}

impl Entity for Device {
    type Id = String;
    
    fn id(&self) -> &Self::Id {
        &self.device_hash
    }
    
    fn validate(&self) -> Result<()> {
        if self.device_hash.is_empty() {
            return Err(SyncError::Validation("Device hash cannot be empty".to_string()));
        }
        
        if self.account_hash.is_empty() {
            return Err(SyncError::Validation("Account hash cannot be empty".to_string()));
        }
        
        if self.device_name.is_empty() {
            return Err(SyncError::Validation("Device name cannot be empty".to_string()));
        }
        
        if self.created_at <= 0 || self.last_seen <= 0 {
            return Err(SyncError::Validation("Invalid timestamps".to_string()));
        }
        
        self.device_type.validate()?;
        
        Ok(())
    }
}

impl AggregateRoot for Device {
    type Event = DeviceEvent;
    
    fn events(&self) -> Vec<Self::Event> {
        self.events.clone()
    }
    
    fn clear_events(&mut self) {
        self.events.clear();
    }
}

/// 디바이스 관련 도메인 이벤트
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DeviceEvent {
    Registered {
        device_hash: String,
        account_hash: String,
        device_name: String,
        device_type: DeviceType,
        timestamp: i64,
    },
    ActivityUpdated {
        device_hash: String,
        timestamp: i64,
    },
    Deactivated {
        device_hash: String,
        timestamp: i64,
    },
}

impl DomainEvent for DeviceEvent {
    fn event_type(&self) -> &'static str {
        match self {
            DeviceEvent::Registered { .. } => "device.registered",
            DeviceEvent::ActivityUpdated { .. } => "device.activity_updated",
            DeviceEvent::Deactivated { .. } => "device.deactivated",
        }
    }
    
    fn timestamp(&self) -> i64 {
        match self {
            DeviceEvent::Registered { timestamp, .. } => *timestamp,
            DeviceEvent::ActivityUpdated { timestamp, .. } => *timestamp,
            DeviceEvent::Deactivated { timestamp, .. } => *timestamp,
        }
    }
    
    fn event_id(&self) -> String {
        uuid::Uuid::new_v4().to_string()
    }
}

// 기존 models와의 호환성을 위한 변환 구현
impl From<crate::models::account::Account> for Account {
    fn from(model: crate::models::account::Account) -> Self {
        Self {
            id: model.id,
            email: model.email,
            account_hash: model.account_hash,
            created_at: model.created_at.timestamp(),
            updated_at: model.updated_at.timestamp(),
            is_active: model.is_active,
            events: Vec::new(),
        }
    }
}

impl From<Account> for crate::models::account::Account {
    fn from(entity: Account) -> Self {
        use chrono::TimeZone;
        
        let id = entity.id.clone(); // Clone first to avoid move
        
        Self {
            account_hash: entity.account_hash,
            id: id.clone(),
            email: entity.email,
            name: "Unknown".to_string(), // Default value
            user_type: "user".to_string(), // Default value
            password_hash: "".to_string(), // Default value
            salt: "".to_string(), // Default value
            is_active: entity.is_active,
            created_at: chrono::Utc.timestamp_opt(entity.created_at, 0).single().unwrap_or_else(|| chrono::Utc::now()),
            last_login: chrono::Utc::now(), // Default value
            updated_at: chrono::Utc.timestamp_opt(entity.updated_at, 0).single().unwrap_or_else(|| chrono::Utc::now()),
            user_id: id,
        }
    }
}

impl From<crate::models::device::Device> for Device {
    fn from(model: crate::models::device::Device) -> Self {
        // Extract device type from os_version for classification
        let device_type = if model.os_version.to_lowercase().contains("desktop") {
            DeviceType::Desktop
        } else if model.os_version.to_lowercase().contains("laptop") {
            DeviceType::Laptop
        } else if model.os_version.to_lowercase().contains("mobile") {
            DeviceType::Mobile
        } else if model.os_version.to_lowercase().contains("tablet") {
            DeviceType::Tablet
        } else {
            DeviceType::Other(model.os_version.clone())
        };
        
        Self {
            device_hash: model.device_hash,
            account_hash: model.account_hash,
            device_name: model.user_id.clone(), // Use user_id as device name
            device_type,
            last_seen: model.last_sync.timestamp(),
            is_active: model.is_active,
            created_at: model.registered_at.timestamp(),
            events: Vec::new(),
        }
    }
}

impl From<Device> for crate::models::device::Device {
    fn from(entity: Device) -> Self {
        use chrono::TimeZone;
        
        let os_version = match entity.device_type {
            DeviceType::Desktop => "Desktop OS",
            DeviceType::Laptop => "Laptop OS",
            DeviceType::Mobile => "Mobile OS",
            DeviceType::Tablet => "Tablet OS",
            DeviceType::Server => "Server OS",
            DeviceType::Other(ref name) => name,
        };
        
        Self {
            user_id: entity.device_name,
            account_hash: entity.account_hash,
            device_hash: entity.device_hash,
            updated_at: chrono::Utc::now(), // Default to now
            registered_at: chrono::Utc.timestamp_opt(entity.created_at, 0).single().unwrap_or_else(|| chrono::Utc::now()),
            last_sync: chrono::Utc.timestamp_opt(entity.last_seen, 0).single().unwrap_or_else(|| chrono::Utc::now()),
            is_active: entity.is_active,
            os_version: os_version.to_string(),
            app_version: "1.0.0".to_string(), // Default version
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_account_creation() {
        let account = Account::create(
            "test@example.com".to_string(),
            "hash123".to_string(),
        ).unwrap();
        
        assert_eq!(account.email, "test@example.com");
        assert_eq!(account.account_hash, "hash123");
        assert!(account.is_active);
        assert_eq!(account.events.len(), 1);
        
        match &account.events[0] {
            AccountEvent::Created { email, .. } => {
                assert_eq!(email, "test@example.com");
            }
            _ => panic!("Expected Created event"),
        }
    }

    #[test]
    fn test_device_registration() {
        let device = Device::register(
            "device123".to_string(),
            "account123".to_string(),
            "My Laptop".to_string(),
            DeviceType::Laptop,
        ).unwrap();
        
        assert_eq!(device.device_hash, "device123");
        assert_eq!(device.device_name, "My Laptop");
        assert_eq!(device.device_type, DeviceType::Laptop);
        assert!(device.is_active);
        assert_eq!(device.events.len(), 1);
    }

    #[test]
    fn test_account_validation() {
        let result = Account::create(
            "invalid-email".to_string(),
            "hash123".to_string(),
        );
        
        assert!(result.is_err());
    }
}
