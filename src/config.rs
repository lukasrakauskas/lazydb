use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

use crate::db::Connection;

#[derive(Default, Serialize, Deserialize)]
pub struct Config {
    pub connections: Vec<Connection>,
    // ponytail: #[serde(default)] so pre-existing config files without a
    // `[features]` table still load (back-compat).
    #[serde(default)]
    pub features: Features,
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
    pub const LIST: &'static [(&'static str, &'static str)] = &[
        ("Readable binary fields", "Render BLOB/binary columns as hex instead of raw bytes"),
    ];

    pub fn get(&self, i: usize) -> bool {
        match i {
            0 => self.readable_binary,
            _ => false,
        }
    }

    pub fn set(&mut self, i: usize, v: bool) {
        match i {
            0 => self.readable_binary = v,
            _ => {}
        }
    }
}

fn path() -> Result<PathBuf> {
    let home = std::env::var("HOME").map_err(|_| anyhow::anyhow!("HOME not set"))?;
    Ok(PathBuf::from(home).join(".config/lazydb/connections.toml"))
}

impl Config {
    pub fn load() -> Self {
        let Ok(p) = path() else { return Self::default() };
        fs::read_to_string(&p)
            .ok()
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default()
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
    use std::sync::Mutex;

    // ponytail: HOME is process-global; serialize the tests that mutate it
    // so parallel `cargo test` threads don't race.
    static HOME_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn config_roundtrips() {
        let _lock = HOME_LOCK.lock().unwrap();
        let tmp = std::env::temp_dir().join(format!("lazydb-home-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let old = std::env::var_os("HOME");
        unsafe { std::env::set_var("HOME", &tmp); }

        let cfg = Config {
            connections: vec![Connection {
                name: "local".into(),
                kind: "mysql".into(),
                host: "127.0.0.1".into(),
                port: 3306,
                username: "root".into(),
                password: "p@ss".into(),
                database: "test".into(),
            }],
            features: Features { readable_binary: true },
        };
        cfg.save().unwrap();
        let loaded = Config::load();

        if let Some(o) = old {
            unsafe { std::env::set_var("HOME", o); }
        } else {
            unsafe { std::env::remove_var("HOME"); }
        }
        std::fs::remove_dir_all(&tmp).ok();

        assert_eq!(loaded.connections.len(), 1);
        assert_eq!(loaded.connections[0].name, "local");
        assert_eq!(loaded.connections[0].password, "p@ss");
        assert_eq!(loaded.connections[0].port, 3306);
        assert_eq!(loaded.features.readable_binary, true);
    }

    // ponytail: old config files written before the `[features]` table existed
    // must still load (serde default).
    #[test]
    fn config_loads_without_features_table() {
        let _lock = HOME_LOCK.lock().unwrap();
        let tmp = std::env::temp_dir().join(format!("lazydb-home-nf-{}", std::process::id()));
        std::fs::create_dir_all(tmp.join(".config/lazydb")).unwrap();
        let old = std::env::var_os("HOME");
        unsafe { std::env::set_var("HOME", &tmp); }
        std::fs::write(
            tmp.join(".config/lazydb/connections.toml"),
            "connections = []\n",
        ).unwrap();
        let loaded = Config::load();
        if let Some(o) = old { unsafe { std::env::set_var("HOME", o); } }
        else { unsafe { std::env::remove_var("HOME"); } }
        std::fs::remove_dir_all(&tmp).ok();
        assert_eq!(loaded.features.readable_binary, false);
    }
}