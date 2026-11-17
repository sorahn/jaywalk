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
        let encoded = entry(&registration_name(provider, client_id))?
            .get_password()
            .map_err(|error| match error {
                keyring::Error::NoEntry => Error::Registration,
                _ => Error::StoreUnavailable,
            })?;
        let registration: ClientRegistration =
            serde_json::from_str(&encoded).map_err(|_| Error::StoreInvalid)?;
        registration.validate()?;
        if registration.provider != provider || registration.client_id != client_id {
            return Err(Error::Registration);
        }
        Ok(registration)
    }
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
}
