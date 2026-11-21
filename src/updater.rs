//! The only updater entry point. Unconfigured builds never create a client.
use gpui::*;
#[cfg(all(target_os = "macos", feature = "macos-updater"))]
mod native {
    unsafe extern "C" {
        pub(super) fn jaywalk_updater_configure(
            checks: bool,
            downloads: bool,
            install: bool,
            interval: f64,
            restart: bool,
        );
        pub(super) fn jaywalk_updater_check() -> bool;
        pub(super) fn jaywalk_updater_status() -> *const std::ffi::c_char;
        pub(super) fn jaywalk_updater_feed() -> *const std::ffi::c_char;
    }
}
fn configure_native(updates: jaywalk::setup::UpdateSettings, restart_allowed: bool) {
    #[cfg(all(target_os = "macos", feature = "macos-updater"))]
    // SAFETY: called only on GPUI's main thread; the C adapter retains its objects.
    unsafe {
        native::jaywalk_updater_configure(
            updates.automatic_checks,
            updates.automatic_downloads,
            updates.automatic_installation,
            f64::from(updates.check_interval_hours) * 3600.,
            restart_allowed,
        );
    }
    #[cfg(not(all(target_os = "macos", feature = "macos-updater")))]
    let _ = (updates, restart_allowed);
}
pub(crate) fn check(window: &mut Window, cx: &mut App) {
    #[cfg(all(target_os = "macos", feature = "macos-updater"))]
    // SAFETY: GPUI actions run on the main thread. The adapter owns its strings.
    unsafe {
        if native::jaywalk_updater_check() {
            return;
        }
    }
    drop(window.prompt(PromptLevel::Info,"Updates are not configured for this build",Some(&format!("{} Development and unsigned rehearsal builds do not contact an update service. Use a configured official release to check for updates.", status())),&["OK"],cx));
}
pub(crate) fn status() -> String {
    #[cfg(all(target_os = "macos", feature = "macos-updater"))]
    // SAFETY: static adapter-owned NSString, copied before another adapter call.
    unsafe {
        std::ffi::CStr::from_ptr(native::jaywalk_updater_status())
            .to_string_lossy()
            .into_owned()
    }
    #[cfg(not(all(target_os = "macos", feature = "macos-updater")))]
    "Updates are not configured for this build.".into()
}

struct UpdaterState {
    updates: jaywalk::setup::UpdateSettings,
    last_status: String,
    _task: Task<()>,
}
impl Global for UpdaterState {}
pub(crate) fn configure(settings: &jaywalk::settings::Settings, cx: &mut App) {
    if cfg!(test) || !cfg!(all(target_os = "macos", feature = "macos-updater")) {
        return;
    }
    if !cx.has_global::<UpdaterState>() {
        let task = cx.spawn(async move |cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(1))
                    .await;
                cx.update(|cx| {
                    let state = cx.global::<UpdaterState>();
                    configure_native(state.updates, crate::windows::update_restart_allowed(cx));
                    let current = status();
                    if cx.global::<UpdaterState>().last_status != current {
                        cx.global_mut::<UpdaterState>().last_status = current;
                        cx.refresh_windows();
                    }
                });
            }
        });
        cx.set_global(UpdaterState {
            updates: settings.updates,
            last_status: status(),
            _task: task,
        });
    } else {
        let state = cx.global_mut::<UpdaterState>();
        state.updates = settings.updates;
    }
    configure_native(settings.updates, crate::windows::update_restart_allowed(cx));
}

#[cfg(all(target_os = "macos", feature = "macos-updater"))]
#[unsafe(no_mangle)]
extern "C" fn jaywalk_updater_restart_is_safe() -> bool {
    crate::windows::restart_is_safe()
}

pub(crate) fn source() -> String {
    #[cfg(all(target_os = "macos", feature = "macos-updater"))]
    // SAFETY: read-only bundle metadata on the UI thread; copy the borrowed text.
    unsafe {
        std::ffi::CStr::from_ptr(native::jaywalk_updater_feed())
            .to_string_lossy()
            .into_owned()
    }
    #[cfg(not(all(target_os = "macos", feature = "macos-updater")))]
    "No public update feed is configured in this build.".into()
}
