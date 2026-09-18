use async_trait::async_trait;
use chrono::Utc;
use easyjob_common::{Error, Result};
use easyjob_domain::SystemSettings;
use sqlx::{Row, SqlitePool};

#[async_trait]
pub trait SettingsRepository: Send + Sync {
    async fn get_system_settings(&self) -> Result<SystemSettings>;
    async fn save_system_settings(&self, settings: &SystemSettings) -> Result<()>;
}

pub struct SqliteSettingsRepository {
    pool: SqlitePool,
}

impl SqliteSettingsRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl SettingsRepository for SqliteSettingsRepository {
    async fn get_system_settings(&self) -> Result<SystemSettings> {
        let row = sqlx::query("SELECT value_json FROM settings WHERE key = 'system_settings'")
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

        match row {
            Some(r) => {
                let json_str: String = r.get("value_json");
                let settings: SystemSettings = serde_json::from_str(&json_str)?;
                Ok(settings)
            }
            None => Ok(SystemSettings::default()),
        }
    }

    async fn save_system_settings(&self, settings: &SystemSettings) -> Result<()> {
        let json_str = serde_json::to_string(settings)?;
        let now = Utc::now().to_rfc3339();

        sqlx::query(
            "INSERT INTO settings (key, value_json, updated_at)
             VALUES ('system_settings', ?, ?)
             ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json, updated_at = excluded.updated_at",
        )
        .bind(json_str)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;

        Ok(())
    }
}
