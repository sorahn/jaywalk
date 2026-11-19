//! Comparison keys affect alignment only. Source bytes and line addresses remain
//! authoritative, including rows suppressed by an ignore policy.
use crate::diff_model::Algorithm;
use std::borrow::Cow;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Whitespace {
    #[default]
    Exact,
    IgnoreTrailing,
    IgnoreChanges,
    IgnoreAll,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ComparisonOptions {
    pub whitespace: Whitespace,
    pub ignore_line_endings: bool,
    pub ignore_blank_lines: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineAlignment {
    pub before: Option<usize>,
    pub after: Option<usize>,
    pub changed: bool,
}
impl ComparisonOptions {
    fn key(self, line: &str) -> Cow<'_, str> {
        if self.whitespace == Whitespace::Exact && !self.ignore_line_endings {
            return Cow::Borrowed(line);
        }
        let (body, ending) = if let Some(s) = line.strip_suffix("\r\n") {
            (
                s,
                if self.ignore_line_endings {
                    "\n"
                } else {
                    "\r\n"
                },
            )
        } else if let Some(s) = line.strip_suffix('\n') {
            (s, "\n")
        } else {
            (line, "")
        };
        let mut key = match self.whitespace {
            Whitespace::Exact => body.to_owned(),
            Whitespace::IgnoreTrailing => body.trim_end_matches([' ', '\t']).to_owned(),
            Whitespace::IgnoreAll => body.chars().filter(|c| !matches!(c, ' ' | '\t')).collect(),
            Whitespace::IgnoreChanges => {
                let mut s = String::new();
                let mut space = false;
                for c in body.trim_end_matches([' ', '\t']).chars() {
                    if matches!(c, ' ' | '\t') {
                        if !space {
                            s.push(' ');
                        }
                        space = true;
                    } else {
                        s.push(c);
                        space = false;
                    }
                }
                s
            }
        };
        key.push_str(ending);
        Cow::Owned(key)
    }
    pub fn label(self) -> Option<String> {
        let mut parts = Vec::new();
        match self.whitespace {
            Whitespace::Exact => {}
            Whitespace::IgnoreTrailing => parts.push("Ignoring trailing whitespace"),
            Whitespace::IgnoreChanges => parts.push("Ignoring whitespace amount"),
            Whitespace::IgnoreAll => parts.push("Ignoring whitespace"),
        }
        if self.ignore_line_endings {
            parts.push("Ignoring EOL");
        }
        if self.ignore_blank_lines {
            parts.push("Ignoring blank lines");
        }
        (!parts.is_empty()).then(|| parts.join(" · "))
    }
    pub fn alignment(self, before: &str, after: &str, algorithm: Algorithm) -> Vec<LineAlignment> {
        let old: Vec<_> = before.split_inclusive('\n').collect();
        let new: Vec<_> = after.split_inclusive('\n').collect();
        let keep = |line: &&str| {
            !self.ignore_blank_lines || !line.trim_matches([' ', '\t', '\r', '\n']).is_empty()
        };
        let left: Vec<_> = old
            .iter()
            .enumerate()
            .filter(|(_, s)| keep(s))
            .map(|(i, s)| (i, self.key(s)))
            .collect();
        let right: Vec<_> = new
            .iter()
            .enumerate()
            .filter(|(_, s)| keep(s))
            .map(|(i, s)| (i, self.key(s)))
            .collect();
        let left_keys: Vec<_> = left.iter().map(|(_, s)| s.as_ref()).collect();
        let right_keys: Vec<_> = right.iter().map(|(_, s)| s.as_ref()).collect();
        let diff = similar::TextDiff::configure()
            .algorithm(algorithm.similar())
            .diff_slices(&left_keys, &right_keys);
        let mut rows = Vec::new();
        let (mut a, mut b) = (0, 0);
        for op in diff.ops() {
            let l = op.old_range();
            let r = op.new_range();
            for n in 0..l.len().max(r.len()) {
                let x = (n < l.len()).then(|| left[l.start + n].0);
                let y = (n < r.len()).then(|| right[r.start + n].0);
                // Emit any excluded blank rows before the next retained row.
                while x.is_some_and(|x| a < x) || y.is_some_and(|y| b < y) {
                    let oa = x.filter(|x| a < *x).map(|_| {
                        let i = a;
                        a += 1;
                        i
                    });
                    let nb = y.filter(|y| b < *y).map(|_| {
                        let i = b;
                        b += 1;
                        i
                    });
                    rows.push(LineAlignment {
                        before: oa,
                        after: nb,
                        changed: false,
                    });
                }
                rows.push(LineAlignment {
                    before: x,
                    after: y,
                    changed: op.tag() != similar::DiffTag::Equal,
                });
                if x.is_some() {
                    a += 1;
                }
                if y.is_some() {
                    b += 1;
                }
            }
        }
        while a < old.len() || b < new.len() {
            let x = (a < old.len()).then(|| {
                let i = a;
                a += 1;
                i
            });
            let y = (b < new.len()).then(|| {
                let i = b;
                b += 1;
                i
            });
            rows.push(LineAlignment {
                before: x,
                after: y,
                changed: false,
            });
        }
        rows
    }
}
