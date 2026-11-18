//! Bounded, private metadata snapshots for restart recovery. Never credentials
//! or PR bodies. Reads and writes run on workers; replacement is atomic.
use super::{Batch, EntryInput, PullRequest, Scope, model::bounded_text};
use crate::accounts::{AccountKey, Control, Error};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{Read, Write},
    path::Path,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
const MAX_BYTES: usize = 4 * 1024 * 1024;
const MAX_ENTRIES: usize = 16;
static WRITER: Mutex<()> = Mutex::new(());
#[derive(Serialize)]
pub struct Snapshots {
    version: u8,
    records: Vec<Record>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileData {
    version: u8,
    records: Vec<Record>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    account: AccountKey,
    scope: String,
    saved_at: u64,
    #[serde(default)]
    fetched_nanos: u32,
    items: Vec<Item>,
    partial: bool,
    coverage: Vec<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Item {
    url: String,
    id: String,
    repository_id: String,
    title: String,
    author: String,
    branch: String,
    state: String,
}
pub struct SavedBatch {
    pub batch: Arc<Batch>,
    pub saved_at: u64,
}
fn scope_key(scope: &Scope) -> String {
    match scope {
        Scope::Authored => "authored".into(),
        Scope::ReviewRequested => "review-requested".into(),
        Scope::Repository(provider, repo) => format!("repository:{}:{repo}", provider.name()),
        Scope::Direct(locator) => format!("direct:{}", locator.url()),
    }
}
fn checked(text: &str, limit: usize) -> Result<String, Error> {
    bounded_text(Some(text), limit).map_err(|_| Error::DiscoveryStorage)
}
impl Record {
    fn batch(&self) -> Result<Batch, Error> {
        if self.fetched_nanos >= 1_000_000_000 {
            return Err(Error::DiscoveryStorage);
        }
        if self.items.len() > 256 || self.coverage.len() > 10 || self.account.subject.is_empty() {
            return Err(Error::DiscoveryStorage);
        }
        checked(&self.account.subject, 256)?;
        checked(&self.scope, 4096)?;
        let mut seen = std::collections::HashSet::new();
        let items = self
            .items
            .iter()
            .map(|item| {
                let Ok(EntryInput::Url(locator)) = EntryInput::parse(&item.url) else {
                    return Err(Error::DiscoveryStorage);
                };
                if locator.provider != self.account.provider || !seen.insert(locator.clone()) {
                    return Err(Error::DiscoveryStorage);
                }
                Ok(PullRequest {
                    locator,
                    id: checked(&item.id, 512)?,
                    repository_id: checked(&item.repository_id, 512)?,
                    title: checked(&item.title, 4096)?,
                    author: checked(&item.author, 256)?,
                    branch: checked(&item.branch, 1024)?,
                    state: checked(&item.state, 64)?,
                })
            })
            .collect::<Result<Vec<_>, Error>>()?;
        Ok(Batch {
            items,
            partial: self.partial,
            coverage: self
                .coverage
                .iter()
                .map(|text| checked(text, 2048))
                .collect::<Result<_, _>>()?,
        })
    }
}
impl Snapshots {
    pub fn get(&self, account: &AccountKey, scope: &Scope) -> Option<SavedBatch> {
        self.records
            .iter()
            .find(|record| &record.account == account && record.scope == scope_key(scope))
            .map(|record| SavedBatch {
                batch: Arc::new(record.batch().expect("snapshot was validated on load")),
                saved_at: record.saved_at,
            })
    }
}
pub fn load(path: &Path) -> Result<Snapshots, Error> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Snapshots {
                version: 1,
                records: vec![],
            });
        }
        Err(_) => return Err(Error::DiscoveryStorage),
    };
    let mut bytes = Vec::new();
    file.take((MAX_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::DiscoveryStorage)?;
    if bytes.len() > MAX_BYTES {
        return Err(Error::DiscoveryStorage);
    }
    let data: FileData = serde_json::from_slice(&bytes).map_err(|_| Error::DiscoveryStorage)?;
    let snapshots = Snapshots {
        version: data.version,
        records: data.records,
    };
    if snapshots.version != 1 || snapshots.records.len() > MAX_ENTRIES {
        return Err(Error::DiscoveryStorage);
    }
    for record in &snapshots.records {
        record.batch()?;
    }
    Ok(snapshots)
}
pub fn save(
    path: &Path,
    account: &AccountKey,
    scope: &Scope,
    batch: &Batch,
    fetched_at: SystemTime,
    control: &Control,
) -> Result<(), Error> {
    let _writer = loop {
        control.check()?;
        match WRITER.try_lock() {
            Ok(guard) => break guard,
            Err(std::sync::TryLockError::WouldBlock) => {
                std::thread::sleep(Duration::from_millis(20))
            }
            Err(std::sync::TryLockError::Poisoned(_)) => panic!("snapshot writer mutex poisoned"),
        }
    };
    let mut snapshots = load(path)?;
    let scope = scope_key(scope);
    let fetched = fetched_at
        .duration_since(UNIX_EPOCH)
        .map_err(|_| Error::DiscoveryStorage)?;
    if snapshots.records.iter().any(|record| {
        &record.account == account
            && record.scope == scope
            && (record.saved_at, record.fetched_nanos)
                >= (fetched.as_secs(), fetched.subsec_nanos())
    }) {
        return Ok(());
    }
    let record = Record {
        account: account.clone(),
        scope: scope.clone(),
        saved_at: fetched.as_secs(),
        fetched_nanos: fetched.subsec_nanos(),
        items: batch
            .items
            .iter()
            .map(|item| Item {
                url: item.locator.url(),
                id: item.id.clone(),
                repository_id: item.repository_id.clone(),
                title: item.title.clone(),
                author: item.author.clone(),
                branch: item.branch.clone(),
                state: item.state.clone(),
            })
            .collect(),
        partial: batch.partial,
        coverage: batch.coverage.clone(),
    };
    record.batch()?;
    snapshots
        .records
        .retain(|record| &record.account != account || record.scope != scope);
    snapshots.records.insert(0, record);
    snapshots.records.truncate(MAX_ENTRIES);
    let bytes = loop {
        let bytes = serde_json::to_vec(&snapshots).map_err(|_| Error::DiscoveryStorage)?;
        if bytes.len() <= MAX_BYTES {
            break bytes;
        }
        if snapshots.records.len() == 1 {
            return Err(Error::DiscoveryStorage);
        }
        snapshots.records.pop();
    };
    let parent = path.parent().ok_or(Error::DiscoveryStorage)?;
    std::fs::create_dir_all(parent).map_err(|_| Error::DiscoveryStorage)?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|_| Error::DiscoveryStorage)?;
    file.write_all(&bytes)
        .map_err(|_| Error::DiscoveryStorage)?;
    file.as_file()
        .sync_all()
        .map_err(|_| Error::DiscoveryStorage)?;
    control.check()?;
    file.persist(path).map_err(|_| Error::DiscoveryStorage)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        accounts::Provider,
        hosting::{Locator, RepositoryName},
    };
    fn key() -> AccountKey {
        AccountKey {
            provider: Provider::Github,
            subject: "synthetic-account".into(),
        }
    }
    fn batch() -> Batch {
        Batch {
            items: vec![PullRequest {
                locator: Locator {
                    provider: Provider::Github,
                    repository: RepositoryName::parse("team/widgets").unwrap(),
                    number: 42,
                },
                id: "PR_42".into(),
                repository_id: "R_widgets".into(),
                title: "Old wheel snapshot".into(),
                author: "person".into(),
                branch: "fix/wheel".into(),
                state: "OPEN".into(),
            }],
            partial: true,
            coverage: vec!["synthetic".into()],
        }
    }
    fn control() -> Control {
        Control::new(Duration::from_secs(10))
    }
    #[test]
    fn metadata_survives_reload_and_is_partitioned_by_account_and_scope() {
        let fixture = tempfile::tempdir().unwrap();
        let path = fixture.path().join("snapshots.json");
        save(
            &path,
            &key(),
            &Scope::Authored,
            &batch(),
            SystemTime::now(),
            &control(),
        )
        .unwrap();
        let restored = load(&path).unwrap().get(&key(), &Scope::Authored).unwrap();
        assert_eq!(restored.batch.items, batch().items);
        assert!(restored.batch.partial);
        assert!(restored.saved_at > 0);
        assert!(
            load(&path)
                .unwrap()
                .get(&key(), &Scope::ReviewRequested)
                .is_none()
        );
        let mut other = key();
        other.subject = "another-account".into();
        assert!(load(&path).unwrap().get(&other, &Scope::Authored).is_none());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }
    #[test]
    fn invalid_snapshots_are_bounded_and_never_replaced_by_save() {
        let fixture = tempfile::tempdir().unwrap();
        let path = fixture.path().join("snapshots.json");
        save(
            &path,
            &key(),
            &Scope::Authored,
            &batch(),
            SystemTime::now(),
            &control(),
        )
        .unwrap();
        let mut value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        value["records"][0]["items"][0]["url"] =
            "https://user:secret@github.com/team/widgets/pull/42".into();
        let invalid = serde_json::to_vec(&value).unwrap();
        std::fs::write(&path, &invalid).unwrap();
        assert!(load(&path).is_err());
        assert!(
            save(
                &path,
                &key(),
                &Scope::Authored,
                &batch(),
                SystemTime::now(),
                &control()
            )
            .is_err()
        );
        assert_eq!(std::fs::read(&path).unwrap(), invalid);
        std::fs::write(&path, vec![0; MAX_BYTES + 1]).unwrap();
        assert!(load(&path).is_err());
    }
    #[test]
    fn updates_replace_only_their_scope_and_evict_oldest_entries() {
        let fixture = tempfile::tempdir().unwrap();
        let path = fixture.path().join("snapshots.json");
        for index in 0..20 {
            let scope = Scope::Repository(
                crate::accounts::Provider::Github,
                RepositoryName::parse(&format!("team/repo{index}")).unwrap(),
            );
            save(
                &path,
                &key(),
                &scope,
                &batch(),
                SystemTime::now(),
                &control(),
            )
            .unwrap();
        }
        let snapshots = load(&path).unwrap();
        assert_eq!(snapshots.records.len(), MAX_ENTRIES);
        save(
            &path,
            &key(),
            &Scope::Authored,
            &Batch::default(),
            SystemTime::now(),
            &control(),
        )
        .unwrap();
        assert!(
            load(&path)
                .unwrap()
                .get(&key(), &Scope::Authored)
                .unwrap()
                .batch
                .items
                .is_empty()
        );
    }
    #[test]
    fn late_or_reused_results_cannot_replace_a_newer_saved_snapshot() {
        let fixture = tempfile::tempdir().unwrap();
        let path = fixture.path().join("snapshots.json");
        let newer = UNIX_EPOCH + Duration::from_secs(20);
        save(&path, &key(), &Scope::Authored, &batch(), newer, &control()).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let mut older = batch();
        older.items[0].title = "Older result".into();
        save(
            &path,
            &key(),
            &Scope::Authored,
            &older,
            UNIX_EPOCH + Duration::from_secs(10),
            &control(),
        )
        .unwrap();
        save(&path, &key(), &Scope::Authored, &older, newer, &control()).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        save(
            &path,
            &key(),
            &Scope::Authored,
            &older,
            UNIX_EPOCH + Duration::from_secs(30),
            &control(),
        )
        .unwrap();
        assert_eq!(
            load(&path)
                .unwrap()
                .get(&key(), &Scope::Authored)
                .unwrap()
                .batch
                .items[0]
                .title,
            "Older result"
        );
    }
}
