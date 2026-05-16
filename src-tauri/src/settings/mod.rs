#![allow(missing_docs)] // Method-level docs cover the API surface; `Settings` struct is a 1-field wrapper.

//! Typed read/write facade over the `settings` table.
//!
//! See [`model::SettingKey`] for the registry of known keys. Values
//! are stored as JSON-encoded TEXT so any shape round-trips.

pub mod model;

use rusqlite::{params, Connection, OptionalExtension};
use serde::{de::DeserializeOwned, Serialize};

use crate::error::{AppError, AppResult};
use model::SettingKey;

pub struct Settings<'a> {
    conn: &'a Connection,
}

impl<'a> Settings<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    /// Get a typed setting. Falls back to the key's default if the
    /// row is missing OR the stored value won't deserialize as `T`.
    pub fn get<T: DeserializeOwned>(&self, key: SettingKey) -> AppResult<T> {
        let raw = self.get_raw(key)?;
        serde_json::from_value(raw)
            .map_err(|e| AppError::Other(format!("deserialize {}: {e}", key.as_str())))
    }

    /// Get the raw JSON value. Returns the key's default if absent OR
    /// if the stored TEXT isn't parseable as JSON (which would mean
    /// the row got corrupted out-of-band; we log a warning and
    /// recover).
    pub fn get_raw(&self, key: SettingKey) -> AppResult<serde_json::Value> {
        let stored: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                params![key.as_str()],
                |r| r.get(0),
            )
            .optional()?;
        let Some(s) = stored else {
            return Ok(key.default_value());
        };
        match serde_json::from_str(&s) {
            Ok(v) => Ok(v),
            Err(e) => {
                tracing::warn!(
                    key = key.as_str(),
                    error = %e,
                    "corrupt setting value; using default"
                );
                Ok(key.default_value())
            }
        }
    }

    pub fn set<T: Serialize>(&self, key: SettingKey, value: &T) -> AppResult<()> {
        let json = serde_json::to_value(value)
            .map_err(|e| AppError::Other(format!("serialize {}: {e}", key.as_str())))?;
        self.set_raw(key, &json)
    }

    pub fn set_raw(&self, key: SettingKey, value: &serde_json::Value) -> AppResult<()> {
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key.as_str(), value.to_string()],
        )?;
        Ok(())
    }

    pub fn reset_to_default(&self, key: SettingKey) -> AppResult<()> {
        self.conn
            .execute("DELETE FROM settings WHERE key = ?1", params![key.as_str()])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    fn fresh() -> Database {
        Database::open_in_memory().unwrap()
    }

    #[test]
    fn get_returns_default_when_unset() {
        let db = fresh();
        let s = Settings::new(&db.conn);
        let v: bool = s.get(SettingKey::AutostartEnabled).unwrap();
        assert!(!v);
        let theme: String = s.get(SettingKey::Theme).unwrap();
        assert_eq!(theme, "system");
    }

    #[test]
    fn set_then_get_round_trips_bool() {
        let db = fresh();
        let s = Settings::new(&db.conn);
        s.set(SettingKey::AutostartEnabled, &true).unwrap();
        let v: bool = s.get(SettingKey::AutostartEnabled).unwrap();
        assert!(v);
    }

    #[test]
    fn set_then_get_round_trips_string() {
        let db = fresh();
        let s = Settings::new(&db.conn);
        s.set(SettingKey::Theme, &"dark").unwrap();
        let v: String = s.get(SettingKey::Theme).unwrap();
        assert_eq!(v, "dark");
    }

    #[test]
    fn set_then_get_round_trips_int() {
        let db = fresh();
        let s = Settings::new(&db.conn);
        s.set(SettingKey::AudioRetentionDays, &14).unwrap();
        let v: i64 = s.get(SettingKey::AudioRetentionDays).unwrap();
        assert_eq!(v, 14);
    }

    #[test]
    fn set_overwrites_via_upsert() {
        let db = fresh();
        let s = Settings::new(&db.conn);
        s.set(SettingKey::Theme, &"dark").unwrap();
        s.set(SettingKey::Theme, &"light").unwrap();
        let v: String = s.get(SettingKey::Theme).unwrap();
        assert_eq!(v, "light");
        // Only one row in the table for this key.
        let n: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM settings WHERE key = 'theme'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn reset_to_default_removes_row() {
        let db = fresh();
        let s = Settings::new(&db.conn);
        s.set(SettingKey::Theme, &"dark").unwrap();
        s.reset_to_default(SettingKey::Theme).unwrap();
        let v: String = s.get(SettingKey::Theme).unwrap();
        assert_eq!(v, "system");
    }

    #[test]
    fn get_with_wrong_type_errors_cleanly() {
        let db = fresh();
        let s = Settings::new(&db.conn);
        s.set(SettingKey::Theme, &42_i64).unwrap();
        let err = s.get::<String>(SettingKey::Theme).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("theme"), "error should mention key: {msg}");
    }

    #[test]
    fn corrupt_stored_value_falls_back_to_default() {
        let db = fresh();
        // Raw INSERT non-JSON garbage to simulate out-of-band corruption.
        db.conn
            .execute(
                "INSERT INTO settings (key, value) VALUES ('theme', 'not-json-{')",
                [],
            )
            .unwrap();
        let s = Settings::new(&db.conn);
        let v: String = s.get(SettingKey::Theme).unwrap();
        assert_eq!(v, "system", "should fall back to default on parse failure");
    }

    #[test]
    fn raw_round_trip_null() {
        let db = fresh();
        let s = Settings::new(&db.conn);
        let v = s.get_raw(SettingKey::ClaudeApiKeyRef).unwrap();
        assert!(v.is_null());
    }
}
