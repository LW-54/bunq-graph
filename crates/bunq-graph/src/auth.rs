use std::fs;
use std::io::Write;
use std::path::Path;

use bunq_api::{Client, InstallationContext, PersistedInstallation, Session, install_device};
use thiserror::Error;

pub struct ConnectedClient {
    pub client: Client,
    pub context: InstallationContext,
    pub session: Session,
}

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("bunq authentication failed: {0}")]
    Bunq(#[source] Box<bunq_api::Error>),
    #[error("no API key file was configured for first-time installation")]
    MissingApiKey,
    #[error("persisted installation is invalid: {0}")]
    Installation(#[source] Box<bunq_api::Error>),
    #[error("installation storage failed: {0}")]
    Storage(String),
}

/// Connects using a runtime-selected installation path, creating and saving it
/// when the file does not exist.
///
/// # Errors
///
/// Returns an error when the embedded installation is invalid or bunq rejects
/// installation/session creation.
pub async fn connect(
    installation_path: &Path,
    api_key: Option<&str>,
    base_url: &str,
    user_agent: &str,
    device_description: &str,
) -> Result<ConnectedClient, AuthError> {
    let path = installation_path;
    if installation_path.is_file() {
        let json = fs::read_to_string(path)
            .map_err(|error| AuthError::Storage(format!("could not read installation: {error}")))?;
        let persisted = PersistedInstallation::from_json(&json)
            .map_err(|error| AuthError::Installation(Box::new(error)))?;
        let context = persisted
            .into_context()
            .map_err(|error| AuthError::Installation(Box::new(error)))?;
        let (client, session) = Client::connect(&context)
            .await
            .map_err(|error| AuthError::Bunq(Box::new(error)))?;
        return Ok(ConnectedClient {
            client,
            context,
            session,
        });
    }

    let api_key = api_key.ok_or(AuthError::MissingApiKey)?;
    let (context, session) = install_device(api_key, base_url, user_agent, device_description)
        .await
        .map_err(|error| AuthError::Bunq(Box::new(error)))?;
    let client = Client::from_installation(&context, session.token())
        .map_err(|error| AuthError::Bunq(Box::new(error)))?;
    let json = persisted_json(&context)?;
    write_installation(path, &json)?;
    Ok(ConnectedClient {
        client,
        context,
        session,
    })
}

fn write_installation(path: &Path, json: &str) -> Result<(), AuthError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            AuthError::Storage(format!("could not create installation directory: {error}"))
        })?;
    }
    let temporary = path.with_extension("tmp");
    let mut file = fs::File::create(&temporary).map_err(|error| {
        AuthError::Storage(format!("could not create installation file: {error}"))
    })?;
    file.write_all(json.as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|error| {
            AuthError::Storage(format!("could not write installation file: {error}"))
        })?;
    fs::rename(temporary, path)
        .map_err(|error| AuthError::Storage(format!("could not commit installation file: {error}")))
}

/// Serializes an installation for the runtime installation file.
///
/// # Errors
///
/// Returns an error when the SDK cannot serialize the installation record.
pub fn persisted_json(context: &InstallationContext) -> Result<String, AuthError> {
    context
        .to_persisted()
        .to_json()
        .map_err(|error| AuthError::Installation(Box::new(error)))
}
