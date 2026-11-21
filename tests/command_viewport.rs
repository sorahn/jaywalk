//! Exercise the vendored pure viewport calculation through the host's locked
//! dependency graph, without a second toolkit workspace or native window.
extern crate gpui_kit as gpui;
#[path = "../vendor/gpui-component/src/command/viewport.rs"]
mod viewport;
