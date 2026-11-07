use similar::{DiffTag, TextDiff};
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

fn line_offsets(text: &str) -> Vec<usize> {
    let mut offsets = vec![0];
    for line in text.split_inclusive('\n') {
        offsets.push(offsets.last().copied().unwrap() + line.len());
    }
    offsets
}

impl FileDiff {
    pub fn new(before: &str, after: &str) -> Self {
        let diff = TextDiff::from_lines(before, after);
        let blocks: Vec<Correspondence> = diff
            .ops()
            .iter()
            .map(|operation| Correspondence {
                before: operation.old_range(),
                after: operation.new_range(),
                changed: operation.tag() != DiffTag::Equal,
            })
            .collect();
        let (before_words, after_words) = changed_words(before, after, &blocks);
        Self {
            blocks,
            before_words,
            after_words,
            before_offsets: line_offsets(before),
            after_offsets: line_offsets(after),
        }
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
                return if !block.changed {
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
) -> (Vec<Range<usize>>, Vec<Range<usize>>) {
    let old = line_offsets(before);
    let new = line_offsets(after);
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
