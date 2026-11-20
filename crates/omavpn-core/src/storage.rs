use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::models::{AppSettings, Profile, Subscription};

#[derive(Debug, Clone)]
pub struct Storage {
    base_dir: PathBuf,
}

impl Storage {
    pub fn open_default() -> Result<Self> {
        let dir = dirs::config_dir()
            .context("could not determine user config directory")?
            .join("omavpn");
        Self::open_at(dir)
    }

    pub fn open_at(base_dir: PathBuf) -> Result<Self> {
        fs::create_dir_all(&base_dir)
            .with_context(|| format!("failed to create config dir {}", base_dir.display()))?;
        Ok(Self { base_dir })
    }

    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    pub fn load_profiles(&self) -> Result<Vec<Profile>> {
        self.read_json_or_default("profiles.json")
    }

    pub fn save_profiles(&self, profiles: &[Profile]) -> Result<()> {
        self.write_json_atomic("profiles.json", profiles)
    }

    pub fn load_subscriptions(&self) -> Result<Vec<Subscription>> {
        self.read_json_or_default("subscriptions.json")
    }

    pub fn save_subscriptions(&self, subs: &[Subscription]) -> Result<()> {
        self.write_json_atomic("subscriptions.json", subs)
    }

    pub fn load_settings(&self) -> Result<AppSettings> {
        self.read_json_or_default("settings.json")
    }

    pub fn save_settings(&self, settings: &AppSettings) -> Result<()> {
        self.write_json_atomic("settings.json", settings)
    }

    fn read_json_or_default<T: DeserializeOwned + Default>(&self, filename: &str) -> Result<T> {
        let path = self.base_dir.join(filename);
        if !path.exists() {
            return Ok(T::default());
        }
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        if raw.trim().is_empty() {
            return Ok(T::default());
        }
        serde_json::from_str(&raw)
            .with_context(|| format!("failed to parse JSON in {}", path.display()))
    }

    fn write_json_atomic<T: Serialize + ?Sized>(&self, filename: &str, value: &T) -> Result<()> {
        let path = self.base_dir.join(filename);
        let tmp_path = self.base_dir.join(format!("{filename}.tmp"));
        let content = serde_json::to_string_pretty(value)?;
        fs::write(&tmp_path, content)
            .with_context(|| format!("failed to write {}", tmp_path.display()))?;
        fs::rename(&tmp_path, &path).with_context(|| {
            format!(
                "failed to replace {} with {}",
                path.display(),
                tmp_path.display()
            )
        })?;
        Ok(())
    }
}
