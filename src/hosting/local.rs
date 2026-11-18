use super::{Locator, RepositoryName};
use crate::accounts::{Control, Error, Provider};
use std::{
    collections::HashSet,
    io::Read,
    path::{Path, PathBuf},
    process::Stdio,
};
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteRepository {
    pub provider: Provider,
    pub repository: RepositoryName,
}
impl RemoteRepository {
    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        let (host, path) = if let Some(value) = value.strip_prefix("git@") {
            let (host, path) = value.split_once(':')?;
            (host.to_owned(), path.to_owned())
        } else {
            let url = reqwest::Url::parse(value).ok()?;
            if !matches!(url.scheme(), "https" | "ssh") {
                return None;
            }
            (
                url.host_str()?.to_owned(),
                url.path().trim_start_matches('/').to_owned(),
            )
        };
        let provider = match host.as_str() {
            "github.com" => Provider::Github,
            "bitbucket.org" => Provider::Bitbucket,
            _ => return None,
        };
        let path = path.trim_end_matches('/');
        let path = path.strip_suffix(".git").unwrap_or(path);
        Some(Self {
            provider,
            repository: RepositoryName::parse(path).ok()?,
        })
    }
}
pub fn repository_remotes(root: &Path) -> Result<Vec<RemoteRepository>, Error> {
    let mut command = crate::tools::command("git").map_err(|_| Error::LocalRepository)?;
    let mut child = command
        .arg("-C")
        .arg(root)
        .args([
            "config",
            "--local",
            "--no-includes",
            "--get-regexp",
            r"^remote\..*\.url$",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| Error::LocalRepository)?;
    let mut bytes = Vec::new();
    let read = child
        .stdout
        .take()
        .expect("stdout is piped")
        .take(65_537)
        .read_to_end(&mut bytes);
    if read.is_err() || bytes.len() > 65_536 {
        let _ = child.kill();
        let _ = child.wait();
        return Err(Error::LocalRepository);
    }
    let status = child.wait().map_err(|_| Error::LocalRepository)?;
    if !status.success() && status.code() != Some(1) {
        return Err(Error::LocalRepository);
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| Error::LocalRepository)?;
    let mut remotes = Vec::new();
    for line in text.lines() {
        if let Some((_, url)) = line.split_once(' ')
            && let Some(repo) = RemoteRepository::parse(url)
            && !remotes.contains(&repo)
        {
            remotes.push(repo);
        }
    }
    Ok(remotes)
}
#[derive(Clone, Debug, Default)]
pub struct Association {
    pub matches: Vec<PathBuf>,
    pub unreadable: usize,
    pub truncated: bool,
}
pub fn associate(
    locator: &Locator,
    candidates: &[PathBuf],
    control: &Control,
) -> Result<Association, Error> {
    let mut result = Association {
        truncated: candidates.len() > 50,
        ..Association::default()
    };
    let mut seen = HashSet::new();
    for path in candidates.iter().take(50) {
        control.check()?;
        let canonical = match path.canonicalize() {
            Ok(path) => path,
            Err(_) => {
                result.unreadable += 1;
                continue;
            }
        };
        if !seen.insert(canonical) {
            continue;
        }
        match repository_remotes(path) {
            Ok(remotes) => {
                if remotes.iter().any(|remote| {
                    remote.provider == locator.provider
                        && remote
                            .repository
                            .to_string()
                            .eq_ignore_ascii_case(&locator.repository.to_string())
                }) {
                    result.matches.push(path.clone());
                }
            }
            Err(_) => result.unreadable += 1,
        }
    }
    control.check()?;
    Ok(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn remote_recognition_uses_exact_hosts_and_does_not_retain_credentials() {
        for value in [
            "git@github.com:team/repo.git",
            "ssh://git@github.com/team/repo.git",
            "https://user:secret@github.com/team/repo.git",
        ] {
            let repo = RemoteRepository::parse(value).unwrap();
            assert_eq!(repo.repository.to_string(), "team/repo");
            assert!(!format!("{repo:?}").contains("secret"));
        }
        assert!(RemoteRepository::parse("https://github.com.evil/team/repo").is_none());
        assert!(RemoteRepository::parse("git@unknown:team/repo.git").is_none());
    }
}

#[cfg(test)]
mod association_tests {
    use super::*;
    use std::time::Duration;
    #[test]
    fn association_reads_remotes_without_fetching_or_changing_working_files() {
        let fixture = tempfile::tempdir().unwrap();
        let one = fixture.path().join("one");
        let two = fixture.path().join("two");
        for (root, url) in [
            (&one, "git@github.com:team/repo.git"),
            (&two, "git@github.com:another/repo.git"),
        ] {
            std::fs::create_dir(root).unwrap();
            for args in [vec!["init", "-q"], vec!["remote", "add", "origin", url]] {
                assert!(
                    std::process::Command::new("git")
                        .arg("-C")
                        .arg(root)
                        .args(args)
                        .status()
                        .unwrap()
                        .success()
                );
            }
            std::fs::write(root.join("untouched.txt"), "unsaved content").unwrap();
        }
        let locator = Locator {
            provider: Provider::Github,
            repository: RepositoryName::parse("team/repo").unwrap(),
            number: 42,
        };
        let result = associate(
            &locator,
            &[one.clone(), two, fixture.path().join("missing")],
            &Control::new(Duration::from_secs(3)),
        )
        .unwrap();
        assert_eq!(result.matches, vec![one.clone()]);
        assert_eq!(result.unreadable, 1);
        assert_eq!(
            std::fs::read_to_string(one.join("untouched.txt")).unwrap(),
            "unsaved content"
        );
        assert!(!one.join(".git/FETCH_HEAD").exists());
    }
}
