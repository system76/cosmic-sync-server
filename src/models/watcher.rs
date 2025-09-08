use crate::sync;
use crate::utils::time::timestamp_serde;
use crate::utils::time::{datetime_to_timestamp, timestamp_to_datetime};
use chrono::{DateTime, Utc};
use prost_types::Timestamp;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// watcher group info (DB model)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WatcherGroup {
    pub id: i32,       // Primary Key (server generated)
    pub group_id: i32, // client group ID
    pub account_hash: String,
    pub title: String,             // group title
    pub created_at: DateTime<Utc>, // creation time
    pub updated_at: DateTime<Utc>, // last updated time
    pub is_active: bool,           // active status
    pub watcher_ids: Vec<i32>,     // watcher ID list
}

/// watcher condition info (DB model)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WatcherCondition {
    pub id: Option<i64>,               // Primary Key (None for new records)
    pub account_hash: String, // account hash (required for security and user identification)
    pub watcher_id: i32,      // FK to Watcher (server DB ID)
    pub local_watcher_id: i32, // client-side watcher ID (for sync)
    pub local_group_id: i32,  // client-side group ID
    pub condition_type: ConditionType, // union or subtract
    pub key: String,          // condition key (e.g., "extension", "filename")
    pub value: Vec<String>,   // condition values as array (e.g., ["json", "yaml"])
    pub operator: String,     // operator (default: "equals")
    pub created_at: DateTime<Utc>, // creation time
    pub updated_at: DateTime<Utc>, // last updated time
}

/// condition type enum
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConditionType {
    Union,
    Subtract,
}

impl std::fmt::Display for ConditionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConditionType::Union => write!(f, "union"),
            ConditionType::Subtract => write!(f, "subtract"),
        }
    }
}

impl std::str::FromStr for ConditionType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "union" => Ok(ConditionType::Union),
            "subtract" => Ok(ConditionType::Subtract),
            _ => Err(format!("Invalid condition type: {}", s)),
        }
    }
}

/// individual watcher info (DB model)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Watcher {
    pub id: i32,         // DB PK (server generated)
    pub watcher_id: i32, // client watcher ID
    pub account_hash: String,
    pub group_id: i32,       // FK to WatcherGroup
    pub local_group_id: i32, // client group ID
    pub title: String,
    pub folder: String,
    pub union_conditions: Vec<Condition>,
    pub subtracting_conditions: Vec<Condition>,
    pub recursive_path: bool,
    pub preset: bool,
    pub custom_type: String,
    pub update_mode: String,
    pub is_active: bool,           // Whether the watcher is active
    pub extra_json: String,        // Additional JSON data for watcher configuration
    pub created_at: DateTime<Utc>, // creation time
    pub updated_at: DateTime<Utc>, // last updated time
}

/// watcher condition info (same as proto structure)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Condition {
    pub key: String,
    pub value: Vec<String>,
}

/// protobuf compatible watcher group data (for proto conversion)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatcherGroupData {
    pub group_id: i32,
    pub title: String,
    pub watchers: Vec<WatcherData>,
    #[serde(with = "timestamp_serde")]
    pub last_updated: Timestamp,
}

/// protobuf compatible watcher data (for proto conversion)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatcherData {
    pub watcher_id: i32,
    pub folder: String,
    pub union_conditions: Vec<ConditionData>,
    pub subtracting_conditions: Vec<ConditionData>,
    pub recursive_path: bool,
    pub preset: bool,
    pub custom_type: String,
    pub update_mode: String,
    pub is_active: bool,
    pub extra_json: String,
}

/// protobuf compatible condition data (for proto conversion)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConditionData {
    pub key: String,
    pub value: Vec<String>,
}

impl From<&WatcherGroup> for WatcherGroupData {
    fn from(group: &WatcherGroup) -> Self {
        // DB model -> proto conversion structure
        Self {
            group_id: group.group_id, // return group_id to client
            title: group.title.clone(),
            watchers: Vec::new(), // DB has no watchers, need to query separately
            last_updated: datetime_to_timestamp(&group.updated_at),
        }
    }
}

/// Convert from sync proto WatcherGroupData to model WatcherGroupData
impl From<&sync::WatcherGroupData> for WatcherGroupData {
    fn from(proto: &sync::WatcherGroupData) -> Self {
        Self {
            group_id: proto.group_id,
            title: proto.title.clone(),
            watchers: proto
                .watchers
                .iter()
                .map(|w| WatcherData::from(w))
                .collect(),
            last_updated: proto.last_updated.clone().unwrap_or_else(|| Timestamp {
                seconds: 0,
                nanos: 0,
            }),
        }
    }
}

/// Convert from model WatcherGroupData to sync proto WatcherGroupData
impl From<WatcherGroupData> for sync::WatcherGroupData {
    fn from(model: WatcherGroupData) -> Self {
        Self {
            group_id: model.group_id,
            title: model.title,
            watchers: model
                .watchers
                .into_iter()
                .map(|w| sync::WatcherData::from(&w))
                .collect(),
            last_updated: Some(model.last_updated),
        }
    }
}

