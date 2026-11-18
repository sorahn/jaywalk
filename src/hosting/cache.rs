use super::{Batch, Scope};
use crate::accounts::{AccountKey, Error};
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};
#[derive(Default)]
pub(crate) struct Cache {
    entries: HashMap<(AccountKey, u64, Scope), (Instant, Arc<Batch>)>,
    limited: HashMap<AccountKey, Instant>,
}
impl Cache {
    pub(crate) fn get(
        &self,
        key: &AccountKey,
        generation: u64,
        scope: &Scope,
    ) -> Option<Arc<Batch>> {
        self.entries
            .get(&(key.clone(), generation, scope.clone()))
            .filter(|(at, _)| at.elapsed() < Duration::from_secs(60))
            .map(|(_, batch)| batch.clone())
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
    ) {
        self.entries
            .retain(|(account, epoch, _), _| account != &key || *epoch == generation);
        if self.entries.len() >= 16
            && let Some(old) = self
                .entries
                .iter()
                .min_by_key(|(_, (at, _))| *at)
                .map(|(key, _)| key.clone())
        {
            self.entries.remove(&old);
        }
        self.entries
            .insert((key, generation, scope), (Instant::now(), batch));
    }
}
#[derive(Clone)]
pub struct Fetched {
    pub generation: u64,
    pub batch: Arc<Batch>,
    pub cached: bool,
}
