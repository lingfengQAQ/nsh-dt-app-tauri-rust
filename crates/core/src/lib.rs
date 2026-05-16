use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};
use url::Url;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O error at {path}: {source}")]
    Io { path: PathBuf, source: io::Error },
    #[error("failed to serialize or deserialize JSON at {path}: {source}")]
    Json {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("invalid URL `{value}`: {source}")]
    InvalidUrl {
        value: String,
        source: url::ParseError,
    },
    #[error("application data directory could not be resolved")]
    DataDirUnavailable,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub settings: Settings,
    #[serde(default)]
    pub ai: AIConfig,
    #[serde(default = "default_secondary_ai_config")]
    pub ai_secondary: AIConfig,
    #[serde(default)]
    pub baidu_ocr: BaiduOcrConfig,
    #[serde(default)]
    pub history: HistoryConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            settings: Settings::default(),
            ai: AIConfig::default(),
            ai_secondary: default_secondary_ai_config(),
            baidu_ocr: BaiduOcrConfig::default(),
            history: HistoryConfig::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct HistoryConfig {
    #[serde(default)]
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default = "default_auto_save")]
    pub auto_save: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: default_language(),
            theme: default_theme(),
            auto_save: default_auto_save(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AIConfig {
    #[serde(default = "default_ai_provider")]
    pub provider: String,
    #[serde(default = "default_ai_model")]
    pub model: String,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: Option<u32>,
    #[serde(default = "default_temperature")]
    pub temperature: Option<f32>,
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
    #[serde(default)]
    pub use_primary_endpoint: bool,
}

impl Default for AIConfig {
    fn default() -> Self {
        Self {
            provider: default_ai_provider(),
            model: default_ai_model(),
            base_url: None,
            api_key: None,
            max_tokens: default_max_tokens(),
            temperature: default_temperature(),
            timeout_secs: default_timeout_secs(),
            use_primary_endpoint: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct BaiduOcrConfig {
    #[serde(default)]
    pub app_id: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub secret_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    pub app_dir: PathBuf,
    pub data_dir: PathBuf,
    pub config_file: PathBuf,
}

impl AppPaths {
    pub fn resolve(
        app_dir: Option<impl AsRef<Path>>,
        data_dir: Option<impl AsRef<Path>>,
    ) -> Result<Self> {
        let app_dir = app_dir
            .map(|path| path.as_ref().to_path_buf())
            .unwrap_or_else(default_app_dir);
        let portable_data_dir = app_dir.join("data");
        let data_dir = if portable_data_dir.exists() {
            portable_data_dir
        } else if let Some(data_dir) = data_dir {
            data_dir.as_ref().to_path_buf()
        } else {
            default_data_dir()?
        };
        let config_file = data_dir.join("config.json");

        Ok(Self {
            app_dir,
            data_dir,
            config_file,
        })
    }
}

pub fn default_app_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn default_data_dir() -> Result<PathBuf> {
    ProjectDirs::from("cn", "nsh", "nsh-dt-app")
        .map(|dirs| dirs.data_local_dir().to_path_buf())
        .ok_or(Error::DataDirUnavailable)
}

pub fn read_config(path: impl AsRef<Path>) -> Result<AppConfig> {
    let path = path.as_ref();
    if !path.exists() {
        return Ok(AppConfig::default());
    }

    let content = fs::read_to_string(path).map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })?;

    serde_json::from_str(&content).map_err(|source| Error::Json {
        path: path.to_path_buf(),
        source,
    })
}

pub fn write_config(path: impl AsRef<Path>, config: &AppConfig) -> Result<()> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| Error::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }

    let content = serde_json::to_string_pretty(config).map_err(|source| Error::Json {
        path: path.to_path_buf(),
        source,
    })?;
    fs::write(path, content).map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })
}

