use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

use crate::db::Connection;

#[derive(Default, Serialize, Deserialize)]
pub struct Config {
    pub connections: Vec<Connection>,
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

    #[test]
    fn config_roundtrips() {
        // ponytail: env HOME mutation is not thread-safe; this is the only test
        // touching HOME, so parallel test runs are safe.
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
    }
}