#![allow(clippy::unsafe_derive_deserialize)]

use std::env;
use std::fs;
use std::path::PathBuf;

secretspec_derive::declare_secrets!("../../secretspec.toml");
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SettingsError {
    #[error("unknown argument {0}")]
    UnknownArgument(String),
    #[error("{flag} requires a value")]
    MissingValue { flag: String },
    #[error(
        "missing runtime setting {0}; provide it at build time or with its CLI/environment override"
    )]
    MissingCompiledSetting(String),
    #[error("SecretSpec resolution failed: {0}")]
    SecretSpec(#[from] secretspec::SecretSpecError),
    #[error("could not read API key file {path}: {source}")]
    ApiKeyFile {
        path: String,
        source: std::io::Error,
    },
    #[error("API key file {0} was empty")]
    EmptyApiKey(String),
    #[error("invalid boolean value for {name}: {value}")]
    InvalidBoolean { name: String, value: String },
}

#[derive(Debug)]
pub struct Settings {
    pub graph_config: Option<String>,
    pub bindings_config: String,
    pub installation_path: PathBuf,
    pub api_key: Option<String>,
    pub base_url: String,
    pub user_agent: String,
    pub device_description: String,
    pub dry_run: bool,
}

const DEFAULT_SECRETSPEC_PROVIDER: &str = match option_env!("DEFAULT_SECRETSPEC_PROVIDER") {
    Some(provider) => provider,
    None => "env://",
};

impl Settings {
    /// Reads runtime configuration with CLI values taking precedence over env.
    ///
    /// # Errors
    ///
    /// Returns an error for unsupported arguments or missing required paths.
    pub fn load() -> Result<Self, SettingsError> {
        let mut graph_config = env::var_os("BUNQ_GRAPH");
        let mut bindings_config = env::var_os("BUNQ_BINDINGS");
        let mut api_key_file = env::var_os("BUNQ_API_KEY_FILE");
        let mut installation = env::var_os("PATH_TO_BUNQ_INSTALLATION");
        let mut base_url = env::var_os("BUNQ_BASE_URL");
        let mut user_agent = env::var_os("BUNQ_USER_AGENT");
        let mut device_description = env::var_os("BUNQ_DEVICE_DESCRIPTION");
        let mut secretspec_provider = env::var_os("SECRETSPEC_PROVIDER");
        let mut dry_run = env::var_os("BUNQ_DRY_RUN")
            .map(|value| parse_bool("BUNQ_DRY_RUN", &value.to_string_lossy()))
            .transpose()?;

        let mut arguments = env::args_os().skip(1);

        while let Some(argument) = arguments.next() {
            let flag = argument.to_string_lossy().into_owned();
            if flag == "--dry-run" {
                dry_run = Some(true);
                continue;
            }
            let value = arguments
                .next()
                .ok_or_else(|| SettingsError::MissingValue { flag: flag.clone() })?;
            match flag.as_str() {
                "--graph" => graph_config = Some(value),
                "--bindings" => bindings_config = Some(value),
                "--api-key-file" => api_key_file = Some(value),
                "--installation" => installation = Some(value),
                "--base-url" => base_url = Some(value),
                "--user-agent" => user_agent = Some(value),
                "--device-description" => device_description = Some(value),
                "--secretspec-provider" => secretspec_provider = Some(value),
                _ => return Err(SettingsError::UnknownArgument(flag)),
            }
        }

        let resolved = (bindings_config.is_none() || api_key_file.is_none())
            .then(|| {
                let provider = secretspec_provider.as_ref().map_or_else(
                    || DEFAULT_SECRETSPEC_PROVIDER.to_owned(),
                    |value| value.to_string_lossy().into_owned(),
                );
                SecretSpec::builder()
                    .with_provider(provider)
                    .with_reason("run bunq-graph")
                    .load()
            })
            .transpose()?;

        let bindings_config = bindings_config
            .map(|value| value.to_string_lossy().into_owned())
            .or_else(|| {
                resolved
                    .as_ref()
                    .map(|value| value.secrets.bunq_bindings.clone())
            })
            .ok_or_else(|| SettingsError::MissingCompiledSetting("BUNQ_BINDINGS".to_owned()))?;

        let installation_path = installation.map_or_else(default_installation_path, PathBuf::from);

        let api_key = api_key_file
            .map(|path| {
                let display = path.to_string_lossy().into_owned();
                fs::read_to_string(&path)
                    .map_err(|source| SettingsError::ApiKeyFile {
                        path: display.clone(),
                        source,
                    })
                    .and_then(|key| {
                        let key = key.trim().to_owned();
                        if key.is_empty() {
                            Err(SettingsError::EmptyApiKey(display))
                        } else {
                            Ok(key)
                        }
                    })
            })
            .transpose()?
            .or_else(|| {
                resolved
                    .as_ref()
                    .and_then(|value| value.secrets.bunq_api_key.clone())
            });

        Ok(Self {
            graph_config: graph_config.map(|value| value.to_string_lossy().into_owned()),
            bindings_config,
            installation_path,
            api_key,
            base_url: base_url.map_or_else(
                || "https://api.bunq.com/v1".to_owned(),
                |value| value.to_string_lossy().into_owned(),
            ),
            user_agent: user_agent.map_or_else(
                || "bunq-graph/0.1".to_owned(),
                |value| value.to_string_lossy().into_owned(),
            ),
            device_description: device_description.map_or_else(
                || "bunq-graph".to_owned(),
                |value| value.to_string_lossy().into_owned(),
            ),
            dry_run: dry_run.unwrap_or(false),
        })
    }
}

fn parse_bool(name: &str, value: &str) -> Result<bool, SettingsError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => Err(SettingsError::InvalidBoolean {
            name: name.to_owned(),
            value: value.to_owned(),
        }),
    }
}

fn default_installation_path() -> PathBuf {
    if let Some(state_home) = env::var_os("XDG_STATE_HOME") {
        return PathBuf::from(state_home)
            .join("bunq-graph")
            .join("installation.json");
    }
    if let Some(home) = env::var_os("HOME") {
        #[cfg(target_os = "macos")]
        return PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("bunq-graph")
            .join("installation.json");
        #[cfg(not(target_os = "macos"))]
        return PathBuf::from(home)
            .join(".local")
            .join("state")
            .join("bunq-graph")
            .join("installation.json");
    }
    PathBuf::from("bunq-graph-installation.json")
}
