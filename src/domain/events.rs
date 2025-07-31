// Domain events - Events that occur within the domain

use serde::{Serialize, Deserialize};
use crate::{
    domain::DomainEvent,
    error::Result,
};

/// Base event structure that all domain events should implement
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaseEvent {
    pub event_id: String,
    pub timestamp: i64,
    pub event_type: String,
    pub aggregate_id: String,
    pub version: i64,
}

impl BaseEvent {
    /// Create a new base event
    pub fn new(event_type: String, aggregate_id: String) -> Self {
        Self {
            event_id: uuid::Uuid::new_v4().to_string(),
            timestamp: chrono::Utc::now().timestamp(),
            event_type,
            aggregate_id,
            version: 1,
        }
    }
}

/// Account-related domain events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AccountDomainEvent {
    AccountCreated {
        base: BaseEvent,
        account_id: String,
        email: String,
        account_hash: String,
    },
    AccountEmailUpdated {
        base: BaseEvent,
        account_id: String,
        old_email: String,
        new_email: String,
    },
    AccountDeactivated {
        base: BaseEvent,
        account_id: String,
        reason: Option<String>,
    },
    AccountReactivated {
        base: BaseEvent,
        account_id: String,
    },
}

impl DomainEvent for AccountDomainEvent {
    fn event_type(&self) -> &'static str {
        match self {
            AccountDomainEvent::AccountCreated { .. } => "account.created",
            AccountDomainEvent::AccountEmailUpdated { .. } => "account.email_updated",
            AccountDomainEvent::AccountDeactivated { .. } => "account.deactivated",
            AccountDomainEvent::AccountReactivated { .. } => "account.reactivated",
        }
    }
    
    fn timestamp(&self) -> i64 {
        match self {
            AccountDomainEvent::AccountCreated { base, .. } => base.timestamp,
            AccountDomainEvent::AccountEmailUpdated { base, .. } => base.timestamp,
            AccountDomainEvent::AccountDeactivated { base, .. } => base.timestamp,
            AccountDomainEvent::AccountReactivated { base, .. } => base.timestamp,
        }
    }
    
    fn event_id(&self) -> String {
        match self {
            AccountDomainEvent::AccountCreated { base, .. } => base.event_id.clone(),
            AccountDomainEvent::AccountEmailUpdated { base, .. } => base.event_id.clone(),
            AccountDomainEvent::AccountDeactivated { base, .. } => base.event_id.clone(),
            AccountDomainEvent::AccountReactivated { base, .. } => base.event_id.clone(),
        }
    }
}

/// Device-related domain events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DeviceDomainEvent {
    DeviceRegistered {
        base: BaseEvent,
        device_hash: String,
        account_hash: String,
        device_name: String,
        device_type: String,
    },
    DeviceActivityUpdated {
        base: BaseEvent,
        device_hash: String,
        last_seen: i64,
    },
    DeviceDeactivated {
        base: BaseEvent,
        device_hash: String,
        reason: Option<String>,
    },
    DeviceNameUpdated {
        base: BaseEvent,
        device_hash: String,
        old_name: String,
        new_name: String,
    },
}

impl DomainEvent for DeviceDomainEvent {
    fn event_type(&self) -> &'static str {
        match self {
            DeviceDomainEvent::DeviceRegistered { .. } => "device.registered",
            DeviceDomainEvent::DeviceActivityUpdated { .. } => "device.activity_updated",
            DeviceDomainEvent::DeviceDeactivated { .. } => "device.deactivated",
            DeviceDomainEvent::DeviceNameUpdated { .. } => "device.name_updated",
        }
    }
    
    fn timestamp(&self) -> i64 {
        match self {
            DeviceDomainEvent::DeviceRegistered { base, .. } => base.timestamp,
            DeviceDomainEvent::DeviceActivityUpdated { base, .. } => base.timestamp,
            DeviceDomainEvent::DeviceDeactivated { base, .. } => base.timestamp,
            DeviceDomainEvent::DeviceNameUpdated { base, .. } => base.timestamp,
        }
    }
    
    fn event_id(&self) -> String {
        match self {
            DeviceDomainEvent::DeviceRegistered { base, .. } => base.event_id.clone(),
            DeviceDomainEvent::DeviceActivityUpdated { base, .. } => base.event_id.clone(),
            DeviceDomainEvent::DeviceDeactivated { base, .. } => base.event_id.clone(),
            DeviceDomainEvent::DeviceNameUpdated { base, .. } => base.event_id.clone(),
        }
    }
}