pub struct WatcherDirectory {
    pub path: PathBuf,
    pub recursive_path: bool,
}

// DB model -> proto message conversion function (based on sync::WatcherGroupData created by prost)
impl From<&WatcherGroup> for sync::WatcherGroupData {
    fn from(group: &WatcherGroup) -> Self {
        sync::WatcherGroupData {
            group_id: group.group_id, // return group_id to client
            title: group.title.clone(),
            watchers: vec![], // DB has no watchers, need to query separately
            last_updated: Some(datetime_to_timestamp(&group.updated_at)),
        }
    }
}

// WatcherGroupData -> WatcherGroup creation logic
impl WatcherGroupData {
    // create WatcherGroup from proto message data
    pub fn create_watcher_group(&self, account_hash: String) -> WatcherGroup {
        let updated_at = timestamp_to_datetime(&self.last_updated);

        WatcherGroup {
            id: 0,                   // server generated
            group_id: self.group_id, // client group ID
            account_hash,
            title: self.title.clone(),
            created_at: updated_at, // creation time is set to the same as update time
            updated_at,
            is_active: true,
            watcher_ids: Vec::new(), // need to set separately
        }
    }
}

impl From<&Condition> for sync::ConditionData {
    fn from(cond: &Condition) -> Self {
        sync::ConditionData {
            key: cond.key.clone(),
            value: cond.value.clone(),
        }
    }
}

impl From<&sync::ConditionData> for Condition {
    fn from(proto: &sync::ConditionData) -> Self {
        Self {
            key: proto.key.clone(),
            value: proto.value.clone(),
        }
    }
}

/// Represents a watcher preset
#[derive(Debug, Clone)]
pub struct WatcherPreset {
    /// Preset ID
    pub id: i32,
    /// Preset name
    pub title: String,
    /// Preset settings (JSON)
    pub presets: Vec<String>,
    /// Creation timestamp
    pub created_at: i64,
    /// Last updated timestamp
    pub updated_at: i64,
}

impl From<&sync::WatcherData> for WatcherData {
    fn from(proto: &sync::WatcherData) -> Self {
        Self {
            watcher_id: proto.watcher_id,
            folder: proto.folder.clone(),
            union_conditions: proto
                .union_conditions
                .iter()
                .map(|c| ConditionData {
                    key: c.key.clone(),
                    value: c.value.clone(),
                })
                .collect(),
            subtracting_conditions: proto
                .subtracting_conditions
                .iter()
                .map(|c| ConditionData {
                    key: c.key.clone(),
                    value: c.value.clone(),
                })
                .collect(),
            recursive_path: proto.recursive_path,
            preset: proto.preset,
            custom_type: proto.custom_type.clone(),
            update_mode: proto.update_mode.clone(),
            is_active: proto.is_active,
            extra_json: proto.extra_json.clone(),
        }
    }
}

impl From<sync::WatcherData> for WatcherData {
    fn from(proto: sync::WatcherData) -> Self {
        Self {
            watcher_id: proto.watcher_id,
            folder: proto.folder,
            union_conditions: proto
                .union_conditions
                .into_iter()
                .map(|c| ConditionData {
                    key: c.key,
                    value: c.value,
                })
                .collect(),
            subtracting_conditions: proto
                .subtracting_conditions
                .into_iter()
                .map(|c| ConditionData {
                    key: c.key,
                    value: c.value,
                })
                .collect(),
            recursive_path: proto.recursive_path,
            preset: proto.preset,
            custom_type: proto.custom_type,
            update_mode: proto.update_mode,
            is_active: proto.is_active,
            extra_json: proto.extra_json,
        }
    }
}

/// Convert from Watcher model to WatcherData for client response
impl From<&Watcher> for WatcherData {
    fn from(watcher: &Watcher) -> Self {
        Self {
            watcher_id: watcher.watcher_id, // return watcher_id to client
            folder: watcher.folder.clone(),
            union_conditions: watcher
                .union_conditions
                .iter()
                .map(|c| ConditionData {
                    key: c.key.clone(),
                    value: c.value.clone(),
                })
                .collect(),
            subtracting_conditions: watcher
                .subtracting_conditions
                .iter()
                .map(|c| ConditionData {
                    key: c.key.clone(),
                    value: c.value.clone(),
                })
                .collect(),
            recursive_path: watcher.recursive_path,
            preset: watcher.preset,
            custom_type: watcher.custom_type.clone(),
            update_mode: watcher.update_mode.clone(),
            is_active: watcher.is_active,
            extra_json: watcher.extra_json.clone(),
        }
    }
}

