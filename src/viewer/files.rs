use super::*;
use gpui::component::list::ListItem;
use gpui::component::scroll::ScrollableMask;
use gpui::component::tree::{Tree, TreeItem, TreeState};
use gpui::prelude::FluentBuilder;
use jaywalk::file_browser::{CompactPath, FileNode, present_paths};
use std::collections::HashMap;

#[derive(Default, PartialEq, Eq)]
enum FileListMode {
    #[default]
    List,
    Tree,
}

pub(super) struct FileBrowser {
    mode: FileListMode,
    labels: Vec<CompactPath>,
    tree: Entity<TreeState>,
    tree_indices: HashMap<SharedString, usize>,
    observer: Option<Subscription>,
    horizontal: ScrollHandle,
    width: f32,
    tree_width: f32,
}

impl FileBrowser {
    pub(super) fn new(default_width: f32, cx: &mut App) -> Self {
        Self {
            mode: FileListMode::default(),
            labels: Vec::new(),
            tree: cx.new(|cx| TreeState::new(cx)),
            tree_indices: HashMap::new(),
            observer: None,
            horizontal: ScrollHandle::new(),
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
                FileNode::File { index, name } => {
                    let id: SharedString = format!("file:{index}").into();
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
                        viewer.select_file(index, window, cx);
                    }
                }),
            );
    }

    pub(super) fn rebuild_browser(&mut self, cx: &mut Context<Self>) {
        let files = self.comparison().map(|comparison| comparison.files.clone());
        let paths: Vec<_> = match &files {
            Some(files) => files.iter().map(|file| file.after_path.as_str()).collect(),
            None => Vec::new(), // No displayed comparison while loading or failed.
        };
        let (labels, nodes) = present_paths(&paths);
        self.browser.labels = labels;
        self.browser.tree_indices.clear();
        self.browser.tree_width = 0.;
        let items = tree_items(
            nodes,
            0,
            &mut self.browser.tree_indices,
            &mut self.browser.tree_width,
            self.settings.ui.font_size * 0.5,
            self.settings.files.indent_width,
        );
        self.browser.horizontal.set_offset(point(px(0.), px(0.)));
        self.browser
            .tree
            .update(cx, |tree, cx| tree.set_items(items, cx));
    }

    pub(super) fn select_browser_file(&self, index: usize, cx: &mut Context<Self>) {
        let id: SharedString = format!("file:{index}").into();
        self.browser.tree.update(cx, |tree, cx| {
            if let Some(row) = tree.index_of(&id) {
                tree.set_selected_index(Some(row), cx);
            }
        });
    }

    fn render_tree(&self, comparison: &Comparison) -> AnyElement {
        let files = comparison.files.clone();
        let row_height = self
            .settings
            .files
            .tree_row_height
            .max(self.settings.ui.font_size * 1.25 + 8.);
        let indent_width = self.settings.files.indent_width;
        let tree = Tree::new(&self.browser.tree, move |index, entry, _, _, _| {
            let file_index = entry.item().id.strip_prefix("file:").map(|id| {
                id.parse::<usize>()
                    .expect("file tree IDs are built from file indices")
            });
            let color = file_index
                .and_then(|index| files.get(index))
                .map_or(0xa4b4c2, |file| file_style(&file.status).1);
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
                    .child(entry.item().label.clone()),
            );
            ListItem::new(("file-tree", index))
                .h(px(row_height))
                .text_sm()
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
            .bg(rgb(0x253448))
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .justify_between()
                    .px_3()
                    .py_2()
                    .child(div().text_xs().child("CHANGED FILES"))
                    .child(
                        div()
                            .h_flex()
                            .gap_1()
                            .child(
                                Button::new("file-evolution-toggle")
                                    .ghost()
                                    .compact()
                                    .icon(IconName::GitCompareArrows)
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
                                    .compact()
                                    .icon(IconName::List)
                                    .tooltip("File list")
                                    .selected(self.browser.mode == FileListMode::List)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.browser.mode = FileListMode::List;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("file-tree-mode")
                                    .ghost()
                                    .compact()
                                    .icon(IconName::FolderTree)
                                    .tooltip("Directory tree")
                                    .selected(self.browser.mode == FileListMode::Tree)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.browser.mode = FileListMode::Tree;
                                        cx.notify();
                                    })),
                            ),
                    ),
            );
        match self.comparison_state() {
            LoadState::Loading => panel = panel.child(div().p_3().child("Loading comparison…")),
            LoadState::Failed(error) => panel = panel.child(div().p_3().child(error)),
            LoadState::Loaded(comparison) if comparison.files.is_empty() => {
                panel = panel.child(div().p_3().child("No changed files"))
            }
            LoadState::Loaded(comparison) if self.browser.mode == FileListMode::Tree => {
                panel = panel.child(div().flex_1().min_h_0().child(self.render_tree(comparison)));
            }
            LoadState::Loaded(comparison) => {
                let wrap = self.settings.files.list_word_wrap;
                let mut list = div()
                    .id("changed-files")
                    .flex_1()
                    .min_h_0()
                    .when(wrap, |list| list.overflow_y_scroll())
                    .when(!wrap, |list| list.overflow_scroll());
                let columns = ((self.browser.width - 52.) / (self.settings.ui.font_size * 0.4375))
                    .max(1.) as usize;
                for (index, file) in comparison.files.iter().enumerate() {
                    let (marker, color) = file_style(&file.status);
                    let label = self.browser.labels.get(index).map_or_else(
                        || file.after_path.clone(),
                        |path| {
                            path.label(if self.settings.files.compact_paths && !wrap {
                                columns
                            } else {
                                usize::MAX
                            })
                            .to_owned()
                        },
                    );
                    list = list.child(
                        div()
                            .id(("file", index))
                            .h_flex()
                            .items_center()
                            .gap_2()
                            .px_3()
                            .py_2()
                            .w_full()
                            .when(wrap, |row| row.items_start())
                            .when(!wrap, |row| row.whitespace_nowrap())
                            .bg(rgb(if self.selected_index() == Some(index) {
                                0x345577
                            } else {
                                0x253448
                            }))
                            .text_sm()
                            .cursor_pointer()
                            .text_color(rgb(color))
                            .child(div().w(px(14.)).flex_shrink_0().child(marker))
                            .child(
                                div()
                                    .when(wrap, |label| {
                                        label.whitespace_normal().flex_1().min_w_0()
                                    })
                                    .when(!wrap, |label| label.whitespace_nowrap().flex_shrink_0())
                                    .child(label),
                            )
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.select_file(index, window, cx)
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