/// File synchronization events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FileSyncEvent {
    FileUploaded {
        base: BaseEvent,
        file_id: u64,
        account_hash: String,
        device_hash: String,
        file_path: String,
        file_size: u64,
        file_hash: String,
    },
    FileDownloaded {
        base: BaseEvent,
        file_id: u64,
        account_hash: String,
        device_hash: String,
    },
    FileDeleted {
        base: BaseEvent,
        file_id: u64,
        account_hash: String,
        device_hash: String,
        file_path: String,
    },
    FileSyncConflictDetected {
        base: BaseEvent,
        file_id: u64,
        account_hash: String,
        conflicting_devices: Vec<String>,
        resolution_strategy: String,
    },
    FileSyncCompleted {
        base: BaseEvent,
        file_id: u64,
        account_hash: String,
        synced_devices: Vec<String>,
    },
}

impl DomainEvent for FileSyncEvent {
    fn event_type(&self) -> &'static str {
        match self {
            FileSyncEvent::FileUploaded { .. } => "file.uploaded",
            FileSyncEvent::FileDownloaded { .. } => "file.downloaded",
            FileSyncEvent::FileDeleted { .. } => "file.deleted",
            FileSyncEvent::FileSyncConflictDetected { .. } => "file.sync_conflict_detected",
            FileSyncEvent::FileSyncCompleted { .. } => "file.sync_completed",
        }
    }
    
    fn timestamp(&self) -> i64 {
        match self {
            FileSyncEvent::FileUploaded { base, .. } => base.timestamp,
            FileSyncEvent::FileDownloaded { base, .. } => base.timestamp,
            FileSyncEvent::FileDeleted { base, .. } => base.timestamp,
            FileSyncEvent::FileSyncConflictDetected { base, .. } => base.timestamp,
            FileSyncEvent::FileSyncCompleted { base, .. } => base.timestamp,
        }
    }
    
    fn event_id(&self) -> String {
        match self {
            FileSyncEvent::FileUploaded { base, .. } => base.event_id.clone(),
            FileSyncEvent::FileDownloaded { base, .. } => base.event_id.clone(),
            FileSyncEvent::FileDeleted { base, .. } => base.event_id.clone(),
            FileSyncEvent::FileSyncConflictDetected { base, .. } => base.event_id.clone(),
            FileSyncEvent::FileSyncCompleted { base, .. } => base.event_id.clone(),
        }
    }
}

/// Generic event wrapper for all domain events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DomainEventWrapper {
    Account(AccountDomainEvent),
    Device(DeviceDomainEvent),
    FileSync(FileSyncEvent),
}

impl DomainEvent for DomainEventWrapper {
    fn event_type(&self) -> &'static str {
        match self {
            DomainEventWrapper::Account(event) => event.event_type(),
            DomainEventWrapper::Device(event) => event.event_type(),
            DomainEventWrapper::FileSync(event) => event.event_type(),
        }
    }
    
    fn timestamp(&self) -> i64 {
        match self {
            DomainEventWrapper::Account(event) => event.timestamp(),
            DomainEventWrapper::Device(event) => event.timestamp(),
            DomainEventWrapper::FileSync(event) => event.timestamp(),
        }
    }
    
    fn event_id(&self) -> String {
        match self {
            DomainEventWrapper::Account(event) => event.event_id(),
            DomainEventWrapper::Device(event) => event.event_id(),
            DomainEventWrapper::FileSync(event) => event.event_id(),
        }
    }
}

/// Event store interface for persisting domain events
#[async_trait::async_trait]
pub trait EventStore: Send + Sync {
    /// Save a domain event
    async fn save_event(&self, event: DomainEventWrapper) -> Result<()>;
    
    /// Get events for a specific aggregate
    async fn get_events_for_aggregate(&self, aggregate_id: &str) -> Result<Vec<DomainEventWrapper>>;
    
