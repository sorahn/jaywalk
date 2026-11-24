//! Ordered layout snapshots survive closing their originating window. The
//! worker coalesces pending updates and flushes before acknowledging app quit.
use gpui::*;
use jaywalk::layout_state::{LayoutState, RepositoryLayoutStore};
use jaywalk::preferences::groups::Group;
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

#[derive(Clone, PartialEq, Eq, Hash)]
pub(crate) struct StoreKey {
    pub(crate) path: PathBuf,
    pub(crate) repository: PathBuf,
}
enum Change {
    Save(StoreKey, LayoutState),
    Settings(PathBuf, BTreeSet<Group>),
    Flush(async_channel::Sender<()>),
}
struct InterfaceState {
    sender: Sender<Change>,
    latest: HashMap<StoreKey, LayoutState>,
    settings: HashMap<PathBuf, BTreeSet<Group>>,
}
impl Global for InterfaceState {}
pub(crate) fn init(cx: &mut App) {
    let (sender, receiver) = mpsc::channel();
    let (results, responses) = async_channel::unbounded();
    let flush = sender.clone();
    cx.on_app_quit(move |_| {
        let (ack, done) = async_channel::bounded(1);
        let sent = flush.send(Change::Flush(ack)).is_ok();
        async move {
            if sent {
                let _ = done.recv().await;
            }
        }
    })
    .detach();
    cx.set_global(InterfaceState {
        sender,
        latest: HashMap::new(),
        settings: HashMap::new(),
    });
    if let Err(error) = std::thread::Builder::new()
        .name("jaywalk-interface".into())
        .spawn(move || write_layouts(receiver, results))
    {
        eprintln!("Could not start interface writer: {error}");
    }
    cx.spawn(async move |cx| {
        while let Ok((key, result)) = responses.recv().await {
            cx.update(|cx| crate::windows::layout_saved(&key, result.err(), cx));
        }
    })
    .detach();
}
pub(crate) fn save(key: StoreKey, state: LayoutState, cx: &mut App) -> Result<(), String> {
    let service = cx.global_mut::<InterfaceState>();
    service
        .sender
        .send(Change::Save(key.clone(), state.clone()))
        .map_err(|error| format!("Could not queue interface choices: {error}"))?;
    service.latest.insert(key, state);
    Ok(())
}
pub(crate) fn latest(path: &Path, repository: &Path, cx: &App) -> Option<LayoutState> {
    cx.global::<InterfaceState>()
        .latest
        .get(&StoreKey {
            path: path.to_owned(),
            repository: repository.to_owned(),
        })
        .cloned()
}
fn ensure(cx: &mut App) {
    if !cx.has_global::<InterfaceState>() {
        #[cfg(test)]
        init_for_test(cx);
        #[cfg(not(test))]
        init(cx);
    }
}
pub(crate) fn settings_groups(path: &Path, cx: &App) -> Option<BTreeSet<Group>> {
    cx.try_global::<InterfaceState>()
        .and_then(|state| state.settings.get(path).cloned())
}
pub(crate) fn loaded_settings_groups(
    path: PathBuf,
    expanded: BTreeSet<Group>,
    cx: &mut App,
) -> BTreeSet<Group> {
    ensure(cx);
    cx.global_mut::<InterfaceState>()
        .settings
        .entry(path)
        .or_insert(expanded)
        .clone()
}
pub(crate) fn save_settings_groups(
    path: PathBuf,
    expanded: BTreeSet<Group>,
    cx: &mut App,
) -> Result<(), String> {
    ensure(cx);
    let state = cx.global_mut::<InterfaceState>();
    state
        .sender
        .send(Change::Settings(path.clone(), expanded.clone()))
        .map_err(|e| format!("Could not queue settings view state: {e}"))?;
    state.settings.insert(path, expanded);
    Ok(())
}
/// Exercise the actual file writer without waking GPUI's deterministic test
/// executor from an OS thread. Tests inspect the shared cache and flush to disk.
#[cfg(test)]
pub(crate) fn init_for_test(cx: &mut App) {
    let (sender, receiver) = mpsc::channel();
    let (results, _responses) = async_channel::unbounded();
    cx.set_global(InterfaceState {
        sender,
        latest: HashMap::new(),
        settings: HashMap::new(),
    });
    std::thread::spawn(move || write_layouts(receiver, results));
}
#[cfg(test)]
pub(crate) fn flush_for_test(cx: &App) {
    let (ack, done) = async_channel::bounded(1);
    cx.global::<InterfaceState>()
        .sender
        .send(Change::Flush(ack))
        .unwrap();
    done.recv_blocking().unwrap();
}
fn write_layouts(
    receiver: Receiver<Change>,
    results: async_channel::Sender<(StoreKey, Result<(), String>)>,
) {
    let mut pending = HashMap::new();
    let mut settings = HashMap::new();
    let flush = |pending: &mut HashMap<StoreKey, LayoutState>,
                 settings: &mut HashMap<PathBuf, BTreeSet<Group>>| {
        for (key, saved) in pending.drain() {
            let result = RepositoryLayoutStore::new(key.path.clone(), &key.repository)
                .and_then(|store| store.save(&saved));
            let _ = results.try_send((key, result));
        }
        for (path, expanded) in settings.drain() {
            if let Err(error) = jaywalk::preferences::groups::save(&path, &expanded) {
                eprintln!("{error}");
            }
        }
    };
    loop {
        match receiver.recv_timeout(Duration::from_millis(250)) {
            Ok(Change::Save(key, state)) => {
                pending.insert(key, state);
            }
            Ok(Change::Settings(path, expanded)) => {
                settings.insert(path, expanded);
            }
            Ok(Change::Flush(ack)) => {
                flush(&mut pending, &mut settings);
                let _ = ack.try_send(());
            }
            Err(RecvTimeoutError::Timeout) => flush(&mut pending, &mut settings),
            Err(RecvTimeoutError::Disconnected) => {
                flush(&mut pending, &mut settings);
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Change, StoreKey, write_layouts};
    use jaywalk::layout_state::{LayoutState, RepositoryLayoutStore};
    #[test]
    fn last_snapshot_survives_sender_drop_and_flush_preserves_other_repositories() {
        let root = std::env::temp_dir().join(format!("jw-interface-worker-{}", std::process::id()));
        std::fs::create_dir_all(root.join("one")).unwrap();
        std::fs::create_dir_all(root.join("two")).unwrap();
        let path = root.join("interface.toml");
        let first = StoreKey {
            path: path.clone(),
            repository: root.join("one"),
        };
        let second = StoreKey {
            path: path.clone(),
            repository: root.join("two"),
        };
        let (sender, receiver) = std::sync::mpsc::channel();
        let (results, responses) = async_channel::unbounded();
        let worker = std::thread::spawn(move || write_layouts(receiver, results));
        sender
            .send(Change::Save(
                first.clone(),
                LayoutState {
                    history_width: Some(200.),
                    ..Default::default()
                },
            ))
            .unwrap();
        sender
            .send(Change::Save(
                first.clone(),
                LayoutState {
                    history_width: Some(333.),
                    ..Default::default()
                },
            ))
            .unwrap();
        sender
            .send(Change::Save(
                second.clone(),
                LayoutState {
                    files_hidden: true,
                    ..Default::default()
                },
            ))
            .unwrap();
        let (ack, done) = async_channel::bounded(1);
        sender.send(Change::Flush(ack)).unwrap();
        done.recv_blocking().unwrap();
        for _ in 0..2 {
            assert!(responses.recv_blocking().unwrap().1.is_ok());
        }
        assert_eq!(
            RepositoryLayoutStore::new(path.clone(), &first.repository)
                .unwrap()
                .read()
                .unwrap()
                .unwrap()
                .history_width,
            Some(333.)
        );
        assert!(
            RepositoryLayoutStore::new(path.clone(), &second.repository)
                .unwrap()
                .read()
                .unwrap()
                .unwrap()
                .files_hidden
        );
        sender
            .send(Change::Save(
                first.clone(),
                LayoutState {
                    history_width: Some(444.),
                    ..Default::default()
                },
            ))
            .unwrap();
        drop(sender);
        worker.join().unwrap();
        assert_eq!(
            RepositoryLayoutStore::new(path, &first.repository)
                .unwrap()
                .read()
                .unwrap()
                .unwrap()
                .history_width,
            Some(444.)
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
