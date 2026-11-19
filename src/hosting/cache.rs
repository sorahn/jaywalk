use super::{Batch, Scope};
use crate::accounts::{AccountKey, Error};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, Weak},
    time::{Duration, Instant, SystemTime},
};
struct Entry {
    inserted: Instant,
    batch: Arc<Batch>,
    fetched_at: SystemTime,
}
#[derive(Default)]
pub(crate) struct Cache {
    entries: HashMap<(AccountKey, u64, Scope), Entry>,
    limited: HashMap<AccountKey, Instant>,
    requests: HashMap<(AccountKey, u64), Weak<Mutex<()>>>,
}
impl Cache {
    pub(crate) fn request_gate(&mut self, key: &AccountKey, generation: u64) -> Arc<Mutex<()>> {
        self.requests.retain(|_, gate| gate.strong_count() > 0);
        let gate = self.requests.entry((key.clone(), generation)).or_default();
        if let Some(gate) = gate.upgrade() {
            return gate;
        }
        let next = Arc::new(Mutex::new(()));
        *gate = Arc::downgrade(&next);
        next
    }
    pub(crate) fn get(
        &self,
        key: &AccountKey,
        generation: u64,
        scope: &Scope,
    ) -> Option<(Arc<Batch>, SystemTime)> {
        self.entries
            .get(&(key.clone(), generation, scope.clone()))
            .filter(|entry| entry.inserted.elapsed() < Duration::from_secs(60))
            .map(|entry| (entry.batch.clone(), entry.fetched_at))
    }
    pub(crate) fn updated_since(
        &self,
        key: &AccountKey,
        generation: u64,
        scope: &Scope,
        since: Instant,
    ) -> Option<(Arc<Batch>, SystemTime)> {
        self.entries
            .get(&(key.clone(), generation, scope.clone()))
            .filter(|entry| entry.inserted >= since)
            .map(|entry| (entry.batch.clone(), entry.fetched_at))
    }
    pub(crate) fn check_rate(&self, key: &AccountKey) -> Result<(), Error> {
        if self
            .limited
            .get(key)
            .is_some_and(|at| at.elapsed() < Duration::from_secs(60))
        {
            Err(Error::RateLimited)
        } else {
            Ok(())
        }
    }
    pub(crate) fn limited(&mut self, key: AccountKey) {
        self.limited.insert(key, Instant::now());
    }
    pub(crate) fn put(
        &mut self,
        key: AccountKey,
        generation: u64,
        scope: Scope,
        batch: Arc<Batch>,
        fetched_at: SystemTime,
    ) {
        self.entries
            .retain(|(account, epoch, _), _| account != &key || *epoch == generation);
        if self.entries.len() >= 16
            && let Some(old) = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.inserted)
                .map(|(key, _)| key.clone())
        {
            self.entries.remove(&old);
        }
        self.entries.insert(
            (key, generation, scope),
            Entry {
                inserted: Instant::now(),
                batch,
                fetched_at,
            },
        );
    }
}
#[derive(Clone)]
pub struct Fetched {
    pub generation: u64,
    pub batch: Arc<Batch>,
    pub cached: bool,
    pub fetched_at: SystemTime,
}
