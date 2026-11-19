/// Line matching is a display policy; it never changes comparison endpoints.
#[derive(Clone, Copy, Debug, Default, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Algorithm {
    #[default]
    Myers,
    Patience,
}
impl Algorithm {
    pub fn similar(self) -> similar::Algorithm {
        match self {
            Self::Myers => similar::Algorithm::Myers,
            Self::Patience => similar::Algorithm::Patience,
        }
    }
}
use similar::TextDiff;
use std::ops::Range;

#[derive(Clone, Debug)]
pub struct Correspondence {
    pub before: Range<usize>,
    pub after: Range<usize>,
    pub changed: bool,
}

#[derive(Clone, Debug)]
pub struct FileDiff {
    pub blocks: Vec<Correspondence>,
    pub before_offsets: Vec<usize>,
    pub after_offsets: Vec<usize>,
    pub before_words: Vec<Range<usize>>,
    pub after_words: Vec<Range<usize>>,
}

// The editor and Git patch records use LF boundaries. A CR is preserved as
// source content (including the CR in CRLF), not treated as an extra row.
fn source_lines(text: &str) -> Vec<&str> {
    text.split_inclusive('\n').collect()
}
fn line_offsets(lines: &[&str]) -> Vec<usize> {
    let mut offsets = vec![0];
    for line in lines {
        offsets.push(offsets.last().copied().unwrap() + line.len());
    }
    offsets
}

impl FileDiff {
    pub fn new(before: &str, after: &str) -> Self {
        Self::with_algorithm(before, after, Algorithm::default())
    }
    pub fn with_algorithm(before: &str, after: &str, algorithm: Algorithm) -> Self {
        Self::with_options(before, after, algorithm, Default::default())
    }
    pub fn with_options(
        before: &str,
        after: &str,
        algorithm: Algorithm,
        options: crate::diff_options::ComparisonOptions,
    ) -> Self {
        let old = source_lines(before);
        let new = source_lines(after);
        let before_offsets = line_offsets(&old);
        let after_offsets = line_offsets(&new);
        let blocks = if options == Default::default() {
            line_diff(&old, &new, algorithm)
                .ops()
                .iter()
                .map(|op| Correspondence {
                    before: op.old_range(),
                    after: op.new_range(),
                    changed: op.tag() != similar::DiffTag::Equal,
                })
                .collect()
        } else {
            let rows = options.alignment(before, after, algorithm);
            let mut blocks: Vec<Correspondence> = Vec::new();
            let (mut a, mut b) = (0, 0);
            for row in rows {
                let next = Correspondence {
                    before: a..a + usize::from(row.before.is_some()),
                    after: b..b + usize::from(row.after.is_some()),
                    changed: row.changed,
                };
                a = next.before.end;
                b = next.after.end;
                if let Some(last) = blocks.last_mut()
                    && last.changed == next.changed
                    && (next.changed
                        || (last.before.is_empty() == next.before.is_empty()
                            && last.after.is_empty() == next.after.is_empty()))
                {
                    last.before.end = next.before.end;
                    last.after.end = next.after.end;
                } else {
                    blocks.push(next);
                }
            }
            blocks
        };
        let (before_words, after_words) =
            changed_words(before, after, &blocks, &before_offsets, &after_offsets);
        Self {
            blocks,
            before_words,
            after_words,
            before_offsets,
            after_offsets,
        }
    }

    pub fn line_alignment(&self) -> Vec<crate::diff_options::LineAlignment> {
        self.blocks
            .iter()
            .flat_map(|b| {
                (0..b.before.len().max(b.after.len())).map(move |n| {
                    crate::diff_options::LineAlignment {
                        before: (n < b.before.len()).then_some(b.before.start + n),
                        after: (n < b.after.len()).then_some(b.after.start + n),
                        changed: b.changed,
                    }
                })
            })
            .collect()
    }
    /// Map a source line to its counterpart, without treating insertions as padding.
    pub fn corresponding_line(&self, line: usize, before: bool) -> usize {
        for block in &self.blocks {
            let (source, target) = if before {
                (&block.before, &block.after)
            } else {
                (&block.after, &block.before)
            };
            if source.contains(&line) {
                return if !block.changed && source.len() == target.len() {
                    target.start + line - source.start
                } else if target.is_empty() {
                    target.start
                } else {
                    target.start
                        + ((line - source.start) * target.len() / source.len().max(1))
                            .min(target.len() - 1)
                };
            }
        }
        if before {
            self.after_offsets.len().saturating_sub(2)
        } else {
            self.before_offsets.len().saturating_sub(2)
        }
    }

    pub fn byte_range(&self, block: &Correspondence, before: bool) -> Range<usize> {
        let (offsets, range) = if before {
            (&self.before_offsets, &block.before)
        } else {
            (&self.after_offsets, &block.after)
        };
        offsets[range.start]..offsets[range.end]
    }
}

fn line_diff<'old, 'new, 'bufs>(
    old: &'bufs [&'old str],
    new: &'bufs [&'new str],
    algorithm: Algorithm,
) -> TextDiff<'old, 'new, 'bufs, str> {
    TextDiff::configure()
        .algorithm(algorithm.similar())
        .newline_terminated(true)
        .diff_slices(old, new)
}

