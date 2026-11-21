//! Search every captured path, but only clone and mount the best bounded rows.
use super::*;
use jaywalk::repository::ComparisonIdentity;
use std::collections::{BinaryHeap, VecDeque};

pub(super) const ROW_LIMIT: usize = 200;
struct Cached {
    source: ComparisonIdentity,
    query: String,
    rows: Rc<Vec<(String, Choice)>>,
    total: usize,
}
#[derive(Default)]
pub(super) struct FilePicker {
    cache: VecDeque<Cached>,
    pub(super) shown: usize,
    pub(super) total: usize,
    #[cfg(test)]
    pub(super) searches: usize,
}

fn best_paths<'a>(
    paths: impl Iterator<Item = (&'a str, Option<&'a str>)>,
    query: &str,
) -> (usize, Vec<(String, Option<String>)>) {
    let needles: Vec<char> = query
        .to_lowercase()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    let mut heap = BinaryHeap::new();
    let mut total = 0;
    for (path, status) in paths {
        let score = if needles.is_empty() {
            Some(0)
        } else {
            let lowercase = path.to_lowercase();
            let mut rest = lowercase.as_str();
            let mut score = 0;
            let mut matched = true;
            for needle in &needles {
                match rest.find(*needle) {
                    Some(offset) => {
                        score += offset;
                        rest = &rest[offset + needle.len_utf8()..];
                    }
                    None => {
                        matched = false;
                        break;
                    }
                }
            }
            matched.then_some(score)
        };
        let Some(score) = score else {
            continue;
        };
        total += 1;
        let candidate = (score, status.is_none(), path, status);
        if heap.len() < ROW_LIMIT {
            heap.push(candidate);
        } else if heap.peek().is_some_and(|worst| candidate < *worst) {
            *heap
                .peek_mut()
                .expect("a full nonzero heap has a worst entry") = candidate;
        }
    }
    let rows = heap
        .into_sorted_vec()
        .into_iter()
        .map(|(_, _, path, status)| (path.to_owned(), status.map(str::to_owned)))
        .collect();
    (total, rows)
}

impl Viewer {
    pub(in crate::viewer::palette) fn file_palette_choices(
        &mut self,
        query: &str,
    ) -> Vec<(String, Choice)> {
        let Some(comparison) = self
            .comparison()
            .filter(|_| self.page == pages::Page::Comparison)
        else {
            self.palette.files.shown = 0;
            self.palette.files.total = 0;
            return Vec::new();
        };
        let source = comparison.identity();
        if let Some(cached) = self
            .palette
            .files
            .cache
            .iter()
            .find(|cached| cached.source == source && cached.query == query)
        {
            self.palette.files.shown = cached.rows.len();
            self.palette.files.total = cached.total;
            return cached.rows.as_ref().clone();
        }
        let (total, paths) = best_paths(
            comparison
                .catalog()
                .entries()
                .map(|entry| (entry.path(), entry.status())),
            query,
        );
        let identity = Rc::new(source.clone());
        let rows: Rc<Vec<_>> = Rc::new(
            paths
                .into_iter()
                .map(|(path, status)| {
                    (
                        path.clone(),
                        Choice::File {
                            path,
                            status,
                            source: identity.clone(),
                        },
                    )
                })
                .collect(),
        );
        self.palette.files.shown = rows.len();
        self.palette.files.total = total;
        #[cfg(test)]
        {
            self.palette.files.searches += 1;
        }
        self.palette
            .files
            .cache
            .retain(|entry| entry.source == source);
        self.palette.files.cache.push_front(Cached {
            source,
            query: query.to_owned(),
            rows: rows.clone(),
            total,
        });
        // Reopening the empty query stays warm after a search; retain no file text.
        if self.palette.files.cache.len() > 4 {
            let oldest_search = self
                .palette
                .files
                .cache
                .iter()
                .rposition(|entry| !entry.query.is_empty())
                .expect("at most one cached query is empty for a comparison");
            self.palette.files.cache.remove(oldest_search);
        }
        rows.as_ref().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::{ROW_LIMIT, best_paths};
    #[test]
    fn search_is_bounded_but_can_find_paths_beyond_the_initial_rows() {
        let paths: Vec<_> = (0..50_000).map(|i| format!("src/file-{i:05}.rs")).collect();
        let (total, rows) = best_paths(paths.iter().map(|path| (path.as_str(), None)), "");
        assert_eq!(total, 50_000);
        assert_eq!(rows.len(), ROW_LIMIT);
        let (total, rows) =
            best_paths(paths.iter().map(|path| (path.as_str(), None)), "file-49999");
        assert_eq!(total, 1);
        assert_eq!(rows[0].0, "src/file-49999.rs");
    }
    #[test]
    fn relevance_precedes_status_and_unicode_queries_remain_literal() {
        let paths = [
            ("z/needle.rs", Some("modified")),
            ("needle.rs", None),
            ("β/État.rs", None),
        ];
        let (_, rows) = best_paths(paths.into_iter(), "needle");
        assert_eq!(rows[0].0, "needle.rs");
        let (_, rows) = best_paths(paths.into_iter(), " ÉT ");
        assert_eq!(rows[0].0, "β/État.rs");
    }
}
