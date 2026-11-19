use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs, io,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScaleMode {
    #[default]
    Fit,
    PixelPerfect,
    Stretch,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SecurityPolicy {
    /// Initial milestone; permitted only for loopback without explicit user confirmation.
    #[default]
    LocalhostOnly,
    /// Opt-in for trusted networks or SSH/WireGuard tunnel. Encryption NOT provided by RFB.
    AllowLegacyUnencrypted,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum KeyPreset {
    #[default]
    MacToLinux,
    MacToWindows,
    Identity,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: Option<String>,
    pub auto_reconnect: bool,
    pub fullscreen_on_connect: bool,
    pub scale_mode: ScaleMode,
    pub key_preset: KeyPreset,
    pub security: SecurityPolicy,
}
impl Default for Profile {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            host: String::new(),
            port: 5900,
            username: None,
            auto_reconnect: true,
            fullscreen_on_connect: true,
            scale_mode: ScaleMode::Fit,
            key_preset: KeyPreset::MacToLinux,
            security: SecurityPolicy::LocalhostOnly,
        }
    }
}
impl Profile {
    pub fn validate(&self) -> Result<(), String> {
        if self.id.is_empty()
            || !self
                .id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-')
        {
            return Err("invalid profile id".into());
        }
        if self.name.trim().is_empty() || self.host.trim().is_empty() {
            return Err("name and host required".into());
        }
        if self.port == 0 {
            return Err("port must be greater than zero".into());
        }
        if self.host.chars().any(|c| matches!(c, '\n' | '\r' | '\0')) {
            return Err("host contains forbidden characters".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub theme: String,
    pub language: String,
    pub default_fullscreen: bool,
    pub reconnect_max_attempts: u32,
    pub telemetry: bool,
    /// Scroll-wheel multiplier for the remote desktop (0.25..=4.0, 1.0 = standard).
    pub scroll_speed: f32,
    /// Sync the system clipboard with the remote desktop (text only, both ways).
    pub clipboard_sync: bool,
}
impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme: "dark".into(),
            language: "ja".into(),
            default_fullscreen: true,
            reconnect_max_attempts: 0,
            telemetry: false,
            scroll_speed: 1.0,
            // SECURITY.md / PRODUCT_SPEC call clipboard sharing opt-in; respect that.
            clipboard_sync: false,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Store {
    pub schema_version: u32,
    pub settings: AppSettings,
    pub profiles: Vec<Profile>,
}
impl Store {
    /// Validate the whole store (shared by GUI and CLI via `save`). Duplicate
    /// names are dangerous because `get_profile` matches id OR name: two profiles
    /// named "Home" would make `connect Home` ambiguous and could contact the
    /// wrong host.
    pub fn validate(&self) -> Result<(), String> {
        let mut ids = HashSet::new();
        let mut names = HashSet::new();
        for p in &self.profiles {
            p.validate()?;
            if !ids.insert(p.id.as_str()) {
                return Err(format!("duplicate profile id: {}", p.id));
            }
            let key = p.name.trim().to_lowercase();
            if !names.insert(key) {
                return Err(format!("duplicate profile name: {}", p.name));
            }
        }
        Ok(())
    }
}

/// Map a validation message to an i18n key so the Japanese UI never shows
/// raw English validation errors from the shared core.
pub fn validation_i18n_key(message: &str) -> &'static str {
    if message.starts_with("invalid profile id") {
        "error.invalid_id"
    } else if message.starts_with("name and host required") {
        "error.name_host_required"
    } else if message.starts_with("port must") {
        "error.port"
    } else if message.starts_with("host contains") {
        "error.host_chars"
    } else if message.starts_with("duplicate profile name") {
        "error.duplicate_name"
    } else if message.starts_with("duplicate profile id") {
        "error.duplicate_id"
    } else {
        "error.unknown"
    }
}

pub fn data_path() -> io::Result<PathBuf> {
    if let Some(override_path) = std::env::var_os("GLINTVNC_DATA_DIR") {
        return Ok(PathBuf::from(override_path).join("config.json"));
    }
    let home = std::env::var_os("HOME")
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME missing"))?;
    Ok(PathBuf::from(home).join("Library/Application Support/GlintVNC/config.json"))
}

pub fn load(path: &Path) -> io::Result<Store> {
    if !path.exists() {
        // Attempt recovery from the last-known-good backup if the main file vanished.
        let bak = backup_path(path);
        if bak.exists() {
            if let Ok(store) = load_backup(&bak) {
                return Ok(store);
            }
        }
        return Ok(Store {
            schema_version: 1,
            ..Store::default()
        });
    }
    let bytes = fs::read(path)?;
    match serde_json::from_slice::<Store>(&bytes) {
        Ok(store) => {
            if store.schema_version > 1 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "config created by a newer app",
                ));
            }
            Ok(store)
        }
        Err(primary_err) => {
            // Corruption recovery: fall back to the .bak written before the previous save.
            let bak = backup_path(path);
            if let Ok(store) = load_backup(&bak) {
                eprintln!(
                    "glintvnc: config.json was unreadable ({primary_err}); recovered from backup"
                );
                return Ok(store);
            }
            Err(io::Error::new(io::ErrorKind::InvalidData, primary_err))
        }
    }
}

fn backup_path(path: &Path) -> PathBuf {
    let mut p = path.as_os_str().to_os_string();
    p.push(".bak");
    PathBuf::from(p)
}

