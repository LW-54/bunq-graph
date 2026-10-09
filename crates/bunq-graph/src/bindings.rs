use std::fs;
use std::path::Path;

use schemars::JsonSchema;
use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(description = "Account-name-to-IBAN bindings for a bunq-graph configuration.")]
pub struct Bindings {
    #[schemars(description = "One binding for each account named by the graph.")]
    pub account: Vec<Binding>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[schemars(description = "A Clepsydra account name mapped to a bunq IBAN.")]
pub struct Binding {
    #[schemars(description = "Account name used by the Clepsydra graph.")]
    pub name: String,
    #[schemars(description = "IBAN used to identify the bunq account or counterparty.")]
    pub iban: String,
    #[serde(default)]
    #[schemars(description = "Whether this binding represents an external account.")]
    pub external: bool,
}

#[derive(Debug, Error)]
pub enum BindingError {
    #[error("could not read binding file {path}: {source}")]
    Read {
        path: String,
        source: std::io::Error,
    },
    #[error("binding input is not a regular file: {0}")]
    NotAFile(String),
    #[error("unsupported binding file format for {path}")]
    UnsupportedFormat { path: String },
    #[error("could not parse binding input {label}: {message}")]
    Parse { label: String, message: String },
    #[error("binding name {0} appears more than once")]
    DuplicateName(String),
    #[error("binding IBAN {0} appears more than once")]
    DuplicateIban(String),
    #[error("missing binding for graph account {0}")]
    MissingName(String),
    #[error("binding {0} is not a graph account name")]
    UnexpectedName(String),
    #[error("binding name cannot be empty")]
    EmptyName,
    #[error("binding IBAN cannot be empty")]
    EmptyIban,
}

impl Bindings {
    /// Loads, parses, and validates an account-name-to-IBAN mapping.
    ///
    /// The input may be a path to a TOML, JSON, or YAML file, or inline
    /// content in any of those formats. The bindings must match the supplied
    /// graph names exactly.
    ///
    /// # Errors
    ///
    /// Returns an error when the file cannot be read or contains invalid or
    /// duplicate account-name/IBAN entries.
    pub fn load(input: &str, graph_names: &[&str]) -> Result<Self, BindingError> {
        let bindings = if !input.contains(['\n', '\r']) && Path::new(input).exists() {
            Self::from_path(input)?
        } else {
            Self::parse_text(input, "inline bindings")?
        };
        validate(&bindings)?;
        bindings.validate_names(graph_names)?;
        Ok(bindings)
    }

    fn from_path(path: impl AsRef<Path>) -> Result<Self, BindingError> {
        let path = path.as_ref();
        let label = path.display().to_string();
        if !path.is_file() {
            return Err(BindingError::NotAFile(label));
        }
        let format = path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase);
        let Some(format) =
            format.filter(|format| matches!(format.as_str(), "toml" | "json" | "yaml" | "yml"))
        else {
            return Err(BindingError::UnsupportedFormat { path: label });
        };
        let source = fs::read_to_string(path).map_err(|source| BindingError::Read {
            path: label.clone(),
            source,
        })?;
        Self::parse_text_with_format(&source, &label, &format)
    }

    fn parse_text(source: &str, label: &str) -> Result<Self, BindingError> {
        toml::from_str(source)
            .or_else(|_| serde_json::from_str(source))
            .or_else(|_| serde_yaml::from_str(source))
            .map_err(|error| BindingError::Parse {
                label: label.to_owned(),
                message: error.to_string(),
            })
    }

    fn parse_text_with_format(
        source: &str,
        label: &str,
        format: &str,
    ) -> Result<Self, BindingError> {
        let result = match format {
            "toml" => toml::from_str(source).map_err(|error| error.to_string()),
            "json" => serde_json::from_str(source).map_err(|error| error.to_string()),
            "yaml" | "yml" => serde_yaml::from_str(source).map_err(|error| error.to_string()),
            _ => {
                return Err(BindingError::UnsupportedFormat {
                    path: label.to_owned(),
                });
            }
        };
        result.map_err(|message| BindingError::Parse {
            label: label.to_owned(),
            message,
        })
    }

    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Binding> {
        self.account.iter().find(|binding| binding.name == name)
    }

    /// Requires the bindings to match the graph account names exactly.
    ///
    /// # Errors
    ///
    /// Returns an error when a graph account has no binding or a binding is
    /// not referenced by the graph.
    pub fn validate_names(&self, names: &[&str]) -> Result<(), BindingError> {
        for name in names {
            if self.get(name).is_none() {
                return Err(BindingError::MissingName((*name).to_owned()));
            }
        }
        for binding in &self.account {
            if !names.contains(&binding.name.as_str()) {
                return Err(BindingError::UnexpectedName(binding.name.clone()));
            }
        }
        Ok(())
    }
}

fn validate(bindings: &Bindings) -> Result<(), BindingError> {
    let mut names = std::collections::HashSet::new();
    let mut ibans = std::collections::HashSet::new();
    for binding in &bindings.account {
        if binding.name.is_empty() {
            return Err(BindingError::EmptyName);
        }
        if binding.iban.is_empty() {
            return Err(BindingError::EmptyIban);
        }
        if !names.insert(&binding.name) {
            return Err(BindingError::DuplicateName(binding.name.clone()));
        }
        if !ibans.insert(&binding.iban) {
            return Err(BindingError::DuplicateIban(binding.iban.clone()));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Binding, BindingError, Bindings};

    fn make_bindings(names: &[&str]) -> Bindings {
        Bindings {
            account: names
                .iter()
                .map(|name| Binding {
                    name: (*name).to_owned(),
                    iban: format!("NL00TEST{name}"),
                    external: false,
                })
                .collect(),
        }
    }

    #[test]
    fn requires_exact_graph_names() {
        let bindings = make_bindings(&["source"]);

        assert!(matches!(
            bindings.validate_names(&["source", "sink"]),
            Err(BindingError::MissingName(name)) if name == "sink"
        ));
        assert!(matches!(
            make_bindings(&["source", "sink"]).validate_names(&["source"]),
            Err(BindingError::UnexpectedName(name)) if name == "sink"
        ));
    }

    #[test]
    fn loads_json_and_yaml_bindings() {
        let json = r#"{"account":[{"name":"checking","iban":"NL00TESTchecking"}]}"#;
        let yaml = "account:\n  - name: checking\n    iban: NL00TESTchecking\n";

        assert!(Bindings::load(json, &["checking"]).is_ok());
        assert!(Bindings::load(yaml, &["checking"]).is_ok());
    }
}