/// Convert from Watcher model to sync::WatcherData for proto response
impl From<&Watcher> for sync::WatcherData {
    fn from(watcher: &Watcher) -> Self {
        Self {
            watcher_id: watcher.watcher_id, // return watcher_id to client
            folder: watcher.folder.clone(),
            union_conditions: watcher
                .union_conditions
                .iter()
                .map(|c| sync::ConditionData {
                    key: c.key.clone(),
                    value: c.value.clone(),
                })
                .collect(),
            subtracting_conditions: watcher
                .subtracting_conditions
                .iter()
                .map(|c| sync::ConditionData {
                    key: c.key.clone(),
                    value: c.value.clone(),
                })
                .collect(),
            recursive_path: watcher.recursive_path,
            preset: watcher.preset,
            custom_type: watcher.custom_type.clone(),
            update_mode: watcher.update_mode.clone(),
            is_active: watcher.is_active,
            extra_json: watcher.extra_json.clone(),
        }
    }
}

// watcher group info data
#[derive(Debug, Clone)]
pub struct WatcherGroupInfo {
    pub id: i32,
    pub name: String,
    pub watchers: Vec<WatcherInfo>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// watcher detail info
#[derive(Debug, Clone)]
pub struct WatcherInfo {
    pub id: i32,
    pub group_id: i32,
    pub title: String,
    pub folder: String,
    pub recursive_path: bool,
    pub custom_type: String,
    pub update_mode: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// convert from proto WatcherGroupData to WatcherGroup
impl From<sync::WatcherGroupData> for WatcherGroup {
    fn from(proto: sync::WatcherGroupData) -> Self {
        let updated_at = if let Some(ts) = proto.last_updated {
            timestamp_to_datetime(&ts)
        } else {
            Utc::now()
        };

        Self {
            id: 0,                        // server generated
            group_id: proto.group_id,     // client group ID
            account_hash: "".to_string(), // this data is not in proto, need to set in client
            title: "".to_string(),
            created_at: Utc::now(),
            updated_at,
            is_active: true,
            watcher_ids: Vec::new(),
        }
    }
}

// convert from proto RegisterWatcherGroupRequest to WatcherGroup
impl From<sync::RegisterWatcherGroupRequest> for WatcherGroup {
    fn from(req: sync::RegisterWatcherGroupRequest) -> Self {
        let now = Utc::now();

        Self {
            id: 0,                  // server generated
            group_id: req.group_id, // client group ID
            account_hash: req.account_hash,
            title: req.title,
            created_at: now,
            updated_at: now,
            is_active: true,
            watcher_ids: Vec::new(),
        }
    }
}

impl From<WatcherData> for Watcher {
    fn from(data: WatcherData) -> Self {
        Self {
            id: 0,                        // server generated
            watcher_id: data.watcher_id,  // client watcher ID
            account_hash: "".to_string(), // this data is not in proto, need to set in client
            group_id: 0,                  // this data is not in proto, need to set in client
            local_group_id: 0,            // client group ID (for sync)
            title: "".to_string(),        // this data is not in proto, need to set in client
            folder: data.folder,
            union_conditions: data
                .union_conditions
                .into_iter()
                .map(|c| Condition {
                    key: c.key,
                    value: c.value,
                })
                .collect(),
            subtracting_conditions: data
                .subtracting_conditions
                .into_iter()
                .map(|c| Condition {
                    key: c.key,
                    value: c.value,
                })
                .collect(),
            recursive_path: data.recursive_path,
            preset: data.preset,
            custom_type: data.custom_type,
            update_mode: data.update_mode,
            is_active: data.is_active,
            extra_json: data.extra_json,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }
}

// convert from sync::ConditionData to ConditionData
impl From<&sync::ConditionData> for ConditionData {
    fn from(proto: &sync::ConditionData) -> Self {
        Self {
            key: proto.key.clone(),
            value: proto.value.clone(),
        }
    }
}

impl From<&ConditionData> for sync::ConditionData {
    fn from(data: &ConditionData) -> Self {
        Self {
            key: data.key.clone(),
            value: data.value.clone(),
        }
    }
}

impl From<sync::ConditionData> for ConditionData {
    fn from(proto: sync::ConditionData) -> Self {
        Self {
            key: proto.key,
            value: proto.value,
        }
    }
}

impl From<ConditionData> for sync::ConditionData {
    fn from(data: ConditionData) -> Self {
        Self {
            key: data.key,
            value: data.value,
        }
    }
}

impl From<&WatcherData> for sync::WatcherData {
    fn from(data: &WatcherData) -> Self {
        Self {
            watcher_id: data.watcher_id,
            folder: data.folder.clone(),
            union_conditions: data
                .union_conditions
                .iter()
                .map(|c| sync::ConditionData {
                    key: c.key.clone(),
                    value: c.value.clone(),
                })
                .collect(),
            subtracting_conditions: data
                .subtracting_conditions
                .iter()
                .map(|c| sync::ConditionData {
                    key: c.key.clone(),
                    value: c.value.clone(),
                })
                .collect(),
            recursive_path: data.recursive_path,
            preset: data.preset,
            custom_type: data.custom_type.clone(),
            update_mode: data.update_mode.clone(),
            is_active: data.is_active,
            extra_json: data.extra_json.clone(),
        }
    }
}
