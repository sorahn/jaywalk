//! Native integration stays outside the portable viewer.
use gpui::*;
mod opener;
pub(crate) use opener::open_text_file;

#[cfg(target_os = "macos")]
mod macos;

pub(crate) fn update_menus(cx: &mut App) {
    #[cfg(target_os = "macos")]
    macos::update_menus(cx);
    #[cfg(not(target_os = "macos"))]
    let _ = cx;
}

pub(crate) fn is_app_bundle() -> bool {
    #[cfg(target_os = "macos")]
    return std::env::current_exe().is_ok_and(|path| {
        path.parent()
            .is_some_and(|parent| parent.ends_with("Contents/MacOS"))
    });
    #[cfg(not(target_os = "macos"))]
    false
}
