//! Persist only canonical locators, never titles, account grants or credentials.
use super::{EntryInput, Locator};
use crate::accounts::Error;
use std::{
    fs::File,
    io::{Read, Write},
    path::Path,
};
pub fn load(path: &Path) -> Result<Vec<Locator>, Error> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(_) => return Err(Error::DiscoveryStorage),
    };
    let mut bytes = vec![];
    file.take(65_537)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::DiscoveryStorage)?;
    if bytes.len() > 65_536 {
        return Err(Error::DiscoveryStorage);
    }
    let urls: Vec<String> = serde_json::from_slice(&bytes).map_err(|_| Error::DiscoveryStorage)?;
    if urls.len() > 50 {
        return Err(Error::DiscoveryStorage);
    }
    urls.into_iter()
        .map(|url| match EntryInput::parse(&url) {
            Ok(EntryInput::Url(locator)) => Ok(locator),
            _ => Err(Error::DiscoveryStorage),
        })
        .collect()
}
pub fn save(path: &Path, locators: &[Locator]) -> Result<(), Error> {
    let parent = path.parent().ok_or(Error::DiscoveryStorage)?;
    std::fs::create_dir_all(parent).map_err(|_| Error::DiscoveryStorage)?;
    let urls = locators
        .iter()
        .take(50)
        .map(Locator::url)
        .collect::<Vec<_>>();
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|_| Error::DiscoveryStorage)?;
    file.write_all(&serde_json::to_vec(&urls).map_err(|_| Error::DiscoveryStorage)?)
        .map_err(|_| Error::DiscoveryStorage)?;
    file.persist(path).map_err(|_| Error::DiscoveryStorage)?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recent_files_reject_credentials_and_round_trip_canonical_locators() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("recent.json");
        let EntryInput::Url(locator) =
            EntryInput::parse("https://github.com/team/repo/pull/7").unwrap()
        else {
            panic!()
        };
        save(&path, std::slice::from_ref(&locator)).unwrap();
        assert_eq!(load(&path).unwrap(), vec![locator]);
        std::fs::write(
            &path,
            r#"["https://user:secret@github.com/team/repo/pull/7"]"#,
        )
        .unwrap();
        assert!(load(&path).is_err());
    }
}
