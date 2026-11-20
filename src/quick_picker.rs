//! Prefix parsing and bounded command-use ranking, independent of any window.
use crate::commands::CommandId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Files,
    Commands,
    Line,
    Repositories,
    PullRequests,
    Help,
    History,
    FileSymbols,
    ProjectSymbols,
}
impl Mode {
    pub const ALL: [Self; 9] = [
        Self::Files,
        Self::Commands,
        Self::Line,
        Self::Repositories,
        Self::PullRequests,
        Self::Help,
        Self::History,
        Self::FileSymbols,
        Self::ProjectSymbols,
    ];
    pub fn prefix(self) -> &'static str {
        match self {
            Self::Files => "",
            Self::Commands => ">",
            Self::Line => ":",
            Self::Repositories => "~",
            Self::PullRequests => "!",
            Self::Help => "?",
            Self::History => "/",
            Self::FileSymbols => "@",
            Self::ProjectSymbols => "#",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Files => "Files",
            Self::Commands => "Commands",
            Self::Line => "Go to line",
            Self::Repositories => "Recent repositories",
            Self::PullRequests => "Pull requests",
            Self::Help => "Modes and prefixes",
            Self::History => "Find in history",
            Self::FileSymbols => "File symbols",
            Self::ProjectSymbols => "Project symbols",
        }
    }
    pub fn parse(input: &str) -> (Self, &str) {
        for mode in Self::ALL.into_iter().skip(1) {
            if let Some(query) = input.strip_prefix(mode.prefix()) {
                return (mode, query.trim_start());
            }
        }
        (Self::Files, input.strip_prefix("./").unwrap_or(input))
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CommandUsage {
    sequence: u64,
    entries: BTreeMap<String, Use>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
struct Use {
    count: u32,
    last: u64,
}
impl CommandUsage {
    pub fn record(&mut self, command: CommandId) {
        self.sequence = self.sequence.saturating_add(1);
        let entry = self
            .entries
            .entry(command.key())
            .or_insert(Use { count: 0, last: 0 });
        entry.count = entry.count.saturating_add(1);
        entry.last = self.sequence;
        while self.entries.len() > 256 {
            let oldest = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.last)
                .map(|(key, _)| key.clone())
                .expect("nonempty usage");
            self.entries.remove(&oldest);
        }
    }
    pub fn score(&self, command: CommandId) -> u64 {
        self.entries.get(&command.key()).map_or(0, |entry| {
            let age = self.sequence.saturating_sub(entry.last).min(64);
            (64 - age) * 2 + u64::from(entry.count.ilog2() + 1) * 8
        })
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > 65536 {
            return Err("Command usage exceeds 64 KiB".into());
        }
        let value: Self = serde_json::from_slice(bytes)
            .map_err(|e| format!("Could not read command usage: {e}"))?;
        if value.entries.len() > 256
            || value
                .entries
                .values()
                .any(|entry| entry.count == 0 || entry.last > value.sequence)
        {
            return Err("Invalid command usage data".into());
        }
        Ok(value)
    }
}

/// Normalize a command query once; compute one complete sort key per label.
pub struct CommandQuery {
    literal: String,
    needles: Vec<char>,
}
impl CommandQuery {
    pub fn new(query: &str) -> Self {
        let literal = query.trim().to_lowercase();
        let needles = literal.chars().filter(|ch| !ch.is_whitespace()).collect();
        Self { literal, needles }
    }
    /// Text relevance takes priority; usage can reorder nearby fuzzy scores.
    pub fn rank(
        &self,
        label: &str,
        usage: u64,
    ) -> Option<(u8, usize, std::cmp::Reverse<u64>, usize)> {
        let label = label.to_lowercase();
        let mut remaining = label.as_str();
        let mut score = 0;
        for needle in &self.needles {
            let offset = remaining.find(*needle)?;
            score += offset;
            remaining = &remaining[offset + needle.len_utf8()..];
        }
        let category = if self.literal.is_empty() || label == self.literal {
            0
        } else if label.starts_with(&self.literal) {
            1
        } else if label.contains(&self.literal) {
            2
        } else {
            3
        };
        Some((category, score / 8, std::cmp::Reverse(usage), score))
    }
}

#[cfg(test)]
mod tests {
    use super::{CommandQuery, CommandUsage, Mode};
    use crate::commands::CommandId;
    #[test]
    fn prefixes_are_editable_and_literal_paths_can_escape_them() {
        for mode in Mode::ALL {
            let input = format!("{}query", mode.prefix());
            assert_eq!(Mode::parse(&input), (mode, "query"));
        }
        assert_eq!(
            Mode::parse("./!important.rs"),
            (Mode::Files, "!important.rs")
        );
        assert_eq!(Mode::parse("src/@file.rs"), (Mode::Files, "src/@file.rs"));
        assert_eq!(Mode::parse("! #32"), (Mode::PullRequests, "#32"));
    }
    #[test]
    fn usage_survives_roundtrip_and_labels_do_not_affect_it() {
        let mut usage = CommandUsage::default();
        usage.record(CommandId::Refresh);
        usage.record(CommandId::Overview);
        usage.record(CommandId::Refresh);
        let bytes = serde_json::to_vec(&usage).unwrap();
        let restored = CommandUsage::decode(&bytes).unwrap();
        assert!(restored.score(CommandId::Refresh) > restored.score(CommandId::Overview));
        assert_eq!(restored.score(CommandId::About), 0);
        assert!(CommandQuery::new("refresh").rank("About", 999).is_none());
        assert!(
            CommandQuery::new("refresh").rank("Refresh", 0).unwrap()
                < CommandQuery::new("refresh")
                    .rank("Refresh review status", 999)
                    .unwrap()
        );
        assert!(
            CommandQuery::new("ref").rank("Refresh", 0).unwrap()
                < CommandQuery::new("ref")
                    .rank("Another refresh", 999)
                    .unwrap()
        );
        assert!(
            CommandQuery::new("").rank("Z", 10).unwrap()
                < CommandQuery::new("").rank("A", 0).unwrap()
        );
        assert!(CommandUsage::decode(&vec![b' '; 65537]).is_err());
        assert!(
            CommandUsage::decode(br#"{"sequence":1,"entries":{"refresh":{"count":0,"last":1}}}"#)
                .is_err()
        );
    }
}

/// Fuzzy match spans use original UTF-8 byte boundaries even when case folding expands.
pub fn match_ranges(label: &str, query: &str) -> Vec<std::ops::Range<usize>> {
    let letters: Vec<_> = label
        .char_indices()
        .flat_map(|(start, ch)| {
            ch.to_lowercase()
                .map(move |folded| (folded, start..start + ch.len_utf8()))
        })
        .collect();
    let mut next = 0;
    let mut ranges: Vec<std::ops::Range<usize>> = Vec::new();
    for needle in query
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .flat_map(char::to_lowercase)
    {
        let Some(offset) = letters[next..].iter().position(|(ch, _)| *ch == needle) else {
            return vec![];
        };
        next += offset;
        let range = letters[next].1.clone();
        if let Some(last) = ranges.last_mut()
            && last.end >= range.start
        {
            last.end = last.end.max(range.end);
        } else {
            ranges.push(range);
        }
        next += 1;
    }
    ranges
}

#[cfg(test)]
mod highlight_tests {
    use super::match_ranges;
    #[test]
    fn fuzzy_highlights_stay_on_original_unicode_boundaries() {
        let label = "İ漢🙂file.rs";
        assert_eq!(match_ranges(label, "i漢f"), vec![0..5, 9..10]);
        assert_eq!(match_ranges(label, "i̇"), vec![0..2]);
        assert!(match_ranges(label, "missing").is_empty());
        assert!(match_ranges(label, "").is_empty());
    }
}
