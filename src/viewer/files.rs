use super::*;
use gpui::component::Sizable;
use gpui::component::list::ListItem;
use gpui::component::scroll::ScrollableMask;
use gpui::component::tree::{Tree, TreeItem, TreeState};
use gpui::prelude::FluentBuilder;
use jaywalk::file_browser::{CompactPath, FileNode, present_paths};
use std::collections::HashMap;

use jaywalk::settings::{FileView as FileListMode, TreeScope};

pub(super) struct FileBrowser {
    pub(super) files: std::sync::Arc<[jaywalk::repository_file::FileEntry]>,
    retained: Option<usize>,
    pub(super) mode: FileListMode,
    pub(super) scope: TreeScope,
    labels: Vec<CompactPath>,
    tree: Entity<TreeState>,
    roots: Vec<TreeItem>,
    expansion: HashMap<SharedString, bool>,
    tree_indices: HashMap<SharedString, usize>,
    observer: Option<Subscription>,
    horizontal: ScrollHandle,
    list_scroll: ScrollHandle,
    width: f32,
    tree_width: f32,
}

impl FileBrowser {
    pub(super) fn new(
        default_width: f32,
        mode: FileListMode,
        scope: TreeScope,
        cx: &mut App,
    ) -> Self {
        Self {
            files: std::sync::Arc::from([]),
            retained: None,
            mode,
            scope,
            labels: Vec::new(),
            tree: cx.new(|cx| TreeState::new(cx)),
            roots: Vec::new(),
            expansion: HashMap::new(),
            tree_indices: HashMap::new(),
            observer: None,
            horizontal: ScrollHandle::new(),
            list_scroll: ScrollHandle::new(),
            width: default_width,
            tree_width: default_width,
        }
    }
}

fn file_style(status: &str) -> (&'static str, u32) {
    match status {
        "added" => ("A", 0x7dc67c),
        "removed" => ("D", 0xe58a83),
        "renamed" => ("R", 0x86b4ed),
        "copied" => ("C", 0x86b4ed),
        "unchanged" => ("", 0xa4b4c2),
        _ => ("M", 0xe5c07b),
    }
}

fn tree_items(
    nodes: Vec<FileNode>,
    depth: usize,
    indices: &mut HashMap<SharedString, usize>,
    width: &mut f32,
    glyph_width: f32,
    indent_width: f32,
) -> Vec<TreeItem> {
    nodes
        .into_iter()
        .map(|node| {
            let (name, item) = match node {
                FileNode::Directory {
                    path,
                    name,
                    children,
                } => {
                    let children = tree_items(
                        children,
                        depth + 1,
                        indices,
                        width,
                        glyph_width,
                        indent_width,
                    );
                    (
                        name.clone(),
                        TreeItem::new(format!("dir:{path}"), name)
                            .children(children)
                            .expanded(true),
                    )
                }
                FileNode::File { index, name, path } => {
                    let id: SharedString = format!("file:{path}").into();
                    indices.insert(id.clone(), index);
                    (name.clone(), TreeItem::new(id, name))
                }
            };
            *width = width
                .max(name.chars().count() as f32 * glyph_width + depth as f32 * indent_width + 64.);
            item
        })
        .collect()
}

// TreeItem clones share expansion state. Retain all roots, including descendants
// hidden behind collapsed parents, and transfer state only for surviving IDs.
fn preserve_expansion(
    items: &[TreeItem],
    previous: &[TreeItem],
    states: &mut HashMap<SharedString, bool>,
) {
    fn visit(items: &[TreeItem], states: &mut HashMap<SharedString, bool>) {
        for item in items {
            if item.is_folder() {
                states.insert(item.id.clone(), item.is_expanded());
                visit(&item.children, states);
            }
        }
    }
    fn apply(items: &[TreeItem], states: &HashMap<SharedString, bool>) {
        for item in items {
            if let Some(expanded) = states.get(&item.id) {
                item.clone().expanded(*expanded);
            }
            apply(&item.children, states);
        }
    }
    visit(previous, states);
    apply(items, states);
}

