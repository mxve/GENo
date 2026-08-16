use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub poller: PollerConfig,

    #[serde(default)]
    pub db: DbConfig,
    #[serde(default)]
    pub notifications: NotificationsConfig,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PollerConfig {
    #[serde(default = "default_interval_secs")]
    pub interval_secs: u64,
    #[serde(default = "default_user_agent")]
    pub user_agent: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbConfig {
    #[serde(default = "default_db_path")]
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationsConfig {
    #[serde(default = "default_cooldown_secs")]
    pub default_cooldown_secs: u64,
}

fn default_host() -> String {
    "127.0.0.1".to_string()
}

fn default_port() -> u16 {
    8080
}

fn default_interval_secs() -> u64 {
    60
}

fn default_user_agent() -> String {
    "ge-notifier/0.1 (github.com/mxve/ge-notifier)".to_string()
}

fn default_db_path() -> String {
    "ge-notifier.db".to_string()
}

fn default_cooldown_secs() -> u64 {
    3600
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
        }
    }
}

impl Default for PollerConfig {
    fn default() -> Self {
        Self {
            interval_secs: default_interval_secs(),
            user_agent: default_user_agent(),
        }
    }
}

impl Default for DbConfig {
    fn default() -> Self {
        Self {
            path: default_db_path(),
        }
    }
}

impl Default for NotificationsConfig {
    fn default() -> Self {
        Self {
            default_cooldown_secs: default_cooldown_secs(),
        }
    }
}

impl Config {
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let path = path.as_ref();
        let mut config = if path.exists() {
            let content = fs::read_to_string(path)?;
            toml::from_str::<Config>(&content)?
        } else {
            let default_cfg = Config::default();
            if let Ok(toml_str) = toml::to_string_pretty(&default_cfg) {
                let _ = fs::write(path, toml_str);
            }
            default_cfg
        };

        // wiki api asks for at least 30s between polls
        if config.poller.interval_secs < 30 {
            tracing::warn!(
                "Configured poll interval ({}s) is below 30s minimum. Setting to 30s.",
                config.poller.interval_secs
            );
            config.poller.interval_secs = 30;
        }

        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let cfg = Config::default();
        assert_eq!(cfg.server.host, "127.0.0.1");
        assert_eq!(cfg.server.port, 8080);
        assert_eq!(cfg.poller.interval_secs, 60);
        assert_eq!(cfg.notifications.default_cooldown_secs, 3600);
    }

    #[test]
    fn test_parse_custom_config() {
        let toml_str = r#"
            [server]
            host = "0.0.0.0"
            port = 9000

            [poller]
            interval_secs = 120
            user_agent = "custom-agent/1.0"


            [db]
            path = "test.db"

            [notifications]
            default_cooldown_secs = 1800
        "#;
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.server.host, "0.0.0.0");
        assert_eq!(cfg.server.port, 9000);
        assert_eq!(cfg.poller.interval_secs, 120);
        assert_eq!(cfg.poller.user_agent, "custom-agent/1.0");
        assert_eq!(cfg.db.path, "test.db");
        assert_eq!(cfg.notifications.default_cooldown_secs, 1800);
    }
}
