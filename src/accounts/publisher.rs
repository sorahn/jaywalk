//! Publisher-owned native client identity. GitHub documents these bundled client
//! credentials as public; user grants remain exclusively in the OS token store.
use super::{ClientRegistration, Error, Provider, Secret};

fn configured(
    id: Option<&str>,
    secret: Option<&str>,
    slug: Option<&str>,
) -> Result<Option<ClientRegistration>, Error> {
    match (id, secret, slug) {
        (None, None, None) => Ok(None),
        (Some(id), Some(secret), Some(slug))
            if !slug.is_empty()
                && slug.len() <= 100
                && slug.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') =>
        {
            let registration = ClientRegistration {
                provider: Provider::Github,
                client_id: id.into(),
                secret: Secret::new(secret.into()).map_err(|_| Error::Registration)?,
            };
            registration.validate().map_err(|_| Error::Registration)?;
            Ok(Some(registration))
        }
        _ => Err(Error::Registration),
    }
}

fn github() -> Result<Option<ClientRegistration>, Error> {
    configured(
        option_env!("JAYWALK_GITHUB_CLIENT_ID"),
        option_env!("JAYWALK_GITHUB_CLIENT_SECRET"),
        option_env!("JAYWALK_GITHUB_APP_SLUG"),
    )
}

pub(super) fn registration(
    provider: Provider,
    client_id: Option<&str>,
) -> Result<ClientRegistration, Error> {
    if provider != Provider::Github {
        return Err(Error::Registration);
    }
    matching(github()?, client_id)
}

fn matching(
    registration: Option<ClientRegistration>,
    client_id: Option<&str>,
) -> Result<ClientRegistration, Error> {
    registration
        .filter(|registration| client_id.is_none_or(|id| registration.client_id == id))
        .ok_or(Error::Registration)
}

/// Build capability only; never reads account sessions or contacts GitHub.
pub fn github_configured() -> bool {
    github().is_ok_and(|registration| registration.is_some())
}

/// Public, fixed-origin installation link; opening it remains an explicit action.
pub fn github_install_url() -> Option<String> {
    github().ok().flatten()?;
    Some(format!(
        "https://github.com/apps/{}/installations/new",
        option_env!("JAYWALK_GITHUB_APP_SLUG")?
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fresh_and_restored_sessions_keep_their_exact_publisher_identity() {
        let registration = configured(
            Some("publisher-client"),
            Some("synthetic-secret"),
            Some("jaywalk"),
        )
        .unwrap();
        assert_eq!(
            matching(registration.clone(), None).unwrap().client_id,
            "publisher-client"
        );
        assert!(matching(registration.clone(), Some("publisher-client")).is_ok());
        assert!(matches!(
            matching(registration, Some("different-development-client")),
            Err(Error::Registration)
        ));
        assert!(matches!(matching(None, None), Err(Error::Registration)));
    }
    #[test]
    fn packaged_setup_needs_all_fields_and_valid_identifiers() {
        assert!(configured(None, None, None).unwrap().is_none());
        for values in [
            (Some("client"), None, Some("jaywalk")),
            (None, Some("secret"), Some("jaywalk")),
            (Some("client"), Some("secret"), None),
            (Some(""), Some("secret"), Some("jaywalk")),
            (Some("client"), Some("secret"), Some("../evil")),
            (Some("client"), Some("secret\n"), Some("jaywalk")),
        ] {
            assert!(matches!(
                configured(values.0, values.1, values.2),
                Err(Error::Registration)
            ));
        }
        let app = configured(
            Some("synthetic-client"),
            Some("synthetic-secret"),
            Some("jaywalk"),
        )
        .unwrap()
        .unwrap();
        assert_eq!(app.client_id, "synthetic-client");
        assert_eq!(app.provider, Provider::Github);
    }
}