impl Viewer {
    pub(super) fn initialize_browser(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.browser.observer =
            Some(
                cx.observe_in(&self.browser.tree, window, |viewer, tree, window, cx| {
                    let index = tree
                        .read(cx)
                        .selected_item()
                        .and_then(|item| viewer.browser.tree_indices.get(&item.id))
                        .copied();
                    if let Some(index) = index
                        && viewer.selected_index() != Some(index)
                    {
                        viewer.select_browser_entry(index, window, cx);
                    }
                }),
            );
    }

    fn select_browser_entry(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(path) = self
            .browser
            .files
            .get(index)
            .map(|file| file.path().to_owned())
        {
            self.select_path(&path, window, cx);
        }
    }

    pub(super) fn is_retained_preview(&self) -> bool {
        self.selected_path().is_some_and(|path| {
            self.comparison()
                .is_some_and(|comparison| comparison.catalog.find(&path).is_none())
        })
    }

    pub(super) fn rebuild_browser(&mut self, cx: &mut Context<Self>) {
        // Keep the last coherent list until replacement metadata arrives.
        // Selection callbacks require a loaded current comparison, so stale rows
        // cannot open a file from a different commit while it is pending.
        if matches!(self.comparison_state(), LoadState::Loading) {
            return;
        }
        let entries: Vec<_> = self
            .comparison()
            .map(|comparison| {
                if self.browser.mode == FileListMode::Tree && self.browser.scope == TreeScope::All {
                    comparison.catalog.entries().cloned().collect()
                } else {
                    comparison
                        .files
                        .iter()
                        .map(jaywalk::repository_file::FileEntry::from)
                        .collect()
                }
            })
            .unwrap_or_default();
        let open = self.selected_file.as_ref().map(|selected| {
            self.comparison()
                .and_then(|comparison| comparison.catalog.find(selected.file.path()))
                .unwrap_or(&selected.file)
        });
        let (files, retained) = jaywalk::file_browser::with_entries(&entries, open);
        self.browser.retained = retained;
        self.browser.files = files.into();
        let paths: Vec<_> = self.browser.files.iter().map(|file| file.path()).collect();
        let (labels, nodes) = present_paths(&paths);
        self.browser.labels = labels;
        self.browser.tree_indices.clear();
        self.browser.tree_width = 0.;
        let items = tree_items(
            nodes,
            0,
            &mut self.browser.tree_indices,
            &mut self.browser.tree_width,
            self.settings.panels.files.font.size * 0.5,
            self.settings.files.indent_width,
        );
        preserve_expansion(&items, &self.browser.roots, &mut self.browser.expansion);
        let selected_tree = self.browser.tree.read(cx).selected_item().cloned();
        self.browser.roots = items.clone();
        self.browser
            .tree
            .update(cx, |tree, cx| tree.set_items(items, cx));
        if let Some(selected) = selected_tree.filter(|item| item.id.starts_with("dir:")) {
            self.browser.tree.update(cx, |tree, cx| {
                if let Some(index) = tree.index_of(&selected.id) {
                    tree.set_selected_index(Some(index), cx);
                }
            });
        } else if let Some(index) = self.selected_index() {
            self.select_browser_file(index, cx);
        }
    }

    pub(super) fn apply_file_view(&mut self) {
        self.browser.mode = self.settings.files.default_view;
        self.browser.scope = self.settings.files.tree_scope;
    }

    pub(super) fn refresh_browser_metrics(&mut self) {
        let glyph_width = self.settings.panels.files.font.size * 0.5;
        let indent = self.settings.files.indent_width;
        self.browser.tree_width = self
            .browser
            .files
            .iter()
            .flat_map(|file| {
                file.path().split('/').enumerate().map(|(depth, name)| {
                    name.chars().count() as f32 * glyph_width + depth as f32 * indent + 64.
                })
            })
            .fold(0., f32::max);
    }

    pub(super) fn select_browser_file(&self, index: usize, cx: &mut Context<Self>) {
        let Some(file) = self.browser.files.get(index) else {
            return;
        };
        let id: SharedString = format!("file:{}", file.path()).into();
        self.browser.tree.update(cx, |tree, cx| {
            if let Some(row) = tree.index_of(&id) {
                tree.set_selected_index(Some(row), cx);
            }
        });
    }