    /// Get events by type
    async fn get_events_by_type(&self, event_type: &str) -> Result<Vec<DomainEventWrapper>>;
    
    /// Get events within a time range
    async fn get_events_in_range(&self, from: i64, to: i64) -> Result<Vec<DomainEventWrapper>>;
    
    /// Get latest events with limit
    async fn get_latest_events(&self, limit: usize) -> Result<Vec<DomainEventWrapper>>;
}

/// Event publisher interface for publishing events to external systems
#[async_trait::async_trait]
pub trait EventPublisher: Send + Sync {
    /// Publish a domain event
    async fn publish(&self, event: DomainEventWrapper) -> Result<()>;
    
    /// Publish multiple events in batch
    async fn publish_batch(&self, events: Vec<DomainEventWrapper>) -> Result<()>;
}

/// Event handler registration system
pub struct EventHandlerRegistry {
    handlers: std::collections::HashMap<String, Vec<Box<dyn EventHandler>>>,
}

impl EventHandlerRegistry {
    /// Create a new event handler registry
    pub fn new() -> Self {
        Self {
            handlers: std::collections::HashMap::new(),
        }
    }
    
    /// Register a handler for a specific event type
    pub fn register_handler(&mut self, event_type: String, handler: Box<dyn EventHandler>) {
        self.handlers.entry(event_type).or_insert_with(Vec::new).push(handler);
    }
    
    /// Handle an event by calling all registered handlers
    pub async fn handle_event(&self, event: &DomainEventWrapper) -> Result<()> {
        let event_type = event.event_type();
        if let Some(handlers) = self.handlers.get(event_type) {
            for handler in handlers {
                handler.handle(event).await?;
            }
        }
        Ok(())
    }
}

/// Generic event handler trait
#[async_trait::async_trait]
pub trait EventHandler: Send + Sync {
    /// Handle a domain event
    async fn handle(&self, event: &DomainEventWrapper) -> Result<()>;
    
    /// Get the event types this handler can process
    fn event_types(&self) -> Vec<&'static str>;
    
    /// Get handler name for logging/debugging
    fn name(&self) -> &'static str;
}

/// Event sourcing projection interface
#[async_trait::async_trait]
pub trait EventProjection: Send + Sync {
    /// Apply an event to update the projection
    async fn apply_event(&mut self, event: &DomainEventWrapper) -> Result<()>;
    
    /// Get the current state of the projection
    async fn get_state(&self) -> Result<serde_json::Value>;
    
    /// Reset the projection to initial state
    async fn reset(&mut self) -> Result<()>;
    
    /// Get projection name
    fn name(&self) -> &'static str;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base_event_creation() {
        let event = BaseEvent::new("test.event".to_string(), "aggregate-123".to_string());
        
        assert_eq!(event.event_type, "test.event");
        assert_eq!(event.aggregate_id, "aggregate-123");
        assert_eq!(event.version, 1);
        assert!(!event.event_id.is_empty());
        assert!(event.timestamp > 0);
    }

    #[test]
    fn test_account_domain_event() {
        let base = BaseEvent::new("account.created".to_string(), "account-123".to_string());
        let event = AccountDomainEvent::AccountCreated {
            base,
            account_id: "account-123".to_string(),
            email: "test@example.com".to_string(),
            account_hash: "hash123".to_string(),
        };
        
        assert_eq!(event.event_type(), "account.created");
        assert!(!event.event_id().is_empty());
        assert!(event.timestamp() > 0);
    }

    #[test]
    fn test_domain_event_wrapper() {
        let base = BaseEvent::new("device.registered".to_string(), "device-123".to_string());
        let device_event = DeviceDomainEvent::DeviceRegistered {
            base,
            device_hash: "device-123".to_string(),
            account_hash: "account-123".to_string(),
            device_name: "Test Device".to_string(),
            device_type: "Desktop".to_string(),
        };
        
        let wrapper = DomainEventWrapper::Device(device_event);
        
        assert_eq!(wrapper.event_type(), "device.registered");
        assert!(!wrapper.event_id().is_empty());
        assert!(wrapper.timestamp() > 0);
    }
}
