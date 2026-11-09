//! Bounded browser-style visits. A repeated location updates its snapshot;
//! visiting elsewhere after going Back discards the forward path.
/// Pending destinations have not been shown. Restorations have already moved
/// the cursor and can be stepped again before their content finishes loading.
pub enum VisitState {
    Ready,
    Pending,
    Restoring,
}
#[derive(Clone)]
pub struct Entry<K, V> {
    pub key: K,
    pub value: V,
}
pub struct History<K, V> {
    entries: Vec<Entry<K, V>>,
    cursor: Option<usize>,
    limit: usize,
}
impl<K: Eq, V> History<K, V> {
    pub fn new(limit: usize) -> Self {
        Self {
            entries: Vec::new(),
            cursor: None,
            limit: limit.max(1),
        }
    }
    pub fn current(&self) -> Option<&Entry<K, V>> {
        self.cursor.and_then(|index| self.entries.get(index))
    }
    pub fn can_back(&self) -> bool {
        self.cursor.is_some_and(|index| index > 0)
    }
    pub fn can_forward(&self) -> bool {
        self.cursor
            .is_some_and(|index| index + 1 < self.entries.len())
    }
    pub fn observe(&mut self, key: K, value: V) {
        if let Some(index) = self.cursor {
            if self.entries[index].key == key {
                self.entries[index].value = value;
                return;
            }
            self.entries.truncate(index + 1);
        }
        self.entries.push(Entry { key, value });
        self.cursor = Some(self.entries.len() - 1);
        self.trim();
    }
    pub fn set_limit(&mut self, limit: usize) {
        self.limit = limit.max(1);
        self.trim();
    }
    fn trim(&mut self) {
        // Keep the current location even when lowering the limit while Back.
        let excess = self.entries.len().saturating_sub(self.limit);
        let remove_front = excess.min(self.cursor.unwrap_or(0));
        self.entries.drain(..remove_front);
        self.cursor = self.cursor.map(|index| index - remove_front);
        self.entries.truncate(self.limit);
    }
}
impl<K: Eq + Clone, V: Clone> History<K, V> {
    pub fn navigate(&mut self, forward: bool, state: VisitState) -> Option<Entry<K, V>> {
        if matches!(state, VisitState::Pending) {
            return if forward {
                None
            } else {
                self.current().cloned()
            };
        }
        self.step(forward)
    }
    pub fn step(&mut self, forward: bool) -> Option<Entry<K, V>> {
        if forward && self.can_forward() {
            self.cursor = self.cursor.map(|index| index + 1);
        } else if !forward && self.can_back() {
            self.cursor = self.cursor.map(|index| index - 1);
        } else {
            return None;
        }
        self.current().cloned()
    }
}
#[cfg(test)]
mod tests {
    use super::History;
    #[test]
    fn back_forward_updates_positions_and_new_visits_fork_the_path() {
        let mut history = History::new(10);
        assert!(history.step(false).is_none());
        history.observe("overview", 0);
        history.observe("commit", 10);
        history.observe("file", 20);
        assert_eq!(history.step(false).unwrap().key, "commit");
        history.observe("commit", 15);
        assert!(history.can_forward());
        assert_eq!(history.step(true).unwrap().value, 20);
        assert_eq!(history.step(false).unwrap().value, 15);
        history.observe("another file", 30);
        assert!(!history.can_forward());
        assert_eq!(history.step(false).unwrap().value, 15);
        assert_eq!(history.step(false).unwrap().key, "overview");
        assert!(history.step(false).is_none());
    }
    #[test]
    fn trimming_retains_the_current_view_and_bounds_both_directions() {
        let mut history = History::new(3);
        for index in 0..5 {
            history.observe(index, index);
        }
        assert_eq!(history.step(false).unwrap().key, 3);
        assert_eq!(history.step(false).unwrap().key, 2);
        assert!(!history.can_back());
        history.set_limit(1);
        assert_eq!(history.current().unwrap().key, 2);
        assert!(!history.can_forward());
        assert!(!history.can_back());
        history.observe(6, 6);
        assert_eq!(history.current().unwrap().key, 6);
        assert!(!history.can_back());
    }
    #[test]
    fn pending_navigation_cancels_to_current_but_restorations_can_step_again() {
        use super::VisitState;
        let mut history = History::new(10);
        history.observe("overview", 1);
        history.observe("file a", 2);
        assert_eq!(
            history.navigate(false, VisitState::Pending).unwrap().key,
            "file a"
        );
        assert!(history.navigate(true, VisitState::Pending).is_none());
        assert_eq!(
            history.navigate(false, VisitState::Ready).unwrap().key,
            "overview"
        );
        assert_eq!(
            history.navigate(true, VisitState::Restoring).unwrap().key,
            "file a"
        );
        assert_eq!(
            history.navigate(false, VisitState::Restoring).unwrap().key,
            "overview"
        );
    }
}
