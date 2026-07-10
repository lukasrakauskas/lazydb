use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf, time::Duration};

use crate::db::Connection;

/// ponytail: HOME is process-global; serialize tests that mutate it so parallel
/// `cargo test` threads don't race. Shared across `config::tests` and `app::tests`.
#[cfg(test)]
pub(crate) static HOME_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[derive(Default, Serialize, Deserialize)]
pub struct Config {
    pub connections: Vec<Connection>,
    // ponytail: #[serde(default)] so pre-existing config files without a
    // `[features]` table still load (back-compat).
    #[serde(default)]
    pub features: Features,
    // ponytail: query timeout / row cap are config-file settings (no UI yet);
    // both #[serde(default)] so older config files without them still load.
    #[serde(default)]
    pub query_timeout_secs: Option<u64>,
    #[serde(default)]
    pub select_limit: Option<usize>,
    /// Persistent query history (max 100 entries, newest last).
    #[serde(default)]
    pub history: Vec<String>,
    /// Last connected connection index, restored on next launch.
    #[serde(default)]
    pub last_connection: Option<usize>,
}

/// Togglable app features, persisted in the config file. Add a field + an
/// arm in get/set + an entry in LIST to add a toggle.
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Features {
    /// Render BLOB/binary columns as hex instead of raw (lossy) bytes.
    pub readable_binary: bool,
}

impl Features {
    /// `(name, description)` for each toggle, in modal order.
    pub const LIST: &'static [(&'static str, &'static str)] = &[(
        "Readable binary fields",
        "Render BLOB/binary columns as hex instead of raw bytes",
    )];

    pub fn get(&self, i: usize) -> bool {
        match i {
            0 => self.readable_binary,
            _ => false,
        }
    }

    pub fn set(&mut self, i: usize, v: bool) {
        if i == 0 {
            self.readable_binary = v
        }
    }
}

fn path() -> Result<PathBuf> {
    let home = std::env::var("HOME").map_err(|_| anyhow::anyhow!("HOME not set"))?;
    Ok(PathBuf::from(home).join(".config/lazydb/connections.toml"))
}

impl Config {
    pub fn load() -> Self {
        let Ok(p) = path() else {
            return Self::default();
        };
        fs::read_to_string(&p)
            .ok()
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// Socket read-timeout applied to every query on a connection.
    /// `None`/`0` = no timeout (wait forever). ponytail: a per-connection timeout
    /// would live on `Connection`; this is a global default from config.
    pub fn query_timeout(&self) -> Option<Duration> {
        self.query_timeout_secs
            .filter(|&s| s > 0)
            .map(Duration::from_secs)
    }
    pub fn save(&self) -> Result<()> {
        let p = path()?;
        if let Some(dir) = p.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(p, toml::to_string_pretty(self)?)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Connection;

    #[test]
    fn config_roundtrips() {
        let _lock = super::HOME_LOCK.lock().unwrap();
        let tmp = std::env::temp_dir().join(format!("lazydb-home-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let old = std::env::var_os("HOME");
        unsafe {
            std::env::set_var("HOME", &tmp);
        }

        let cfg = Config {
            connections: vec![Connection {
                name: "local".into(),
                kind: "mysql".into(),
                host: "127.0.0.1".into(),
                port: 3306,
                username: "root".into(),
                password: "p@ss".into(),
                database: "test".into(),
                ssl: false,
                use_keychain: false,
                ssh_enabled: false,
                ssh_host: String::new(),
                ssh_port: 22,
                ssh_user: String::new(),
                ssh_keyfile: String::new(),
                query_timeout_secs: None,
            }],
            features: Features {
                readable_binary: true,
            },
            query_timeout_secs: Some(30),
            select_limit: Some(1000),
            history: Vec::new(),
            last_connection: None,
        };
        cfg.save().unwrap();
        let loaded = Config::load();

        if let Some(o) = old {
            unsafe {
                std::env::set_var("HOME", o);
            }
        } else {
            unsafe {
                std::env::remove_var("HOME");
            }
        }
        std::fs::remove_dir_all(&tmp).ok();

        assert_eq!(loaded.connections.len(), 1);
        assert_eq!(loaded.connections[0].name, "local");
        assert_eq!(loaded.connections[0].password, "p@ss");
        assert_eq!(loaded.connections[0].port, 3306);
        assert!(loaded.features.readable_binary);
        assert_eq!(loaded.query_timeout_secs, Some(30));
        assert_eq!(loaded.select_limit, Some(1000));
    }

    // ponytail: old config files written before the `[features]` table existed
    // must still load (serde default).
    #[test]
    fn config_loads_without_features_table() {
        let _lock = super::HOME_LOCK.lock().unwrap();
        let tmp = std::env::temp_dir().join(format!("lazydb-home-nf-{}", std::process::id()));
        std::fs::create_dir_all(tmp.join(".config/lazydb")).unwrap();
        let old = std::env::var_os("HOME");
        unsafe {
            std::env::set_var("HOME", &tmp);
        }
        std::fs::write(
            tmp.join(".config/lazydb/connections.toml"),
            "connections = []\n",
        )
        .unwrap();
        let loaded = Config::load();
        if let Some(o) = old {
            unsafe {
                std::env::set_var("HOME", o);
            }
        } else {
            unsafe {
                std::env::remove_var("HOME");
            }
        }
        std::fs::remove_dir_all(&tmp).ok();
        assert!(!loaded.features.readable_binary);
    }
}