    pub(super) fn reveal_browser_selection(&self, cx: &mut Context<Self>) {
        fn find<'a>(items: &'a [TreeItem], id: &str) -> Option<&'a TreeItem> {
            for item in items {
                if item.id.as_ref() == id {
                    return Some(item);
                }
                if let Some(found) = find(&item.children, id) {
                    return Some(found);
                }
            }
            None
        }
        let Some(path) = self.selected_path() else {
            return;
        };
        if let Some(item) = find(&self.browser.roots, &format!("file:{path}")) {
            self.browser
                .tree
                .update(cx, |tree, cx| tree.set_selected_item(Some(item), cx));
        }
    }

    fn render_tree(&self) -> AnyElement {
        let files = self.browser.files.clone();
        let retained = self.browser.retained;
        let indices = self.browser.tree_indices.clone();
        let row_height = self.settings.row_height(
            self.settings.files.tree_row_height,
            self.settings.panels.files.font.size,
        );
        let indent_width = self.settings.files.indent_width;
        let font_size = self.settings.panels.files.font.size;
        let tree = Tree::new(&self.browser.tree, move |_, entry, _, _, _| {
            let file_index = indices.get(&entry.item().id).copied();
            let temporary = file_index.is_some() && file_index == retained;
            let color = if temporary {
                0xa4b4c2
            } else {
                file_index
                    .and_then(|index| files.get(index))
                    .map_or(0xa4b4c2, |file| {
                        file_style(file.status().unwrap_or("unchanged")).1
                    })
            };
            let mut content = div().h_flex().items_center().gap_1().whitespace_nowrap();
            if entry.is_folder() {
                content = content.child(if entry.is_expanded() {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                });
            } else {
                content = content.child(div().w(px(14.)).flex_shrink_0());
            }
            content = content.child(
                div()
                    .whitespace_nowrap()
                    .flex_shrink_0()
                    .when(temporary, |label| label.italic())
                    .child(entry.item().label.clone()),
            );
            ListItem::new(entry.item().id.clone())
                .h(px(row_height))
                .py_0()
                .text_size(px(font_size))
                .pl(px(entry.depth() as f32 * indent_width + 8.))
                .text_color(rgb(color))
                .child(content)
        })
        .w(px(self.browser.tree_width.max(self.browser.width)))
        .h_full();
        div()
            .id("tree-horizontal")
            .relative()
            .size_full()
            .overflow_x_hidden()
            .track_scroll(&self.browser.horizontal)
            .child(tree)
            .child(ScrollableMask::new(
                Axis::Horizontal,
                &self.browser.horizontal,
            ))
            .into_any_element()
    }

    pub(super) fn render_files(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut panel = div()
            .relative()
            .v_flex()
            .size_full()
            .bg(gpui::component::Theme::global(cx).sidebar)
            .child(
                self.panel_header(self.settings.panels.files.font.size, cx)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(self.settings.panels.files.font.size * 0.85))
                            .child(
                                if matches!(self.comparison_state(), LoadState::Loading)
                                    && self.loading_visible(LoadingKind::Comparison)
                                {
                                    "FILES · Loading…"
                                } else {
                                    "FILES"
                                },
                            ),
                    )
                    .child(
                        div()
                            .h_flex()
                            .gap_1()
                            .child(self.panel_menu(jaywalk::panel_layout::PanelId::Files))
                            .children((self.browser.mode == FileListMode::Tree).then(|| {
                                Button::new("file-tree-scope")
                                    .ghost()
                                    .small()
                                    .compact()
                                    .label(if self.browser.scope == TreeScope::All {
                                        "All"
                                    } else {
                                        "Changed"
                                    })
                                    .tooltip("Toggle changed files / all repository files")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.browser.scope = if this.browser.scope == TreeScope::All
                                        {
                                            TreeScope::Changed
                                        } else {
                                            TreeScope::All
                                        };
                                        this.rebuild_browser(cx);
                                        this.save_layout(cx);
                                        cx.notify();
                                    }))
                            }))
                            .child(
                                Button::new("file-evolution-toggle")
                                    .ghost()
                                    .small()
                                    .compact()
                                    .icon(IconName::GitCompareArrows)
                                    .when(!self.supports_evolution(), |button| button.hidden())
                                    .tooltip("File evolution")
                                    .selected(self.file_evolution.visible)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.toggle_evolution_panel(
                                            evolution::EvolutionKind::File,
                                            window,
                                            cx,
                                        )
                                    })),
                            )
                            .child(
                                Button::new("file-list-mode")
                                    .ghost()
                                    .small()
                                    .compact()
                                    .icon(IconName::List)
                                    .tooltip("File list")
                                    .selected(self.browser.mode == FileListMode::List)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.browser.mode = FileListMode::List;
                                        this.rebuild_browser(cx);
                                        this.save_layout(cx);
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("file-tree-mode")
                                    .ghost()
                                    .small()
                                    .compact()
                                    .icon(IconName::FolderTree)
                                    .tooltip("Directory tree")
                                    .selected(self.browser.mode == FileListMode::Tree)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.browser.mode = FileListMode::Tree;
                                        this.rebuild_browser(cx);
                                        this.save_layout(cx);
                                        cx.notify();
                                    })),
                            ),
                    ),
            );
        match self.comparison_state() {
            LoadState::Failed(error) => {
                panel = panel.child(
                    div()
                        .p_3()
                        .child(feedback::error_message("files-error", &error)),
                )
            }
            LoadState::Loaded(comparison)
                if comparison.files.is_empty()
                    && !(self.browser.mode == FileListMode::Tree
                        && self.browser.scope == TreeScope::All) =>
            {
                panel = panel.child(div().p_3().child("No changed files"));
            }
            _ => {}
        }
        if !self.browser.files.is_empty() {
            if self.browser.mode == FileListMode::Tree {
                panel = panel.child(div().flex_1().min_h_0().child(self.render_tree()));
            } else {
                let wrap = self.settings.files.list_word_wrap;
                let mut list = div()
                    .id("changed-files")
                    .track_scroll(&self.browser.list_scroll)
                    .flex_1()
                    .min_h_0()
                    .when(wrap, |list| list.overflow_y_scroll())
                    .when(!wrap, |list| list.overflow_scroll());
                let columns = ((self.browser.width - 52.)
                    / (self.settings.panels.files.font.size * 0.5))
                    .max(1.) as usize;
                for (index, file) in self.browser.files.iter().enumerate() {
                    let temporary = self.browser.retained == Some(index);
                    let (marker, color) = if temporary {
                        ("·", 0xa4b4c2)
                    } else {
                        file_style(file.status().unwrap_or("unchanged"))
                    };
                    let label = self.browser.labels.get(index).map_or_else(
                        || file.path().to_owned(),
                        |path| {
                            path.label(if self.settings.files.compact_paths && !wrap {
                                columns
                            } else {
                                usize::MAX
                            })
                            .to_owned()
                        },
                    );
                    let path = file.path().to_owned();
                    let source = self.comparison().map(Comparison::identity);
                    list = list.child(
                        div()
                            .id(SharedString::from(format!("file:{}", file.path())))
                            .h_flex()
                            .items_center()
                            .gap_2()
                            .px_3()
                            .min_h(px(self.settings.row_height(
                                self.settings.files.list_row_height,
                                self.settings.panels.files.font.size,
                            )))
                            .flex_shrink_0()
                            .w_full()
                            .when(wrap, |row| row.items_start())
                            .when(!wrap, |row| row.whitespace_nowrap())
                            .bg(if self.selected_index() == Some(index) {
                                gpui::component::Theme::global(cx).list_active
                            } else {
                                gpui::component::Theme::global(cx).sidebar
                            })
                            .hover(|style| {
                                style.bg(gpui::component::Theme::global(cx).secondary_hover)
                            })
                            .text_size(px(self.settings.panels.files.font.size))
                            .cursor_pointer()
                            .text_color(rgb(color))
                            .child(div().w(px(14.)).flex_shrink_0().child(marker))
                            .child(
                                div()
                                    .when(wrap, |label| {
                                        label.whitespace_normal().flex_1().min_w_0()
                                    })
                                    .when(!wrap, |label| label.whitespace_nowrap().flex_shrink_0())
                                    .when(temporary, |label| label.italic())
                                    .child(label),
                            )
                            .on_click(cx.listener(move |this, _, window, cx| {
                                if source.as_ref().is_some_and(|source| {
                                    this.comparison()
                                        .is_some_and(|comparison| comparison.identity() == *source)
                                }) {
                                    this.select_path(&path, window, cx);
                                }
                            })),
                    );
                }
                panel = panel.child(list);
            }
        }
        let viewer = cx.entity().downgrade();
        panel
            .child(
                canvas(
                    move |bounds, _, cx| {
                        let _ = viewer.update(cx, |viewer, cx| {
                            let width = f32::from(bounds.size.width);
                            if (viewer.browser.width - width).abs() > 1. {
                                viewer.browser.width = width;
                                cx.notify();
                            }
                        });
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .into_any_element()
    }
}

impl Viewer {
    pub(super) fn preserve_row_positions(&self, previous: &Settings, cx: &App) {
        let old_history = previous.row_height(
            previous.history.row_height,
            previous.panels.history.font.size,
        );
        let new_history = self.settings.row_height(
            self.settings.history.row_height,
            self.settings.panels.history.font.size,
        );
        if old_history != new_history {
            let index = self
                .history_scroll
                .0
                .borrow()
                .base_handle
                .logical_scroll_top()
                .0;
            self.history_scroll
                .scroll_to_item_strict(index, ScrollStrategy::Top);
        }
        let old_tree = previous.row_height(
            previous.files.tree_row_height,
            previous.panels.files.font.size,
        );
        let new_tree = self.settings.row_height(
            self.settings.files.tree_row_height,
            self.settings.panels.files.font.size,
        );
        if old_tree != new_tree {
            let scroll = self.browser.tree.read(cx).scroll_handle();
            let index = scroll.0.borrow().base_handle.logical_scroll_top().0;
            scroll.scroll_to_item_strict(index, ScrollStrategy::Top);
        }
        let old_list = previous.row_height(
            previous.files.list_row_height,
            previous.panels.files.font.size,
        );
        let new_list = self.settings.row_height(
            self.settings.files.list_row_height,
            self.settings.panels.files.font.size,
        );
        if old_list != new_list {
            let index = self.browser.list_scroll.logical_scroll_top().0;
            self.browser.list_scroll.scroll_to_top_of_item(index);
        }
    }
}

#[cfg(test)]
mod identity_tests {
    use super::{TreeItem, preserve_expansion};
    use std::collections::HashMap;
    #[test]
    fn expansion_survives_reordering_and_collapsed_ancestors() {
        let previous = vec![
            TreeItem::new("dir:src", "src").expanded(false).child(
                TreeItem::new("dir:src/nested", "nested")
                    .expanded(false)
                    .child(TreeItem::new("file:src/nested/a.rs", "a.rs")),
            ),
        ];
        let next = vec![
            TreeItem::new("dir:new", "new")
                .child(TreeItem::new("file:new/a", "a"))
                .expanded(true),
            TreeItem::new("dir:src", "src").expanded(true).child(
                TreeItem::new("dir:src/nested", "nested")
                    .expanded(true)
                    .child(TreeItem::new("file:src/nested/b.rs", "b.rs")),
            ),
        ];
        let mut states = HashMap::new();
        preserve_expansion(&next, &previous, &mut states);
        assert!(next[0].is_expanded());
        assert!(!next[1].is_expanded());
        assert!(!next[1].children[0].is_expanded());
        preserve_expansion(&[], &next, &mut states);
        let restored = vec![
            TreeItem::new("dir:src", "src")
                .expanded(true)
                .child(TreeItem::new("file:src/a", "a")),
        ];
        preserve_expansion(&restored, &[], &mut states);
        assert!(!restored[0].is_expanded());
    }
}