pub fn append_history_line(path: impl AsRef<Path>, line: &str) -> Result<()> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| Error::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }

    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|source| Error::Io {
            path: path.to_path_buf(),
            source,
        })?;

    writeln!(file, "{line}").map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })
}

pub fn parse_url(value: impl AsRef<str>) -> Result<Url> {
    let value = value.as_ref();
    Url::parse(value).map_err(|source| Error::InvalidUrl {
        value: value.to_string(),
        source,
    })
}

pub fn redact_secret(value: impl AsRef<str>) -> String {
    let value = value.as_ref();
    if value.is_empty() {
        return String::new();
    }

    let total = value.chars().count();
    if total <= 8 {
        return "***".to_string();
    }

    let prefix: String = value.chars().take(4).collect();
    let suffix: String = value
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("{prefix}***{suffix}")
}

pub fn redact_optional_secret(value: Option<&str>) -> Option<String> {
    value.map(redact_secret)
}

fn default_language() -> String {
    "zh-CN".to_string()
}

fn default_theme() -> String {
    "system".to_string()
}

fn default_auto_save() -> bool {
    true
}

fn default_ai_provider() -> String {
    "openai".to_string()
}

fn default_ai_model() -> String {
    "gpt-5.4-mini".to_string()
}

fn default_max_tokens() -> Option<u32> {
    Some(2048)
}

fn default_temperature() -> Option<f32> {
    Some(0.7)
}

fn default_timeout_secs() -> u64 {
    10
}

fn default_secondary_ai_config() -> AIConfig {
    AIConfig {
        provider: "deepseek".to_string(),
        model: "deepseek-v4-flash".to_string(),
        base_url: None,
        api_key: None,
        max_tokens: default_max_tokens(),
        temperature: default_temperature(),
        timeout_secs: default_timeout_secs(),
        use_primary_endpoint: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn app_config_defaults_are_stable() {
        let config = AppConfig::default();

        assert_eq!(config.settings.language, "zh-CN");
        assert_eq!(config.settings.theme, "system");
        assert!(config.settings.auto_save);
        assert_eq!(config.ai.provider, "openai");
        assert_eq!(config.ai.model, "gpt-5.4-mini");
        assert_eq!(config.ai.max_tokens, Some(2048));
        assert_eq!(config.ai.temperature, Some(0.7));
        assert_eq!(config.ai.timeout_secs, 10);
        assert!(!config.ai.use_primary_endpoint);
        assert_eq!(config.ai_secondary.provider, "deepseek");
        assert_eq!(config.ai_secondary.model, "deepseek-v4-flash");
        assert!(!config.ai_secondary.use_primary_endpoint);
        assert_eq!(config.baidu_ocr, BaiduOcrConfig::default());
    }

    #[test]
    fn redact_secret_keeps_only_edges() {
        assert_eq!(redact_secret(""), "");
        assert_eq!(redact_secret("short"), "***");
        assert_eq!(redact_secret("sk-1234567890"), "sk-1***7890");
    }

    #[test]
    fn portable_data_dir_wins_over_explicit_data_dir() {
        let root = unique_temp_dir("nsh-core-paths");
        let app_dir = root.join("app");
        let explicit_data_dir = root.join("explicit-data");
        let portable_data_dir = app_dir.join("data");
        fs::create_dir_all(&portable_data_dir).unwrap();

        let paths = AppPaths::resolve(Some(&app_dir), Some(&explicit_data_dir)).unwrap();

        assert_eq!(paths.data_dir, portable_data_dir);
        assert_eq!(paths.config_file, paths.data_dir.join("config.json"));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn config_roundtrip() {
        let root = unique_temp_dir("nsh-core-config");
        let file = root.join("nested").join("config.json");
        let mut config = AppConfig::default();
        config.ai.api_key = Some("sk-test".to_string());

        write_config(&file, &config).unwrap();
        let restored = read_config(&file).unwrap();

        assert_eq!(restored, config);
        fs::remove_dir_all(root).unwrap();
    }

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{nanos}"))
    }
}
