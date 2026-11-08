//! Delay visible feedback without delaying the operation or its result.
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LoadingKind {
    Comparison,
    File,
    Evolution,
    Live,
}

#[derive(Default)]
pub struct LoadingFeedback {
    started: HashMap<LoadingKind, Instant>,
}

impl LoadingFeedback {
    pub fn begin(&mut self, kind: LoadingKind, now: Instant) {
        self.started.insert(kind, now);
    }
    pub fn finish(&mut self, kind: LoadingKind) {
        self.started.remove(&kind);
    }
    pub fn started(&self, kind: LoadingKind) -> Option<Instant> {
        self.started.get(&kind).copied()
    }
    pub fn remaining(&self, kind: LoadingKind, now: Instant, delay: Duration) -> Option<Duration> {
        self.started(kind)
            .map(|start| delay.saturating_sub(now.saturating_duration_since(start)))
    }
    pub fn visible(&self, kind: LoadingKind, now: Instant, delay: Duration) -> bool {
        self.remaining(kind, now, delay)
            .is_some_and(|remaining| remaining.is_zero())
    }
}

#[cfg(test)]
mod tests {
    use super::{LoadingFeedback, LoadingKind};
    use std::time::{Duration, Instant};
    #[test]
    fn fast_completion_never_exposes_feedback_and_slow_work_does() {
        let now = Instant::now();
        let delay = Duration::from_millis(200);
        let mut state = LoadingFeedback::default();
        state.begin(LoadingKind::Comparison, now);
        assert!(!state.visible(LoadingKind::Comparison, now + delay / 2, delay));
        state.finish(LoadingKind::Comparison);
        assert!(!state.visible(LoadingKind::Comparison, now + delay, delay));
        state.begin(LoadingKind::File, now);
        assert!(state.visible(LoadingKind::File, now + delay, delay));
    }
    #[test]
    fn replacement_requests_and_live_delay_changes_use_the_current_request() {
        let now = Instant::now();
        let delay = Duration::from_millis(200);
        let mut state = LoadingFeedback::default();
        state.begin(LoadingKind::Comparison, now);
        state.begin(LoadingKind::Evolution, now);
        state.begin(LoadingKind::Comparison, now + delay / 2);
        assert!(!state.visible(LoadingKind::Comparison, now + delay, delay));
        assert!(state.visible(LoadingKind::Evolution, now + delay, delay));
        assert!(state.visible(LoadingKind::Comparison, now + delay, delay / 2));
        assert!(!state.visible(LoadingKind::Evolution, now + delay, delay * 2));
        assert!(state.visible(LoadingKind::Comparison, now + delay, Duration::ZERO));
    }
}
