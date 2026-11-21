use super::{AccountKey, ClientRegistration, Error, Provider, StoredSession};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

/// Production has no plaintext or in-memory fallback. Test doubles implement
/// the same boundary so store denial and partial refresh failures are repeatable.
pub trait CredentialStore: Send + Sync {
    fn load(&self) -> Result<Vec<StoredSession>, Error>;
    fn save(&self, session: &StoredSession) -> Result<(), Error>;
    fn remove(&self, key: &AccountKey) -> Result<(), Error>;
    fn registration(
        &self,
        provider: Provider,
        client_id: &str,
    ) -> Result<ClientRegistration, Error>;
    fn selected_registration(&self, provider: Provider) -> Result<ClientRegistration, Error> {
        self.registration(provider, "")
    }
    fn import_development_registration(&self, _: Provider) -> Result<(), Error> {
        Err(Error::Registration)
    }
}

const SERVICE: &str = "jaywalk.accounts.v1";
const ACCOUNT: &str = "sessions";
#[derive(Serialize, Deserialize)]
struct Vault {
    version: u32,
    sessions: Vec<StoredSession>,
}

/// One process owns the application server. This gate serializes read/modify/
/// write across different accounts; the OS store's single entry replaces atomically.
/// Registrations use separate entries and survive account disconnection.
#[derive(Default)]
pub struct OsCredentialStore {
    gate: Mutex<()>,
}
impl OsCredentialStore {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn save_registration(&self, registration: &ClientRegistration) -> Result<(), Error> {
        registration.validate()?;
        let encoded = serde_json::to_string(registration).map_err(|_| Error::StoreInvalid)?;
        entry(&registration_name(
            registration.provider,
            &registration.client_id,
        ))?
        .set_password(&encoded)
        .map_err(|_| Error::StoreUnavailable)
    }
    fn select_registration(&self, registration: &ClientRegistration) -> Result<(), Error> {
        self.save_registration(registration)?;
        entry(&format!("selected-client/{}", registration.provider.name()))?
            .set_password(&registration.client_id)
            .map_err(|_| Error::StoreUnavailable)
    }
    fn read(&self) -> Result<Vec<StoredSession>, Error> {
        let encoded = match entry(ACCOUNT)?.get_password() {
            Ok(value) => value,
            Err(keyring::Error::NoEntry) => return Ok(Vec::new()),
            Err(_) => return Err(Error::StoreUnavailable),
        };
        decode(&encoded)
    }
    fn write(&self, sessions: Vec<StoredSession>) -> Result<(), Error> {
        let encoded = serde_json::to_string(&Vault {
            version: 1,
            sessions,
        })
        .map_err(|_| Error::StoreInvalid)?;
        entry(ACCOUNT)?
            .set_password(&encoded)
            .map_err(|_| Error::StoreUnavailable)
    }
}
fn entry(account: &str) -> Result<keyring::Entry, Error> {
    keyring::Entry::new(SERVICE, account).map_err(|_| Error::StoreUnavailable)
}
fn registration_name(provider: Provider, client_id: &str) -> String {
    format!("client/{}/{client_id}", provider.name())
}
fn decode(encoded: &str) -> Result<Vec<StoredSession>, Error> {
    let vault: Vault = serde_json::from_str(encoded).map_err(|_| Error::StoreInvalid)?;
    if vault.version != 1 {
        return Err(Error::StoreInvalid);
    }
    let mut seen = std::collections::HashSet::new();
    for session in &vault.sessions {
        session.validate().map_err(|_| Error::StoreInvalid)?;
        if !seen.insert(session.identity.key.clone()) {
            return Err(Error::StoreInvalid);
        }
    }
    Ok(vault.sessions)
}
impl CredentialStore for OsCredentialStore {
    fn load(&self) -> Result<Vec<StoredSession>, Error> {
        let _guard = self.gate.lock().expect("credential store mutex poisoned");
        self.read()
    }
    fn save(&self, session: &StoredSession) -> Result<(), Error> {
        session.validate()?;
        let _guard = self.gate.lock().expect("credential store mutex poisoned");
        let mut sessions = self.read()?;
        sessions.retain(|old| old.identity.key != session.identity.key);
        sessions.push(session.clone());
        self.write(sessions)
    }
    fn remove(&self, key: &AccountKey) -> Result<(), Error> {
        let _guard = self.gate.lock().expect("credential store mutex poisoned");
        let mut sessions = self.read()?;
        sessions.retain(|old| &old.identity.key != key);
        self.write(sessions)
    }
    fn registration(
        &self,
        provider: Provider,
        client_id: &str,
    ) -> Result<ClientRegistration, Error> {
        let encoded = match entry(&registration_name(provider, client_id))?.get_password() {
            Ok(encoded) => encoded,
            Err(keyring::Error::NoEntry) => {
                return super::publisher::registration(provider, Some(client_id));
            }
            Err(_) => return Err(Error::StoreUnavailable),
        };
        let registration: ClientRegistration =
            serde_json::from_str(&encoded).map_err(|_| Error::StoreInvalid)?;
        registration.validate()?;
        if registration.provider != provider || registration.client_id != client_id {
            return Err(Error::Registration);
        }
        Ok(registration)
    }
    fn selected_registration(&self, provider: Provider) -> Result<ClientRegistration, Error> {
        let id = match entry(&format!("selected-client/{}", provider.name()))?.get_password() {
            Ok(id) => id,
            Err(keyring::Error::NoEntry) => return super::publisher::registration(provider, None),
            Err(_) => return Err(Error::StoreUnavailable),
        };
        self.registration(provider, &id)
    }
    fn import_development_registration(&self, provider: Provider) -> Result<(), Error> {
        // Explicit developer action only. Copy app registration, never user tokens.
        let account = format!("{}-development-registration", provider.name());
        let source = keyring::Entry::new("jaywalk.hosting-auth-poc.v1", &account)
            .map_err(|_| Error::StoreUnavailable)?;
        let encoded = source.get_password().map_err(|error| match error {
            keyring::Error::NoEntry => Error::Registration,
            _ => Error::StoreUnavailable,
        })?;
        self.select_registration(&development_registration(provider, &encoded)?)
    }
}

