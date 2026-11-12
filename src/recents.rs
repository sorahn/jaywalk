//! Shared recent-repository cache and one ordered background writer.
use gpui::*;
use jaywalk::recent::RecentStore;
use std::path::{Path, PathBuf};

enum Change {
    Flush(async_channel::Sender<()>),
    Record(PathBuf),
    Clear,
}
pub(crate) struct Recents {
    pub(crate) repositories: Vec<PathBuf>,
    pub(crate) error: Option<String>,
    sender: async_channel::Sender<Change>,
}
impl Global for Recents {}
/// In-memory service for headless window tests. OS-thread storage writers
/// cannot wake GPUI's deterministic foreground scheduler from another thread.
#[cfg(test)]
pub(crate) fn init_without_storage(cx: &mut App) {
    let (sender, receiver) = async_channel::unbounded();
    cx.set_global(Recents {
        repositories: Vec::new(),
        error: None,
        sender,
    });
    cx.spawn(async move |_| {
        while let Ok(change) = receiver.recv().await {
            if let Change::Flush(done) = change {
                let _ = done.try_send(());
            }
        }
    })
    .detach();
}

pub(crate) fn init(store: RecentStore, cx: &mut App) {
    let (sender, receiver) = async_channel::unbounded();
    let (results, errors) = async_channel::unbounded();
    let flush_sender = sender.clone();
    cx.on_app_quit(move |_| {
        let (ack, done) = async_channel::bounded(1);
        let sent = flush_sender.try_send(Change::Flush(ack)).is_ok();
        async move {
            if sent {
                let _ = done.recv().await;
            }
        }
    })
    .detach();
    cx.set_global(Recents {
        repositories: store.repositories().to_vec(),
        error: store.error().map(str::to_owned),
        sender,
    });
    // Keep storage independent of the foreground executor, including while
    // quit observers drain queued writes within GPUI's shutdown deadline.
    if let Err(error) = std::thread::Builder::new()
        .name("jaywalk-recents".into())
        .spawn(move || write_history(store, receiver, results))
    {
        cx.global_mut::<Recents>().error =
            Some(format!("Could not start recent-history writer: {error}"));
    }
    cx.spawn(async move |cx| {
        while let Ok(error) = errors.recv().await {
            cx.update(|cx| {
                cx.global_mut::<Recents>().error = error;
                cx.refresh_windows();
            });
        }
    })
    .detach();
}
fn write_history(
    mut store: RecentStore,
    receiver: async_channel::Receiver<Change>,
    results: async_channel::Sender<Option<String>>,
) {
    while let Ok(change) = receiver.recv_blocking() {
        let result = match change {
            Change::Record(path) => store.record(&path),
            Change::Clear => store.clear(),
            Change::Flush(ack) => {
                let _ = ack.try_send(());
                continue;
            }
        };
        let _ = results.try_send(result.err());
    }
}
pub(crate) fn record(directory: &Path, cx: &mut App) {
    let state = cx.global_mut::<Recents>();
    state.repositories.retain(|path| path != directory);
    state.repositories.insert(0, directory.to_owned());
    if let Err(error) = state.sender.try_send(Change::Record(directory.to_owned())) {
        state.error = Some(format!(
            "Could not queue recent repository history: {error}"
        ));
    }
    cx.refresh_windows();
}
pub(crate) fn clear(cx: &mut App) {
    let state = cx.global_mut::<Recents>();
    state.repositories.clear();
    if let Err(error) = state.sender.try_send(Change::Clear) {
        state.error = Some(format!(
            "Could not clear recent repository history: {error}"
        ));
    }
    cx.refresh_windows();
}
pub(crate) fn display_path(path: &Path) -> String {
    if let Some(home) = std::env::var_os("HOME") {
        let home = Path::new(&home);
        if let Ok(relative) = path.strip_prefix(home) {
            return if relative.as_os_str().is_empty() {
                "~".into()
            } else {
                format!("~/{}", relative.display())
            };
        }
    }
    path.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::{Change, write_history};
    use jaywalk::recent::RecentStore;
    #[test]
    fn queued_updates_flush_in_order_without_any_window() {
        let root = std::env::temp_dir().join(format!("jw-recent-writer-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("recent.toml");
        let store = RecentStore::load(path.clone()).unwrap();
        let (sender, receiver) = async_channel::unbounded();
        let (results, errors) = async_channel::unbounded();
        let task = std::thread::spawn(move || write_history(store, receiver, results));
        sender.try_send(Change::Record(root.join("first"))).unwrap();
        sender.try_send(Change::Clear).unwrap();
        sender
            .try_send(Change::Record(root.join("second")))
            .unwrap();
        let (ack, done) = async_channel::bounded(1);
        sender.try_send(Change::Flush(ack)).unwrap();
        done.recv_blocking().unwrap();
        assert_eq!(
            RecentStore::load(path).unwrap().repositories(),
            [root.join("second")]
        );
        for _ in 0..3 {
            assert!(errors.recv_blocking().unwrap().is_none());
        }
        drop(sender);
        task.join().unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
}