fn load_backup(bak: &Path) -> io::Result<Store> {
    let bytes = fs::read(bak)?;
    let store: Store = serde_json::from_slice(&bytes)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    if store.schema_version > 1 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "backup created by a newer app",
        ));
    }
    Ok(store)
}

pub fn save(path: &Path, store: &Store) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut clean = store.clone();
    clean.schema_version = 1;
    clean
        .validate()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    // Profile and settings must NEVER contain password/token/secret fields.
    let bytes = serde_json::to_vec_pretty(&clean).map_err(io::Error::other)?;
    // Best-effort backup of the previous good config before replacing it.
    if path.exists() {
        let _ = fs::copy(path, backup_path(path));
    }
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let tmp = path.with_extension(format!("tmp-{}-{unique}", std::process::id()));
    let result = (|| {
        use std::io::Write;
        let mut opt = fs::OpenOptions::new();
        opt.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opt.mode(0o600);
        }
        let mut f = opt.open(&tmp)?;
        f.write_all(&bytes)?;
        f.sync_all()?;
        fs::rename(&tmp, path)?;
        Ok::<(), io::Error>(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

pub fn get_profile<'a>(store: &'a Store, selector: &str) -> Option<&'a Profile> {
    store
        .profiles
        .iter()
        .find(|p| p.id == selector || p.name == selector)
}

pub fn make_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!(
        "p{:x}-{:x}-{:x}",
        now,
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scroll_speed_defaults_to_one_for_legacy_configs() {
        // Config written before scroll_speed existed must still load.
        let legacy = br#"{"schema_version":1,"settings":{"theme":"dark","language":"ja","default_fullscreen":true,"reconnect_max_attempts":0,"telemetry":false},"profiles":[]}"#;
        let store: Store = serde_json::from_slice(legacy).unwrap();
        assert_eq!(store.settings.scroll_speed, 1.0);
        assert!(
            !store.settings.clipboard_sync,
            "clipboard sync must be opt-in"
        );
    }
    #[test]
    fn scroll_speed_round_trip() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.json");
        let mut s = Store::default();
        s.settings.scroll_speed = 2.5;
        save(&path, &s).unwrap();
        assert_eq!(load(&path).unwrap().settings.scroll_speed, 2.5);
    }
    #[test]
    fn secrets_cannot_be_serialized_from_struct() {
        let s = Store {
            schema_version: 1,
            settings: AppSettings::default(),
            profiles: vec![Profile {
                id: "p1".into(),
                name: "demo".into(),
                host: "127.0.0.1".into(),
                ..Profile::default()
            }],
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(!json.contains("password"));
        assert!(!json.contains("secret"));
    }
    #[test]
    fn atomic_store_round_trip() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.json");
        let mut s = Store::default();
        s.profiles.push(Profile {
            id: "p1".into(),
            name: "Mac".into(),
            host: "localhost".into(),
            ..Profile::default()
        });
        save(&path, &s).unwrap();
        let got = load(&path).unwrap();
        assert_eq!(got.profiles[0].name, "Mac");
    }
    #[test]
    fn corrupt_config_recovers_from_backup() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.json");
        let mut s = Store::default();
        s.profiles.push(Profile {
            id: "p1".into(),
            name: "First".into(),
            host: "localhost".into(),
            ..Profile::default()
        });
        save(&path, &s).unwrap();
        s.profiles[0].name = "Second".into();
        save(&path, &s).unwrap(); // .bak now holds "First"
        fs::write(&path, b"{ not json").unwrap();
        let got = load(&path).unwrap();
        assert_eq!(got.profiles[0].name, "First");
    }
    #[test]
    fn missing_config_recovers_from_backup() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.json");
        let mut s = Store::default();
        s.profiles.push(Profile {
            id: "p1".into(),
            name: "Saved".into(),
            host: "localhost".into(),
            ..Profile::default()
        });
        save(&path, &s).unwrap();
        fs::remove_file(&path).unwrap();
        // no .bak yet (only one save) -> defaults
        assert!(load(&path).unwrap().profiles.is_empty());
        save(&path, &s).unwrap();
        save(&path, &s).unwrap(); // second save creates .bak
        fs::remove_file(&path).unwrap();
        assert_eq!(load(&path).unwrap().profiles[0].name, "Saved");
    }
    #[test]
    fn duplicate_profile_names_rejected() {
        let mut s = Store::default();
        for id in ["a", "b"] {
            s.profiles.push(Profile {
                id: id.into(),
                name: "Home".into(),
                host: "127.0.0.1".into(),
                ..Profile::default()
            });
        }
        assert!(s.validate().is_err());
        assert_eq!(
            validation_i18n_key(&s.validate().unwrap_err()),
            "error.duplicate_name"
        );
        let temp = tempfile::tempdir().unwrap();
        assert!(save(&temp.path().join("config.json"), &s).is_err());
    }
    #[test]
    fn validation_errors_map_to_i18n_keys() {
        assert_eq!(
            validation_i18n_key("invalid profile id"),
            "error.invalid_id"
        );
        assert_eq!(
            validation_i18n_key("name and host required"),
            "error.name_host_required"
        );
        assert_eq!(
            validation_i18n_key("port must be greater than zero"),
            "error.port"
        );
        assert_eq!(validation_i18n_key("unexpected"), "error.unknown");
    }
    #[test]
    fn reject_bad_host() {
        let p = Profile {
            id: "x".into(),
            name: "x".into(),
            host: "bad\nhost".into(),
            ..Profile::default()
        };
        assert!(p.validate().is_err());
    }
}
