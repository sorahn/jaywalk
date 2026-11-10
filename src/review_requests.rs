//! Typed request identities keep results tied to their comparison and file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ComparisonRequest(u64);
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FileRequest {
    comparison: ComparisonRequest,
    file: u64,
}
#[derive(Default)]
pub struct RequestClock {
    comparison: ComparisonRequest,
    file: u64,
}
impl RequestClock {
    pub fn comparison(&self) -> ComparisonRequest {
        self.comparison
    }
    pub fn file(&self) -> FileRequest {
        FileRequest {
            comparison: self.comparison,
            file: self.file,
        }
    }
    pub fn invalidate_comparison(&mut self) {
        self.comparison.0 += 1;
    }
    pub fn invalidate_file(&mut self) {
        self.file += 1;
    }
}
#[cfg(test)]
mod tests {
    use super::RequestClock;
    #[test]
    fn interleaved_file_revision_and_back_requests_reject_late_results() {
        let mut requests = RequestClock::default();
        requests.invalidate_comparison();
        let revision_a = requests.comparison();
        requests.invalidate_file();
        let file_a = requests.file();
        requests.invalidate_file();
        let file_b = requests.file();
        assert_ne!(file_a, requests.file());
        assert_eq!(file_b, requests.file());
        requests.invalidate_comparison();
        let revision_b = requests.comparison();
        assert_ne!(
            file_b,
            requests.file(),
            "a new comparison invalidates files even before the next file request"
        );
        requests.invalidate_file();
        let next_file = requests.file();
        assert_ne!(revision_a, revision_b);
        assert_eq!(next_file, requests.file());
        requests.invalidate_comparison(); // Back restores a recorded view and cancels pending work.
        assert_ne!(revision_b, requests.comparison());
        assert_ne!(next_file, requests.file());
    }
}
