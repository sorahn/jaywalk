//! Conservative presentation-only inference over already captured text. No index,
//! object-store or filesystem writes, and no further reads of mutable files.
use super::{ChangeKind, ChangedFile, FileKind, GitValue, TextPair};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

const MAX_CANDIDATES: usize = 200;
const MAX_EXACT_BYTES: usize = 64 * 1024 * 1024;
const MAX_SIMILARITY_FILE_BYTES: usize = 64 * 1024;
const MAX_SIMILARITY_BYTES: usize = 8 * 1024 * 1024;
const MAX_PAIRS: usize = 20_000;
const MIN_SCORE: usize = 600; // shared line bytes / larger file size, in thousandths
const MAX_COMPETING_SCORE: usize = 300;
const MAX_SIMILARITY_LINES: usize = 65_536;
const MAX_LINE_COMPARISONS: usize = 1_000_000;

struct Candidate<'a> {
    path: &'a str,
    text: &'a str,
    kind: &'a FileKind,
}
impl Candidate<'_> {
    fn digest(&self) -> [u8; 32] {
        Sha256::digest(self.text.as_bytes()).into()
    }
}

pub(super) fn infer(
    files: &mut Vec<ChangedFile>,
    contents: &mut BTreeMap<String, TextPair>,
    trees: [&BTreeMap<String, GitValue>; 2],
) {
    let collect = |kind: ChangeKind, side: usize| -> Vec<Candidate<'_>> {
        files
            .iter()
            .filter(|f| f.kind() == &kind)
            .filter_map(|f| {
                let pair = contents.get(f.after_path())?.as_ref().ok()?;
                let text = if side == 0 { &pair.0 } else { &pair.1 };
                let entry = trees[side].get(if side == 0 {
                    f.before_path()
                } else {
                    f.after_path()
                })?;
                (!text.is_empty() && matches!(entry.kind, FileKind::Regular | FileKind::Symlink))
                    .then_some(Candidate {
                        path: f.after_path(),
                        text,
                        kind: &entry.kind,
                    })
            })
            .collect()
    };
    let old = collect(ChangeKind::Removed, 0);
    let new = collect(ChangeKind::Added, 1);
    if old.len() > MAX_CANDIDATES
        || new.len() > MAX_CANDIDATES
        || old.iter().chain(&new).map(|c| c.text.len()).sum::<usize>() > MAX_EXACT_BYTES
    {
        return;
    }
    let matches = matches(&old, &new);
    let pairs: Vec<_> = matches
        .into_iter()
        .map(|(i, j)| (old[i].path.to_owned(), new[j].path.to_owned()))
        .collect();
    // End all borrowed source views before moving the captured strings.
    for (before, after) in pairs {
        let (left, _) = contents
            .remove(&before)
            .expect("matched deletion has captured text")
            .expect("matched text is readable");
        let (_, right) = contents
            .remove(&after)
            .expect("matched addition has captured text")
            .expect("matched text is readable");
        contents.insert(after.clone(), Ok((left, right)));
        files.retain(|f| f.after_path() != before && f.after_path() != after);
        files.push(ChangedFile::new(ChangeKind::Renamed, before, after));
    }
    files.sort_by(|a, b| a.after_path().cmp(b.after_path()));
}

fn matches(old: &[Candidate<'_>], new: &[Candidate<'_>]) -> Vec<(usize, usize)> {
    let mut old_groups: BTreeMap<_, Vec<_>> = BTreeMap::new();
    let mut new_groups: BTreeMap<_, Vec<_>> = BTreeMap::new();
    for (i, c) in old.iter().enumerate() {
        old_groups.entry(c.digest()).or_default().push(i);
    }
    for (i, c) in new.iter().enumerate() {
        new_groups.entry(c.digest()).or_default().push(i);
    }
    let mut result = vec![];
    let mut reserved_old: BTreeSet<usize> = BTreeSet::new();
    let mut reserved_new: BTreeSet<usize> = BTreeSet::new();
    for (digest, left) in &old_groups {
        if let Some(right) = new_groups.get(digest) {
            // Duplicate exact candidates remain ambiguous, including during the
            // approximate pass. Never let filename order decide their pairing.
            reserved_old.extend(left.iter().copied());
            reserved_new.extend(right.iter().copied());
            if let ([i], [j]) = (left.as_slice(), right.as_slice())
                && old[*i].kind == new[*j].kind
                && old[*i].text == new[*j].text
            {
                result.push((*i, *j));
            }
        }
    }
    let eligible = |c: &Candidate<'_>| {
        c.kind == &FileKind::Regular && c.text.len() <= MAX_SIMILARITY_FILE_BYTES
    };
    let left: Vec<_> = old
        .iter()
        .enumerate()
        .filter(|(i, c)| !reserved_old.contains(i) && eligible(c))
        .collect();
    let right: Vec<_> = new
        .iter()
        .enumerate()
        .filter(|(i, c)| !reserved_new.contains(i) && eligible(c))
        .collect();
    if left.len() * right.len() > MAX_PAIRS
        || left
            .iter()
            .chain(&right)
            .map(|(_, c)| c.text.len())
            .sum::<usize>()
            > MAX_SIMILARITY_BYTES
    {
        return result;
    }
    if left
        .iter()
        .chain(&right)
        .map(|(_, c)| c.text.split_inclusive('\n').count())
        .sum::<usize>()
        > MAX_SIMILARITY_LINES
    {
        return result;
    }
    let left_lines: Vec<_> = left.iter().map(|(_, c)| lines(c.text)).collect();
    let right_lines: Vec<_> = right.iter().map(|(_, c)| lines(c.text)).collect();
    if left_lines.iter().map(BTreeMap::len).sum::<usize>() * right.len() > MAX_LINE_COMPARISONS {
        return result;
    }
    let scores: Vec<Vec<_>> = left
        .iter()
        .enumerate()
        .map(|(i, (_, a))| {
            right
                .iter()
                .enumerate()
                .map(|(j, (_, b))| {
                    let max = a.text.len().max(b.text.len());
                    if a.text.len().min(b.text.len()) * 1000 < max * MAX_COMPETING_SCORE {
                        return 0;
                    }
                    let shared = left_lines[i]
                        .iter()
                        .map(|(line, n)| {
                            line.len() * (*n).min(*right_lines[j].get(line).unwrap_or(&0))
                        })
                        .sum::<usize>();
                    shared * 1000 / max
                })
                .collect()
        })
        .collect();
    for (i, row) in scores.iter().enumerate() {
        if let Some(j) = clear_best(row.iter().copied())
            && clear_best(scores.iter().map(|r| r[j])) == Some(i)
        {
            result.push((left[i].0, right[j].0));
        }
    }
    result
}
fn lines(text: &str) -> BTreeMap<&str, usize> {
    let mut result = BTreeMap::new();
    for line in text.split_inclusive('\n') {
        *result.entry(line).or_default() += 1;
    }
    result
}
/// A candidate must be a clear, mutual best match, not just above a threshold.
fn clear_best(scores: impl Iterator<Item = usize>) -> Option<usize> {
    let mut best = (0, 0);
    let mut second = 0;
    for (i, score) in scores.enumerate() {
        if score > best.1 {
            second = best.1;
            best = (i, score);
        } else {
            second = second.max(score);
        }
    }
    (best.1 >= MIN_SCORE && second < MAX_COMPETING_SCORE).then_some(best.0)
}
