use super::{HistorySource, Revision};
use crate::accounts::Control;
use chrono::NaiveDate;
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchQuery {
    text: Vec<String>,
    authors: Vec<String>,
    after: Option<NaiveDate>,
    before: Option<NaiveDate>,
}
#[derive(Debug)]
pub struct SearchPage {
    pub matches: Vec<Revision>,
    /// Logical record offset in this immutable history source, not a match count.
    pub next_offset: Option<usize>,
    pub scanned: usize,
}
impl SearchQuery {
    pub fn parse(input: &str) -> Result<Self, String> {
        let mut tokens = Vec::new();
        let mut token = String::new();
        let mut quote = None;
        for ch in input.chars() {
            if quote == Some(ch) {
                quote = None;
            } else if quote.is_none()
                && (token.is_empty() || token.ends_with(':'))
                && matches!(ch, '\'' | '"')
            {
                quote = Some(ch);
            } else if quote.is_none() && ch.is_whitespace() {
                if !token.is_empty() {
                    tokens.push(std::mem::take(&mut token));
                }
            } else {
                token.push(ch);
            }
        }
        if quote.is_some() {
            return Err("Close the quoted search value".into());
        }
        if !token.is_empty() {
            tokens.push(token);
        }
        let mut query = Self {
            text: vec![],
            authors: vec![],
            after: None,
            before: None,
        };
        for token in tokens {
            if let Some(author) = token.strip_prefix("author:") {
                if author.is_empty() {
                    return Err("author: needs a name".into());
                }
                query.authors.push(author.to_lowercase());
            } else if let Some(value) = token
                .strip_prefix("after:")
                .or_else(|| token.strip_prefix("before:"))
            {
                if value.len() != 10 {
                    return Err("Dates use YYYY-MM-DD".into());
                }
                let date = NaiveDate::parse_from_str(value, "%Y-%m-%d")
                    .map_err(|_| "Invalid date; use YYYY-MM-DD")?;
                let field = if token.starts_with("after:") {
                    &mut query.after
                } else {
                    &mut query.before
                };
                if field.replace(date).is_some() {
                    return Err("Use only one after: and one before: date".into());
                }
            } else {
                query.text.push(token.to_lowercase());
            }
        }
        if query
            .after
            .zip(query.before)
            .is_some_and(|(after, before)| after >= before)
        {
            return Err("before: must be later than after:".into());
        }
        Ok(query)
    }
    pub fn matches(&self, revision: &Revision) -> bool {
        if self.text.is_empty()
            && self.authors.is_empty()
            && self.after.is_none()
            && self.before.is_none()
        {
            return true;
        }
        if self.after.is_some() || self.before.is_some() {
            let Some(date) = revision
                .timestamp
                .get(..10)
                .and_then(|date| NaiveDate::parse_from_str(date, "%Y-%m-%d").ok())
            else {
                return false;
            };
            if self.after.is_some_and(|after| date < after)
                || self.before.is_some_and(|before| date >= before)
            {
                return false;
            }
        }
        let author = revision.author.to_lowercase();
        if !self.authors.iter().all(|needle| author.contains(needle)) {
            return false;
        }
        if self.text.is_empty() {
            return true;
        }
        let message = if revision.description.is_empty() {
            revision.summary()
        } else {
            revision.description.clone()
        }
        .to_lowercase();
        self.text.iter().all(|needle| {
            if needle.len() >= 4 && needle.bytes().all(|b| b.is_ascii_hexdigit()) {
                return revision.commit_id.starts_with(needle)
                    || revision.change_id.starts_with(needle);
            }
            message.contains(needle)
                || author.contains(needle)
                || revision.commit_id.starts_with(needle)
                || revision.change_id.starts_with(needle)
                || revision
                    .references
                    .iter()
                    .any(|reference| reference.display_name().to_lowercase().contains(needle))
        })
    }
    pub fn in_memory(
        &self,
        revisions: &[Revision],
        offset: usize,
        size: usize,
        control: &Control,
    ) -> Result<SearchPage, String> {
        let mut page = SearchPage {
            matches: vec![],
            next_offset: None,
            scanned: offset,
        };
        for (index, revision) in revisions.iter().enumerate().skip(offset) {
            control.check().map_err(|e| e.to_string())?;
            if self.matches(revision) {
                if page.matches.len() >= size {
                    page.next_offset = Some(index);
                    break;
                }
                page.matches.push(revision.clone());
            }
            page.scanned = index + 1;
        }
        Ok(page)
    }
}
impl HistorySource {
    pub fn search(
        &self,
        query: &SearchQuery,
        offset: usize,
        size: usize,
        control: &Control,
    ) -> Result<SearchPage, String> {
        if !(1..=1000).contains(&size) {
            return Err("History search page size must be 1..1000".into());
        }
        let mut labels = HashMap::<&str, Vec<_>>::new();
        for reference in &self.references.references {
            labels.entry(&reference.target).or_default().push(reference);
        }
        let mut page = SearchPage {
            matches: Vec::new(),
            next_offset: None,
            scanned: offset,
        };
        self.scan(offset, None, control, |mut revision| {
            if let Some(references) = labels.get(revision.commit_id.as_str()) {
                revision.references = references
                    .iter()
                    .map(|reference| (*reference).clone())
                    .collect();
            }
            if query.matches(&revision) {
                if page.matches.len() >= size {
                    page.next_offset = Some(page.scanned);
                    return false;
                }
                page.matches.push(revision);
            }
            page.scanned += 1;
            true
        })?;
        Ok(page)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn revision() -> Revision {
        super::super::History::parse("[\"abcd1234\",\"xyzchange\",\"fix a crash\\n\\nUnicode café details\",\"Example Author\",[],[],false,false,\"2026-11-12 16:00:00 -0500\"]").unwrap().revisions.remove(0)
    }
    #[test]
    fn search_ids_messages_quoted_authors_and_calendar_dates() {
        let revision = revision();
        for text in [
            "abcd",
            "xyzc",
            "café",
            "\"fix a crash\" author:\"Example Author\" after:2026-11-12 before:2026-11-13",
        ] {
            assert!(
                SearchQuery::parse(text).unwrap().matches(&revision),
                "{text}"
            );
        }
        for text in [
            "author:another",
            "after:2026-11-13",
            "before:2026-11-12",
            "ffff",
        ] {
            assert!(!SearchQuery::parse(text).unwrap().matches(&revision));
        }
        for text in [
            "after:2026-02-30",
            "after:2026-11-12 before:2026-11-12",
            "\"incomplete",
            "author:",
        ] {
            assert!(SearchQuery::parse(text).is_err());
        }
    }
}
