use crate::accounts::{Error, Provider};
use reqwest::Url;
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RepositoryName {
    pub(crate) owner: String,
    pub(crate) name: String,
}
impl RepositoryName {
    pub fn parse(value: &str) -> Result<Self, Error> {
        let (owner, name) = value.split_once('/').ok_or(Error::InvalidResponse)?;
        let valid = |part: &str| {
            !part.is_empty()
                && part.len() <= 256
                && part != "."
                && part != ".."
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
        };
        if !valid(owner) || !valid(name) {
            return Err(Error::InvalidResponse);
        }
        Ok(Self {
            owner: owner.into(),
            name: name.into(),
        })
    }
}
impl fmt::Display for RepositoryName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.owner, self.name)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Locator {
    pub provider: Provider,
    pub repository: RepositoryName,
    pub number: u64,
}
impl Locator {
    pub fn url(&self) -> String {
        match self.provider {
            Provider::Github => format!(
                "https://github.com/{}/pull/{}",
                self.repository, self.number
            ),
            Provider::Bitbucket => format!(
                "https://bitbucket.org/{}/pull-requests/{}",
                self.repository, self.number
            ),
        }
    }
    pub fn label(&self) -> String {
        format!(
            "{} · {}#{}",
            self.provider.name(),
            self.repository,
            self.number
        )
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EntryInput {
    Url(Locator),
    Qualified {
        repository: RepositoryName,
        number: u64,
    },
    Number(u64),
    Fuzzy,
}
impl EntryInput {
    pub fn parse(value: &str) -> Result<Self, Error> {
        let value = value.trim();
        let number = |value: &str| {
            value
                .parse::<u64>()
                .ok()
                .filter(|number| *number > 0)
                .ok_or(Error::InvalidResponse)
        };
        if value.starts_with("https://") || value.contains("://") {
            let url = Url::parse(value).map_err(|_| Error::InvalidResponse)?;
            if url.scheme() != "https"
                || !url.username().is_empty()
                || url.password().is_some()
                || url.port_or_known_default() != Some(443)
                || url.query().is_some()
            {
                return Err(Error::InvalidResponse);
            }
            let parts = url
                .path()
                .trim_end_matches('/')
                .split('/')
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>();
            if parts.len() != 4 {
                return Err(Error::InvalidResponse);
            }
            let provider = match (url.host_str(), parts[2]) {
                (Some("github.com"), "pull") => Provider::Github,
                (Some("bitbucket.org"), "pull-requests") => Provider::Bitbucket,
                _ => return Err(Error::Unsupported),
            };
            return Ok(Self::Url(Locator {
                provider,
                repository: RepositoryName::parse(&format!("{}/{}", parts[0], parts[1]))?,
                number: number(parts[3])?,
            }));
        }
        if let Some((repo, n)) = value.rsplit_once('#')
            && repo.contains('/')
        {
            return Ok(Self::Qualified {
                repository: RepositoryName::parse(repo)?,
                number: number(n)?,
            });
        }
        let numeric = value.strip_prefix('#').unwrap_or(value);
        if !numeric.is_empty() && numeric.bytes().all(|b| b.is_ascii_digit()) {
            return Ok(Self::Number(number(numeric)?));
        }
        Ok(Self::Fuzzy)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Scope {
    ForMe,
    Assigned,
    Authored,
    ReviewRequested,
    Repository(Provider, RepositoryName),
    Direct(Locator),
}
impl Scope {
    pub fn supports(&self, provider: Provider) -> bool {
        match self {
            Self::ForMe | Self::Authored => true,
            Self::Assigned | Self::ReviewRequested => provider == Provider::Github,
            Self::Repository(p, _) => *p == provider,
            Self::Direct(locator) => locator.provider == provider,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PullRequest {
    pub locator: Locator,
    pub id: String,
    pub repository_id: String,
    pub title: String,
    pub author: String,
    pub branch: String,
    pub state: String,
}
impl PullRequest {
    pub fn label(&self) -> String {
        format!("{} · {}", self.locator.label(), self.title)
    }
    pub fn score(&self, query: &str) -> Option<(u8, usize)> {
        if let Ok(EntryInput::Number(number)) = EntryInput::parse(query) {
            return (number == self.locator.number).then_some((0, 0));
        }
        let query = query.trim().to_lowercase();
        if query.is_empty() {
            return Some((0, 0));
        }
        let text = format!(
            "{} {} {} {}",
            self.locator.repository, self.locator.number, self.title, self.branch
        )
        .to_lowercase();
        if let Some(offset) = text.find(&query) {
            return Some((0, offset));
        }
        let mut score = 0;
        for term in query.split_whitespace() {
            let mut remaining = text.as_str();
            for character in term.chars() {
                let offset = remaining.find(character)?;
                score += offset;
                remaining = &remaining[offset + character.len_utf8()..];
            }
        }
        Some((1, score))
    }
}
#[derive(Clone, Debug, Default)]
pub struct Batch {
    pub items: Vec<PullRequest>,
    /// Leading assigned/review-requested items in the combined For Me list.
    pub priority_count: usize,
    pub partial: bool,
    pub coverage: Vec<String>,
}
pub(crate) fn bounded_text(value: Option<&str>, limit: usize) -> Result<String, Error> {
    let text = value.ok_or(Error::InvalidResponse)?;
    if text.len() > limit || text.chars().any(char::is_control) {
        return Err(Error::InvalidResponse);
    }
    Ok(text.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_inputs_do_not_guess_repositories_or_accept_foreign_urls() {
        assert!(matches!(
            EntryInput::parse("#12").unwrap(),
            EntryInput::Number(12)
        ));
        assert!(matches!(
            EntryInput::parse("team/repo#12").unwrap(),
            EntryInput::Qualified { number: 12, .. }
        ));
        let EntryInput::Url(pr) =
            EntryInput::parse("https://github.com/team/repo/pull/12#discussion").unwrap()
        else {
            panic!()
        };
        assert_eq!(pr.url(), "https://github.com/team/repo/pull/12");
        for value in [
            "https://github.com.evil/team/repo/pull/12",
            "https://user:secret@github.com/team/repo/pull/12",
            "https://github.com/team/repo/pull/0",
            "http://github.com/team/repo/pull/12",
            "team/../repo#12",
            "https://github.com/team/repo/pull/12?token=secret",
        ] {
            assert!(EntryInput::parse(value).is_err(), "{value}");
        }
    }
    #[test]
    fn fuzzy_search_covers_branch_and_repository_and_numbers_are_exact() {
        let pr = PullRequest {
            id: "pr".into(),
            repository_id: "repo".into(),
            locator: Locator {
                provider: Provider::Github,
                repository: RepositoryName::parse("team/widgets").unwrap(),
                number: 123,
            },
            title: "Improve scrolling".into(),
            author: "person".into(),
            branch: "fix/momentum".into(),
            state: "open".into(),
        };
        assert!(pr.score("wdgts momentum").is_some());
        assert!(pr.score("#123").is_some());
        assert!(pr.score("12").is_none());
        assert!(pr.score("zzzzzz").is_none());
    }
}
