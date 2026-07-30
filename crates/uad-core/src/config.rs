use crate::CACHE_DIR;
use crate::CONFIG_DIR;
use crate::sync::User;
use crate::uad_lists::{PackageState, Removal, UadList};
use crate::utils::DisplayablePath;
use log::error;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::LazyLock;

const DEFAULT_THEME: &str = "Auto (follow system theme)";

#[derive(Default, Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    // NOTE: this scalar must be serialized before the `general`/`devices` tables
    // — TOML requires top-level keys to precede any table headers, so keep it
    // as the first field.
    /// Serial (`adb_id`) of the most recently selected device, re-selected on
    /// the next launch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_device_id: Option<String>,
    pub general: GeneralSettings,
    #[serde(default)]
    pub filters: FilterSettings,
    #[serde(skip_serializing_if = "Vec::is_empty", default = "Vec::new")]
    pub devices: Vec<DeviceSettings>,
}

/// Package-list filters, remembered across launches so the app reopens on the
/// view the user left it in.
///
/// The field defaults deliberately match what the apps view used to hard-code
/// (all lists / enabled / recommended), so a config without this table behaves
/// exactly as before. The search box is intentionally *not* remembered — it is
/// a transient lookup, not a view preference.
#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(default)]
pub struct FilterSettings {
    pub list: UadList,
    pub state: PackageState,
    pub removal: Removal,
    pub user_apps_only: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GeneralSettings {
    pub theme: String,
    pub expert_mode: bool,
    pub backup_folder: PathBuf,
}

#[derive(Default, Debug, Clone)]
pub struct BackupSettings {
    pub backups: Vec<DisplayablePath>,
    pub selected: Option<DisplayablePath>,
    pub users: Vec<User>,
    pub selected_user: Option<User>,
    pub backup_state: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct DeviceSettings {
    /// Unique serial identifier
    pub device_id: String,
    pub disable_mode: bool,
    pub multi_user_mode: bool,
    #[serde(skip)]
    pub backup: BackupSettings,
}

impl Default for GeneralSettings {
    fn default() -> Self {
        Self {
            theme: DEFAULT_THEME.to_string(),
            expert_mode: false,
            backup_folder: CACHE_DIR.join("backups"),
        }
    }
}

static CONFIG_FILE: LazyLock<PathBuf> = LazyLock::new(|| CONFIG_DIR.join("config.toml"));

impl Config {
    pub fn save_device_settings(
        &mut self,
        device_settings: DeviceSettings,
        general: GeneralSettings,
    ) {
        if let Some(device) = self
            .devices
            .iter_mut()
            .find(|x| x.device_id == device_settings.device_id)
        {
            *device = device_settings;
        } else {
            self.devices.push(device_settings);
        }
        self.general = general;
        let toml = toml::to_string(&self).unwrap();
        fs::write(&*CONFIG_FILE, toml).expect("Could not write config file to disk!");
    }

    /// Best-effort write of the whole config. Failures are logged, never fatal:
    /// losing a remembered preference must not take the app down.
    fn write(&self) {
        match toml::to_string(self) {
            Ok(toml) => {
                if let Err(e) = fs::write(&*CONFIG_FILE, toml) {
                    error!("Could not write config file to disk: {e}");
                }
            }
            Err(e) => error!("Could not serialize config: {e}"),
        }
    }

    /// Persist the most recently selected device serial (`adb_id`), so it is
    /// re-selected automatically on the next launch.
    pub fn save_last_device(device_id: &str) {
        let mut config = Self::load_configuration_file();
        if config.last_device_id.as_deref() == Some(device_id) {
            return; // already current — avoid a needless disk write
        }
        config.last_device_id = Some(device_id.to_string());
        config.write();
    }

    /// The most recently selected device serial (`adb_id`), if any.
    #[must_use]
    pub fn last_device_id() -> Option<String> {
        Self::load_configuration_file().last_device_id
    }

    /// Persist the apps-view filters. Re-reads from disk first so this can't
    /// clobber settings changed elsewhere in the meantime.
    pub fn save_filters(filters: &FilterSettings) {
        let mut config = Self::load_configuration_file();
        if config.filters == *filters {
            return; // unchanged — avoid a needless disk write
        }
        config.filters.clone_from(filters);
        config.write();
    }

    /// The filters to open the apps view with.
    #[must_use]
    pub fn filters() -> FilterSettings {
        Self::load_configuration_file().filters
    }

