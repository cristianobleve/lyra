use crate::auth::SpotifyTokens;
use crate::config::AppConfig;
use anyhow::{Context, Result};
use directories::ProjectDirs;
use std::fs;
use std::path::PathBuf;

pub struct CredentialsStorage {
    config_dir: PathBuf,
}

impl CredentialsStorage {
    pub fn new() -> Self {
        let config_dir = if let Some(proj_dirs) = ProjectDirs::from("com", "spotialt", "Spotialt") {
            proj_dirs.config_dir().to_path_buf()
        } else {
            PathBuf::from(".spotialt")
        };
        let _ = fs::create_dir_all(&config_dir);
        Self { config_dir }
    }

    pub fn tokens_path(&self) -> PathBuf {
        self.config_dir.join("tokens.json")
    }

    pub fn config_path(&self) -> PathBuf {
        self.config_dir.join("config.json")
    }

    pub fn save_tokens(&self, tokens: &SpotifyTokens) -> Result<()> {
        let json = serde_json::to_string_pretty(tokens)?;
        fs::write(self.tokens_path(), json).context("Failed to write tokens to disk")?;
        Ok(())
    }

    pub fn load_tokens(&self) -> Option<SpotifyTokens> {
        let data = fs::read_to_string(self.tokens_path()).ok()?;
        serde_json::from_str(&data).ok()
    }

    pub fn delete_tokens(&self) {
        let _ = fs::remove_file(self.tokens_path());
    }

    pub fn save_config(&self, config: &AppConfig) -> Result<()> {
        let json = serde_json::to_string_pretty(config)?;
        fs::write(self.config_path(), json).context("Failed to write config to disk")?;
        Ok(())
    }

    pub fn load_config(&self) -> AppConfig {
        if let Ok(data) = fs::read_to_string(self.config_path()) {
            if let Ok(cfg) = serde_json::from_str(&data) {
                return cfg;
            }
        }
        AppConfig::default()
    }
}