/// Full-context patch with LF record boundaries and exact original source bytes.
/// `similar`'s default tokenizer/formatter also treats bare CR as a newline;
/// feeding it LF slices alone does not fix the missing-final-newline marker.
pub fn full_patch(
    before: &str,
    after: &str,
    algorithm: Algorithm,
    old_path: &str,
    new_path: &str,
) -> String {
    let old = source_lines(before);
    let new = source_lines(after);
    let diff = line_diff(&old, &new, algorithm);
    let mut patch = String::new();
    for hunk in diff
        .unified_diff()
        .context_radius(old.len().max(new.len()))
        .iter_hunks()
    {
        if patch.is_empty() {
            patch.push_str(&format!("--- {old_path}\n+++ {new_path}\n"));
        }
        patch.push_str(&format!("{}\n", hunk.header()));
        for change in hunk.iter_changes() {
            patch.push(match change.tag() {
                similar::ChangeTag::Equal => ' ',
                similar::ChangeTag::Delete => '-',
                similar::ChangeTag::Insert => '+',
            });
            patch.push_str(change.value());
            if !change.value().ends_with('\n') {
                patch.push_str("\n\\ No newline at end of file\n");
            }
        }
    }
    patch
}

fn word_tokens(text: &str) -> Vec<&str> {
    let mut starts = vec![0];
    let mut previous = None;
    for (offset, character) in text.char_indices() {
        let class = if character.is_alphanumeric() || character == '_' {
            0
        } else if character.is_whitespace() {
            1
        } else {
            2
        };
        if offset > 0 && (previous != Some(class) || class == 2) {
            starts.push(offset);
        }
        previous = Some(class);
    }
    starts.push(text.len());
    starts
        .windows(2)
        .filter_map(|range| (range[0] < range[1]).then_some(&text[range[0]..range[1]]))
        .collect()
}

// Limit token comparisons per paired line. Insertions/deletions already have
// full-row emphasis; unlike text edits they need no additional word shading.
fn changed_words(
    before: &str,
    after: &str,
    blocks: &[Correspondence],
    old: &[usize],
    new: &[usize],
) -> (Vec<Range<usize>>, Vec<Range<usize>>) {
    let mut removed = Vec::new();
    let mut added = Vec::new();
    for block in blocks.iter().filter(|block| block.changed) {
        for (a, b) in block.before.clone().zip(block.after.clone()) {
            let left = &before[old[a]..old[a + 1]];
            let right = &after[new[b]..new[b + 1]];
            if left.len().max(right.len()) > 1000 {
                continue;
            }
            let left_tokens = word_tokens(left);
            let right_tokens = word_tokens(right);
            let diff = TextDiff::from_slices(&left_tokens, &right_tokens);
            if diff.ratio() < 0.25 {
                continue;
            }
            let mut left_offset = old[a];
            let mut right_offset = new[b];
            for change in diff.iter_all_changes() {
                let len = change.value().len();
                match change.tag() {
                    similar::ChangeTag::Equal => {
                        left_offset += len;
                        right_offset += len;
                    }
                    similar::ChangeTag::Delete => {
                        if !change.value().trim().is_empty() {
                            removed.push(left_offset..left_offset + len);
                        }
                        left_offset += len;
                    }
                    similar::ChangeTag::Insert => {
                        if !change.value().trim().is_empty() {
                            added.push(right_offset..right_offset + len);
                        }
                        right_offset += len;
                    }
                }
            }
        }
    }
    (removed, added)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reading_line_correspondence_accounts_for_insertions() {
        let diff = FileDiff::new("a\nb\nc\n", "inserted\na\nb\nc\n");
        assert_eq!(diff.corresponding_line(2, false), 1);
        assert_eq!(diff.corresponding_line(1, true), 2);
        assert_eq!(FileDiff::new("", "added").corresponding_line(0, false), 0);
    }

    #[test]
    fn word_spans_are_utf8_boundaries_and_leave_unchanged_words_alone() {
        let before = "const café = oldValue;\n";
        let after = "const café = newValue;\nextra\n";
        let diff = FileDiff::new(before, after);
        assert_eq!(
            diff.before_words
                .iter()
                .map(|range| &before[range.clone()])
                .collect::<Vec<_>>(),
            ["oldValue"]
        );
        assert_eq!(
            diff.after_words
                .iter()
                .map(|range| &after[range.clone()])
                .collect::<Vec<_>>(),
            ["newValue"]
        );
        assert!(FileDiff::new("", "added").after_words.is_empty());
    }

    #[test]
    fn keeps_unicode_byte_ranges_and_zero_length_sides() {
        let before = "héllo\nend\n";
        let after = "héllo\nnew\nend\n";

        let diff = FileDiff::new(before, after);
        let insertion = diff.blocks.iter().find(|block| block.changed).unwrap();

        assert_eq!(insertion.before, 1..1);
        assert_eq!(&after[diff.byte_range(insertion, false)], "new\n");
        assert_eq!(diff.byte_range(insertion, true), 7..7);
    }

    #[test]
    fn handles_empty_files_and_unterminated_final_lines() {
        let diff = FileDiff::new("", "added");
        let block = &diff.blocks[0];

        assert_eq!(diff.byte_range(block, true), 0..0);
        assert_eq!(diff.byte_range(block, false), 0..5);
    }
}
