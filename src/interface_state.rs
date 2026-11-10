//! Ordered layout snapshots survive closing their originating window. The
//! worker coalesces pending updates and flushes before acknowledging app quit.
use gpui::*;
use jaywalk::layout_state::{LayoutState, RepositoryLayoutStore};
use std::collections::HashMap;
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
    Flush(async_channel::Sender<()>),
}
struct InterfaceState {
    sender: Sender<Change>,
    latest: HashMap<StoreKey, LayoutState>,
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
fn write_layouts(
    receiver: Receiver<Change>,
    results: async_channel::Sender<(StoreKey, Result<(), String>)>,
) {
    let mut pending = HashMap::new();
    let flush = |pending: &mut HashMap<StoreKey, LayoutState>| {
        for (key, saved) in pending.drain() {
            let result = RepositoryLayoutStore::new(key.path.clone(), &key.repository)
                .and_then(|store| store.save(&saved));
            let _ = results.try_send((key, result));
        }
    };
    loop {
        match receiver.recv_timeout(Duration::from_millis(250)) {
            Ok(Change::Save(key, state)) => {
                pending.insert(key, state);
            }
            Ok(Change::Flush(ack)) => {
                flush(&mut pending);
                let _ = ack.try_send(());
            }
            Err(RecvTimeoutError::Timeout) => flush(&mut pending),
            Err(RecvTimeoutError::Disconnected) => {
                flush(&mut pending);
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
