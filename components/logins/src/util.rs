/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use crate::error::*;
use rusqlite::Row;
use std::{time, fmt};
use url::Url;
use serde::de::{self, Deserialize, Deserializer, Visitor};
use serde::ser::{Serialize, Serializer};
use std::str::FromStr;
use std::time::Duration;

pub fn url_host_port(url_str: &str) -> Option<String> {
    let url = Url::parse(url_str).ok()?;
    let host = url.host_str()?;
    Some(if let Some(p) = url.port() {
        format!("{}:{}", host, p)
    } else {
        host.to_string()
    })
}

pub fn system_time_millis_from_row(row: &Row<'_>, col_name: &str) -> Result<time::SystemTime> {
    let time_ms = row.get::<_, Option<i64>>(col_name)?.unwrap_or_default() as u64;
    Ok(time::UNIX_EPOCH + time::Duration::from_millis(time_ms))
}

pub fn duration_ms_i64(d: time::Duration) -> i64 {
    (d.as_secs() as i64) * 1000 + (i64::from(d.subsec_nanos()) / 1_000_000)
}

pub fn system_time_ms_i64(t: time::SystemTime) -> i64 {
    duration_ms_i64(t.duration_since(time::UNIX_EPOCH).unwrap_or_default())
}

// Unfortunately, there's not a better way to turn on logging in tests AFAICT
#[cfg(test)]
pub(crate) fn init_test_logging() {
    use std::sync::{Once, ONCE_INIT};
    static INIT_LOGGING: Once = ONCE_INIT;
    INIT_LOGGING.call_once(|| {
        env_logger::init_from_env(env_logger::Env::default().filter_or("RUST_LOG", "trace"));
    });
}

// XXX DONOTLAND COPIED FROM sync15::util

/// Typesafe way to manage server timestamps without accidentally mixing them up with
/// local ones.
#[derive(Debug, Copy, Clone, PartialEq, PartialOrd, Default)]
pub struct ServerTimestamp(pub i64);

impl From<ServerTimestamp> for i64 {
    #[inline]
    fn from(ts: ServerTimestamp) -> Self {
        ts.0
    }
}

impl From<ServerTimestamp> for f64 {
    #[inline]
    fn from(ts: ServerTimestamp) -> Self {
        ts.0 as f64 / 1000.0
    }
}

impl From<i64> for ServerTimestamp {
    #[inline]
    fn from(ts: i64) -> Self {
        ServerTimestamp(ts)
    }
}

impl From<f64> for ServerTimestamp {
    fn from(ts: f64) -> Self {
        let rf = (ts * 1000.0).round();
        if !rf.is_finite() || rf < 0.0 || rf >= i64::max_value() as f64 {
            log::error!("Illegal timestamp: {}", ts);
            ServerTimestamp(0)
        } else {
            ServerTimestamp(rf as i64)
        }
    }
}

// This lets us use these in hyper header! blocks.
impl std::str::FromStr for ServerTimestamp {
    type Err = std::num::ParseFloatError;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        let val = f64::from_str(s)?;
        Ok(val.into())
    }
}

impl std::fmt::Display for ServerTimestamp {
    #[inline]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0 as f64 / 1000.0)
    }
}

pub const SERVER_EPOCH: ServerTimestamp = ServerTimestamp(0);

impl ServerTimestamp {
    /// Returns None if `other` is later than `self` (Duration may not represent
    /// negative timespans in rust).
    #[inline]
    pub fn duration_since(self, other: ServerTimestamp) -> Option<std::time::Duration> {
        let delta = self.0 - other.0;
        if delta < 0 {
            None
        } else {
            Some(std::time::Duration::from_millis(delta as u64))
        }
    }

    /// Get the milliseconds for the timestamp.
    #[inline]
    pub fn as_millis(self) -> i64 {
        self.0
    }
}

impl Serialize for ServerTimestamp {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_f64(self.0 as f64 / 1000.0)
    }
}

impl<'de> Deserialize<'de> for ServerTimestamp {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct TimestampVisitor;

        impl<'de> Visitor<'de> for TimestampVisitor {
            type Value = ServerTimestamp;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("A 64 bit float number value.")
            }

            fn visit_f64<E>(self, value: f64) -> std::result::Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(value.into())
            }
        }

        deserializer.deserialize_f64(TimestampVisitor)
    }
}
