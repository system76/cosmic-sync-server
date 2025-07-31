pub mod account;
pub mod device;
pub mod file;
pub mod watcher;
pub mod auth;

// re-export types from parent modules
pub use account::{Account, SimpleAuthToken};
pub use device::{Device, DeviceInfo};
pub use file::{FileInfo, SyncFile, FileData};
pub use watcher::{
    WatcherGroup, Watcher, WatcherGroupData, WatcherData, WatcherPreset,
    WatcherGroupInfo, WatcherInfo, WatcherDirectory, WatcherCondition as WatcherConditionEnum,
    Condition, ConditionData, WatcherCondition, ConditionType
};
pub use auth::AuthToken;  // use AuthToken from auth.rs

// timestamp related utility functions - move to utils::time
pub use crate::utils::time::{timestamp_to_datetime, datetime_to_timestamp};

pub use device::*;
pub use file::*;
pub use watcher::*;