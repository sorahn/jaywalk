//! Opt-in native frame counters; never requests frames or reads repository contents.
use gpui::*;
use std::time::Duration;

pub(crate) fn start(window: AnyWindowHandle, cx: &mut App) {
    if std::env::var_os("JAYWALK_PROFILE_FRAMES").as_deref() != Some(std::ffi::OsStr::new("1")) {
        return;
    }
    cx.spawn(async move |cx| {
        let mut previous = 0;
        loop {
            cx.background_executor().timer(Duration::from_secs(1)).await;
            if window.update(cx, |_,window,_| {
                let frames=window.frame_duration_snapshot();
                let draw=&frames.draw_duration_histogram;
                if draw.len()==previous { return; }
                previous=draw.len();
                eprintln!("{}",serde_json::json!({
                    "metric":"native-frames",
                    "revision":jaywalk::version::REVISION,
                    "window":format!("{:?}",window.window_handle().window_id()),
                    "frames":draw.len(),
                    "draw_p50_ms":draw.value_at_quantile(0.5) as f64/1_000_000.,
                    "draw_p95_ms":draw.value_at_quantile(0.95) as f64/1_000_000.,
                    "draw_p99_ms":draw.value_at_quantile(0.99) as f64/1_000_000.,
                    "draw_max_ms":draw.max() as f64/1_000_000.,
                    "dirty_to_present_p95_ms":frames.dirty_to_present_histogram.value_at_quantile(0.95) as f64/1_000_000.,
                    "present_interval_p95_ms":frames.present_interval_histogram.value_at_quantile(0.95) as f64/1_000_000.,
                }));
            }).is_err() { break; }
        }
    }).detach();
}