    #[must_use]
    pub fn load_configuration_file() -> Self {
        match fs::read_to_string(&*CONFIG_FILE) {
            Ok(s) => match toml::from_str(&s) {
                Ok(config) => return config,
                Err(e) => error!("Invalid config file: `{e}`"),
            },
            Err(e) => error!("Failed to read config file: `{e}`"),
        }
        error!("Restoring default config file");
        let toml = toml::to_string(&Self::default()).unwrap();
        fs::write(&*CONFIG_FILE, toml).expect("Could not write config file to disk!");
        Self::default()
    }
}

#[cfg(test)]
mod tests {
    //! Unit tests

    use super::*;
    use std::path::Path;

    // create a clean default config file for testing
    fn create_default_config_file() {
        let toml = toml::to_string(&Config::default()).unwrap();
        fs::write(&*CONFIG_FILE, toml).expect("Could not write config file to disk!");
    }

    #[test]
    fn test_create_default_config_file() {
        create_default_config_file();
        assert!(CONFIG_FILE.exists());
    }

    #[test]
    fn test_load_configuration_file() {
        create_default_config_file();
        let config = Config::load_configuration_file();
        // non-deterministic
        //assert_eq!(config.devices.len(), 0);
        assert_eq!(config.general.theme, DEFAULT_THEME);
        assert!(!config.general.expert_mode);
        assert_eq!(config.general.backup_folder, CACHE_DIR.join("backups"));
    }

    // non-deterministic
    /*
    #[test]
    fn test_save_changes() {
        let mut settings = Settings::default();
        let device_id = "test_device".to_string();
        settings.device.device_id = device_id.clone();
        Config::save_changes(&settings, &device_id);
        let config = Config::load_configuration_file();
        assert_eq!(config.devices[0].device_id, device_id);
    }
    */

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.devices.len(), 0);
        assert_eq!(config.general.theme, DEFAULT_THEME);
        assert!(!config.general.expert_mode);
        assert_eq!(config.general.backup_folder, CACHE_DIR.join("backups"));
    }

    #[test]
    fn test_config_file_path() {
        assert_eq!(&*CONFIG_FILE, Path::new(&*CONFIG_DIR.join("config.toml")));
    }

    // Guards the TOML gotcha: `last_device_id` (a scalar) must serialize before
    // the `[general]` / `[[devices]]` tables, else the emitted TOML is invalid.
    #[test]
    fn test_last_device_roundtrip() {
        let config = Config {
            last_device_id: Some("ABC123SERIAL".to_string()),
            devices: vec![DeviceSettings {
                device_id: "ABC123SERIAL".to_string(),
                ..DeviceSettings::default()
            }],
            ..Config::default()
        };
        let toml = toml::to_string(&config).expect("serialize");
        // The scalar key must appear before any table header.
        let key_pos = toml.find("last_device_id").expect("key present");
        let table_pos = toml.find('[').expect("a table exists");
        assert!(key_pos < table_pos, "scalar must precede tables:\n{toml}");
        let parsed: Config = toml::from_str(&toml).expect("deserialize");
        assert_eq!(parsed.last_device_id.as_deref(), Some("ABC123SERIAL"));
    }

    // In-memory only: these must not touch the real config file.
    #[test]
    fn test_filters_round_trip() {
        let config = Config {
            filters: FilterSettings {
                list: UadList::Google,
                state: PackageState::All,
                removal: Removal::Advanced,
                user_apps_only: true,
            },
            devices: vec![DeviceSettings::default()],
            ..Config::default()
        };
        let toml = toml::to_string(&config).expect("serialize");
        let parsed: Config = toml::from_str(&toml).expect("deserialize");
        assert_eq!(parsed.filters, config.filters);
    }

    // A config written by an older build has no `[filters]` table; it must load
    // with the same defaults the apps view used to hard-code.
    #[test]
    fn test_config_without_filters_table() {
        let old = r#"
            [general]
            theme = "Auto (follow system theme)"
            expert_mode = false
            backup_folder = "/tmp/backups"
        "#;
        let parsed: Config = toml::from_str(old).expect("deserialize");
        assert_eq!(parsed.filters.list, UadList::All);
        assert_eq!(parsed.filters.state, PackageState::Enabled);
        assert_eq!(parsed.filters.removal, Removal::Recommended);
        assert!(!parsed.filters.user_apps_only);
    }
}
