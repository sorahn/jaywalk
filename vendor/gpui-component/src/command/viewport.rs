//! Fit a bounded list to complete measured items, without orphaned headings.
use gpui::{Pixels, px};

pub(super) fn height(
    rows: impl IntoIterator<Item = (Pixels, bool)>,
    limit: Pixels,
    inset: Pixels,
) -> Pixels {
    let mut used = px(0.);
    let mut fitted = px(0.);
    for (height, selectable) in rows {
        if used + height + inset * 2. > limit {
            break;
        }
        used += height;
        if selectable {
            fitted = used;
        }
    }
    if fitted > px(0.) {
        fitted + inset * 2.
    } else {
        limit // Oversized first items must remain scrollable, not disappear.
    }
}

#[cfg(test)]
mod tests {
    use super::height;
    use gpui::px;

    #[test]
    fn variable_items_fit_without_orphan_headings() {
        let rows = [
            (px(34.), true),
            (px(24.), false),
            (px(60.), true),
            (px(34.), true),
        ];
        assert_eq!(height(rows, px(100.), px(4.)), px(42.));
        assert_eq!(height(rows, px(130.), px(4.)), px(126.));
        assert_eq!(height(rows, px(500.), px(4.)), px(160.));
    }

    #[test]
    fn oversized_first_item_and_empty_list_keep_a_scrollable_viewport() {
        assert_eq!(height([(px(500.), true)], px(200.), px(4.)), px(200.));
        assert_eq!(height([], px(200.), px(4.)), px(200.));
    }
}
