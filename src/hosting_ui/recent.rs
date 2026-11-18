use gpui::{App, AppContext, Global};
use jaywalk::hosting::Locator;
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
};
enum Write {
    Save(PathBuf, Vec<Locator>),
    Flush(async_channel::Sender<()>),
}
pub(super) struct Recents {
    pub blocked: HashSet<PathBuf>,
    pub lists: HashMap<PathBuf, Vec<Locator>>,
    sender: async_channel::Sender<Write>,
    pub error: Option<String>,
}
impl Global for Recents {}
pub(super) fn init(cx: &mut App) {
    if cx.has_global::<Recents>() {
        return;
    }
    let (sender, receiver) = async_channel::unbounded();
    let flush = sender.clone();
    cx.set_global(Recents {
        blocked: HashSet::new(),
        lists: HashMap::new(),
        sender,
        error: None,
    });
    cx.spawn(async move |cx| {
        while let Ok(message) = receiver.recv().await {
            match message {
                Write::Save(path, items) => {
                    let result = cx
                        .background_spawn(
                            async move { jaywalk::hosting::recent::save(&path, &items) },
                        )
                        .await;
                    if let Err(error) = result {
                        cx.update(|cx| {
                            cx.global_mut::<Recents>().error = Some(error.to_string());
                            cx.refresh_windows();
                        });
                    }
                }
                Write::Flush(done) => {
                    let _ = done.try_send(());
                }
            }
        }
    })
    .detach();
    cx.on_app_quit(move |_| {
        let (send, done) = async_channel::bounded(1);
        let sent = flush.try_send(Write::Flush(send)).is_ok();
        async move {
            if sent {
                let _ = done.recv().await;
            }
        }
    })
    .detach();
}
pub(super) fn record(path: PathBuf, locator: Locator, cx: &mut App) {
    let state = cx.global_mut::<Recents>();
    let list = state.lists.entry(path.clone()).or_default();
    list.retain(|old| old != &locator);
    list.insert(0, locator);
    list.truncate(50);
    if !state.blocked.contains(&path) {
        let _ = state.sender.try_send(Write::Save(path, list.clone()));
    }
    cx.refresh_windows();
}
