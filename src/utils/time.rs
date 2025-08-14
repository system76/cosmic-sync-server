use chrono::{DateTime, Utc, TimeZone, Timelike};
use prost_types::Timestamp;
use serde::{Deserialize, Deserializer, Serializer};

/// Timestamp를 DateTime<Utc>로 변환
pub fn timestamp_to_datetime(ts: &Timestamp) -> DateTime<Utc> {
    DateTime::<Utc>::from_timestamp(ts.seconds, ts.nanos as u32)
        .unwrap_or_else(|| Utc::now())
}

/// DateTime<Utc>를 Timestamp로 변환
pub fn datetime_to_timestamp(dt: &DateTime<Utc>) -> Timestamp {
    Timestamp {
        seconds: dt.timestamp(),
        nanos: dt.nanosecond() as i32,
    }
}

/// DateTime<Utc>를 MySQL 형식 문자열로 변환 ('YYYY-MM-DD HH:MM:SS')
pub fn datetime_to_mysql_string(dt: &DateTime<Utc>) -> String {
    dt.format("%Y-%m-%d %H:%M:%S").to_string()
}

/// Timestamp 직렬화/역직렬화 모듈
pub mod timestamp_serde {
    use super::*;
    use serde::{de};
    
    pub fn serialize<S>(timestamp: &Timestamp, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let dt = Utc.timestamp_opt(timestamp.seconds, timestamp.nanos as u32)
            .single()
            .unwrap_or_else(|| Utc::now());
        serializer.serialize_str(&dt.to_rfc3339())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Timestamp, D::Error>
    where
        D: Deserializer<'de>,
    {
        let str_val = String::deserialize(deserializer)?;
        
        let dt = DateTime::parse_from_rfc3339(&str_val)
            .map_err(|e| de::Error::custom(format!("Invalid timestamp format: {}", e)))?;
        
        let ts = Timestamp {
            seconds: dt.timestamp(),
            nanos: dt.timestamp_subsec_nanos() as i32,
        };
        
        Ok(ts)
    }
}