fn development_registration(
    provider: Provider,
    encoded: &str,
) -> Result<ClientRegistration, Error> {
    let value: serde_json::Value =
        serde_json::from_str(encoded).map_err(|_| Error::StoreInvalid)?;
    let secret_key = if provider == Provider::Github {
        "client_secret"
    } else {
        "secret"
    };
    let registration = ClientRegistration {
        provider,
        client_id: value
            .get("client_id")
            .and_then(serde_json::Value::as_str)
            .ok_or(Error::Registration)?
            .into(),
        secret: super::Secret::new(
            value
                .get(secret_key)
                .and_then(serde_json::Value::as_str)
                .ok_or(Error::Registration)?
                .into(),
        )?,
    };
    registration.validate()?;
    Ok(registration)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn corrupt_or_future_store_is_not_silently_replaced_with_empty_accounts() {
        for value in [
            "",
            "{}",
            "{\"version\":2,\"sessions\":[]}",
            "{\"version\":1,\"sessions\":[{}]}",
        ] {
            assert!(matches!(decode(value), Err(Error::StoreInvalid)));
        }
        assert!(
            decode("{\"version\":1,\"sessions\":[]}")
                .unwrap()
                .is_empty()
        );
    }
    #[test]
    fn development_import_extracts_only_registration_and_requires_a_secret() {
        for (provider, secret_field) in [
            (Provider::Github, "client_secret"),
            (Provider::Bitbucket, "secret"),
        ] {
            let value = serde_json::json!({"client_id":"synthetic-client",secret_field:"synthetic-secret","access_token":"do-not-import","private_key":"do-not-import"});
            let imported = development_registration(provider, &value.to_string()).unwrap();
            assert_eq!(imported.provider, provider);
            assert_eq!(imported.client_id, "synthetic-client");
            let encoded = serde_json::to_string(&imported).unwrap();
            assert!(!encoded.contains("do-not-import"));
            assert!(matches!(
                development_registration(provider, r#"{"client_id":"synthetic-client"}"#),
                Err(Error::Registration)
            ));
        }
    }
}
