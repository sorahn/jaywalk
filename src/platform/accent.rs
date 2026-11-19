//! Optional native accent support. Unsupported platforms use the configured palette.
use gpui::App;
use jaywalk::settings::RgbColor;

pub(crate) fn read(dark: bool) -> Option<RgbColor> {
    #[cfg(all(target_os = "macos", not(test)))]
    {
        native::read(dark)
    }
    #[cfg(any(not(target_os = "macos"), test))]
    {
        let _ = dark;
        None
    }
}
pub(crate) fn observe(cx: &mut App) {
    #[cfg(all(target_os = "macos", not(test)))]
    native::observe(cx);
    #[cfg(any(not(target_os = "macos"), test))]
    let _ = cx;
}

#[cfg(all(target_os = "macos", not(test)))]
mod native {
    use super::*;
    use block2::{RcBlock, StackBlock};
    use gpui::{Global, Task};
    use objc2::{MainThreadMarker, rc::Retained, runtime::ProtocolObject};
    use objc2_app_kit::{
        NSAppearance, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSColor, NSColorSpace,
        NSSystemColorsDidChangeNotification,
    };
    use objc2_foundation::{NSNotification, NSNotificationCenter, NSObjectProtocol};
    use std::{cell::Cell, ptr::NonNull};

    pub(super) fn read(dark: bool) -> Option<RgbColor> {
        let _main = MainThreadMarker::new()?;
        objc2::rc::autoreleasepool(|_| {
            // SAFETY: AppKit supplies these immutable process-lifetime names.
            let name = unsafe {
                if dark {
                    NSAppearanceNameDarkAqua
                } else {
                    NSAppearanceNameAqua
                }
            };
            let appearance = NSAppearance::appearanceNamed(name)?;
            let resolved = Cell::new(None);
            let block = StackBlock::new(|| {
                let Some(color) = NSColor::controlAccentColor()
                    .colorUsingColorSpace(&NSColorSpace::sRGBColorSpace())
                else {
                    return;
                };
                let channels = [
                    color.redComponent(),
                    color.greenComponent(),
                    color.blueComponent(),
                ];
                if channels.iter().any(|channel| !channel.is_finite()) {
                    return;
                }
                let rgb = channels.into_iter().fold(0u32, |rgb, channel| {
                    (rgb << 8) | (channel.clamp(0., 1.) * 255.).round() as u32
                });
                resolved.set(Some(
                    RgbColor::try_from(format!("#{rgb:06x}"))
                        .expect("three normalized channels form a valid RGB color"),
                ));
            });
            appearance.performAsCurrentDrawingAppearance(&block);
            resolved.get()
        })
    }
    struct Listener {
        center: Retained<NSNotificationCenter>,
        observer: Retained<ProtocolObject<dyn NSObjectProtocol>>,
        _task: Task<()>,
    }
    impl Global for Listener {}
    impl Drop for Listener {
        fn drop(&mut self) {
            // SAFETY: this token came from this center and remains retained until
            // removal. The notification block captures only a thread-safe sender.
            unsafe {
                let observer: &ProtocolObject<dyn NSObjectProtocol> = &self.observer;
                self.center.removeObserver(observer.as_ref());
            }
        }
    }
    pub(super) fn observe(cx: &mut App) {
        if MainThreadMarker::new().is_none() || cx.has_global::<Listener>() {
            return;
        }
        let (sender, receiver) = async_channel::bounded(1);
        let block = RcBlock::new(move |_: NonNull<NSNotification>| {
            let _ = sender.try_send(());
        });
        let center = NSNotificationCenter::defaultCenter();
        // SAFETY: no object/queue is supplied; the block is Send + Sync and does
        // not access GPUI/AppKit. The app owns and unregisters the returned token.
        let observer = unsafe {
            center.addObserverForName_object_queue_usingBlock(
                Some(NSSystemColorsDidChangeNotification),
                None,
                None,
                &block,
            )
        };
        let task = cx.spawn(async move |cx| {
            while receiver.recv().await.is_ok() {
                cx.update(crate::appearance::system_colors_changed);
            }
        });
        cx.set_global(Listener {
            center,
            observer,
            _task: task,
        });
    }
}
