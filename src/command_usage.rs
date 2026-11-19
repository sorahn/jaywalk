//! Application-wide command usage. Writes are ordered/coalesced off the UI thread.
use gpui::*;
use jaywalk::{commands::CommandId, quick_picker::CommandUsage};
use std::{io::Read as _, path::PathBuf, sync::mpsc, time::Duration};

#[derive(Default)]
pub(crate) struct Usage {
    pub(crate) history: CommandUsage,
    pub(crate) error: Option<String>,
    sender: Option<mpsc::Sender<Change>>,
    generation: u64,
}
impl Global for Usage {}
enum Change {
    Save(u64, CommandUsage, bool),
    Flush(async_channel::Sender<()>),
}
pub(crate) fn ensure(cx: &mut App) {
    if !cx.has_global::<Usage>() {
        cx.set_global(Usage::default());
    }
}
pub(crate) fn init(cx: &mut App) {
    let path = jaywalk::settings::default_path().map(|p| p.with_file_name("command-usage.json"));
    let loaded =
        path.as_ref()
            .map_err(Clone::clone)
            .and_then(|path| match std::fs::File::open(path) {
                Ok(file) => {
                    let mut bytes = Vec::new();
                    file.take(65537)
                        .read_to_end(&mut bytes)
                        .map_err(|e| format!("Could not read command usage: {e}"))?;
                    CommandUsage::decode(&bytes)
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(CommandUsage::default()),
                Err(e) => Err(format!("Could not read command usage: {e}")),
            });
    let blocked = loaded.is_err();
    let error = loaded.as_ref().err().cloned();
    let (sender, receiver) = mpsc::channel();
    let (results, errors) = async_channel::unbounded();
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
    cx.set_global(Usage {
        history: loaded.unwrap_or_default(),
        error,
        sender: Some(sender),
        generation: 0,
    });
    if let Err(error) = std::thread::Builder::new()
        .name("jaywalk-command-usage".into())
        .spawn(move || write_usage(path, blocked, receiver, results))
    {
        cx.global_mut::<Usage>().error =
            Some(format!("Could not start command usage writer: {error}"));
    }
    cx.spawn(async move |cx| {
        while let Ok((generation, error)) = errors.recv().await {
            cx.update(|cx| {
                let state = cx.global_mut::<Usage>();
                if state.generation == generation {
                    state.error = error;
                    cx.refresh_windows();
                }
            });
        }
    })
    .detach();
}
fn write_usage(
    path: Result<PathBuf, String>,
    mut blocked: bool,
    receiver: mpsc::Receiver<Change>,
    results: async_channel::Sender<(u64, Option<String>)>,
) {
    let mut pending: Option<(u64, CommandUsage, bool)> = None;
    let mut flush = |pending: &mut Option<(u64, CommandUsage, bool)>| {
        if let Some((generation, history, reset)) = pending.take() {
            if reset {
                blocked = false;
            }
            let result = if blocked {
                Err(
                    "Saved command history was preserved; Clear command usage history to reset it."
                        .into(),
                )
            } else {
                path.as_ref()
                    .map_err(Clone::clone)
                    .and_then(|path| save(path, &history))
            };
            let _ = results.try_send((generation, result.err()));
        }
    };
    loop {
        match receiver.recv_timeout(Duration::from_millis(250)) {
            Ok(Change::Save(generation, history, reset)) => {
                let reset = reset || pending.as_ref().is_some_and(|(_, _, reset)| *reset);
                pending = Some((generation, history, reset));
            }
            Ok(Change::Flush(ack)) => {
                flush(&mut pending);
                let _ = ack.try_send(());
            }
            Err(mpsc::RecvTimeoutError::Timeout) => flush(&mut pending),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                flush(&mut pending);
                break;
            }
        }
    }
}
fn save(path: &std::path::Path, history: &CommandUsage) -> Result<(), String> {
    let directory = path.parent().ok_or("Command usage path has no parent")?;
    std::fs::create_dir_all(directory)
        .map_err(|e| format!("Could not create command usage directory: {e}"))?;
    let bytes =
        serde_json::to_vec(history).map_err(|e| format!("Could not encode command usage: {e}"))?;
    let temp = path.with_extension(format!("{}.tmp", std::process::id()));
    std::fs::write(&temp, bytes)
        .and_then(|()| std::fs::rename(&temp, path))
        .map_err(|e| format!("Could not save command usage: {e}"))
}
pub(crate) fn record(command: CommandId, cx: &mut App) {
    update(Some(command), cx);
}
pub(crate) fn clear(cx: &mut App) {
    update(None, cx);
}
fn update(command: Option<CommandId>, cx: &mut App) {
    ensure(cx);
    let state = cx.global_mut::<Usage>();
    if let Some(command) = command {
        state.history.record(command);
    } else {
        state.history = CommandUsage::default();
        state.error = None;
    }
    state.generation += 1;
    if let Some(sender) = &state.sender
        && let Err(error) = sender.send(Change::Save(
            state.generation,
            state.history.clone(),
            command.is_none(),
        ))
    {
        state.error = Some(format!("Could not queue command usage: {error}"));
    }
}

#[cfg(test)]
mod tests {
    use super::{Change, write_usage};
    use jaywalk::{commands::CommandId, quick_picker::CommandUsage};
    #[test]
    fn ordered_writer_preserves_bad_data_until_reset_and_flushes_on_exit() {
        let root = std::env::temp_dir().join(format!("jw-command-usage-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("usage.json");
        std::fs::write(&path, "bad data").unwrap();
        let (sender, receiver) = std::sync::mpsc::channel();
        let (results, errors) = async_channel::unbounded();
        let target = path.clone();
        let worker = std::thread::spawn(move || write_usage(Ok(target), true, receiver, results));
        let mut history = CommandUsage::default();
        history.record(CommandId::Refresh);
        sender
            .send(Change::Save(1, history.clone(), false))
            .unwrap();
        let (ack, done) = async_channel::bounded(1);
        sender.send(Change::Flush(ack)).unwrap();
        done.recv_blocking().unwrap();
        assert!(errors.recv_blocking().unwrap().1.is_some());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "bad data");
        sender
            .send(Change::Save(2, CommandUsage::default(), true))
            .unwrap();
        history.record(CommandId::Refresh);
        sender.send(Change::Save(3, history, false)).unwrap();
        drop(sender);
        worker.join().unwrap();
        let history = CommandUsage::decode(&std::fs::read(path).unwrap()).unwrap();
        assert!(history.score(CommandId::Refresh) > 0);
        std::fs::remove_dir_all(root).unwrap();
    }
}
