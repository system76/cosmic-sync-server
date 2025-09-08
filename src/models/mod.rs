pub mod account;
pub mod auth;
pub mod device;
pub mod file;
pub mod watcher;

// re-export types from parent modules
pub use account::{Account, SimpleAuthToken};
pub use auth::AuthToken;
pub use device::{Device, DeviceInfo};
pub use file::{FileData, FileInfo, SyncFile};
pub use watcher::{
    Condition, ConditionData, ConditionType, Watcher, WatcherCondition as WatcherConditionEnum,
    WatcherCondition, WatcherData, WatcherDirectory, WatcherGroup, WatcherGroupData,
    WatcherGroupInfo, WatcherInfo, WatcherPreset,
}; // use AuthToken from auth.rs

// timestamp related utility functions - move to utils::time
pub use crate::utils::time::{datetime_to_timestamp, timestamp_to_datetime};

pub use device::*;
pub use file::*;
pub use watcher::*;
