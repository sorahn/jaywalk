use super::*;
use gpui::component::Sizable;
use gpui::component::list::ListItem;
use gpui::component::menu::ContextMenuExt as _;
use gpui::component::scroll::ScrollableMask;
use gpui::component::tree::{Tree, TreeItem, TreeState};
use gpui::prelude::FluentBuilder;
use jaywalk::file_browser::{CompactPath, FileNode, present_paths};
use std::collections::HashMap;

use jaywalk::settings::{FileView as FileListMode, TreeScope};

use super::chrome::PANEL_INSET;

/// Canvas bounds start inside the row's indentation padding. Subtract the
/// current depth before locating ancestor icon centers in window coordinates.
fn ancestor_guide_positions(
    content_left: f32,
    depth: usize,
    indent: f32,
    icon_width: f32,
    offset: f32,
) -> impl Iterator<Item = f32> {
    let root = content_left - depth as f32 * indent;
    (0..depth).map(move |ancestor| root + ancestor as f32 * indent + icon_width / 2. + offset)
}

/// The canvas occupies the centered label content, which can be shorter than
/// the fixed tree row. Extend guides to the row edges so adjacent segments join.
fn guide_span(content: std::ops::Range<f32>, row_height: f32) -> std::ops::Range<f32> {
    let center = (content.start + content.end) / 2.;
    center - row_height / 2. ..center + row_height / 2.
}

pub(super) struct FileBrowser {
    focus: FocusHandle,
    pub(super) files: std::sync::Arc<[jaywalk::repository_file::FileEntry]>,
    retained: Option<usize>,
    pub(super) reading: Option<String>,
    reading_reveal: bool,
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
            focus: cx.focus_handle(),
            files: std::sync::Arc::from([]),
            retained: None,
            reading: None,
            reading_reveal: false,
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

fn file_style(status: &str, dark: bool) -> (&'static str, u32) {
    if !dark {
        return match status {
            "added" => ("A", 0x22863a),
            "removed" => ("D", 0xc12e2a),
            "renamed" => ("R", 0x0969da),
            "copied" => ("C", 0x0969da),
            "unchanged" => ("", 0x57606a),
            _ => ("M", 0x8a6116),
        };
    }
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
    expanded: bool,
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
                        expanded,
                    );
                    (
                        name.clone(),
                        TreeItem::new(format!("dir:{path}"), name)
                            .children(children)
                            .expanded(expanded),
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
            self.browser.scope == TreeScope::Changed,
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
        } else {
            self.browser
                .tree
                .update(cx, |tree, cx| tree.set_selected_index(None, cx));
        }
        if self.selected_file.is_none() {
            self.browser.reading_reveal = self.browser.reading.is_some();
        }
    }

    pub(super) fn track_overview_file(&mut self, path: Option<String>, cx: &mut Context<Self>) {
        if self.browser.reading == path {
            return;
        }
        self.browser.reading = path;
        self.browser.reading_reveal = true;
        cx.notify();
    }

    fn reading_tree_target(&self, cx: &App) -> Option<String> {
        let path = self.browser.reading.as_deref()?;
        let tree = self.browser.tree.read(cx);
        jaywalk::file_browser::reading_tree_target(path, |id| {
            tree.index_of(&SharedString::from(id.to_owned())).is_some()
        })
    }

    fn reveal_reading_file(&self, cx: &mut Context<Self>) {
        if self.browser.mode == FileListMode::Tree {
            if let Some(id) = self.reading_tree_target(cx) {
                self.browser.tree.update(cx, |tree, _| {
                    if let Some(index) = tree.index_of(&id.into()) {
                        tree.scroll_to_item(index, ScrollStrategy::Nearest);
                    }
                });
            }
        } else if let Some(index) = self.browser.reading.as_ref().and_then(|path| {
            self.browser
                .files
                .iter()
                .position(|file| file.path() == path)
        }) {
            let scroll = &self.browser.list_scroll;
            if let Some(row) = scroll.bounds_for_item(index) {
                let viewport = scroll.bounds();
                let offset = scroll.offset();
                let y = jaywalk::file_browser::reveal_offset(
                    f32::from(viewport.top())..f32::from(viewport.bottom()),
                    f32::from(row.top())..f32::from(row.bottom()),
                    f32::from(offset.y),
                );
                scroll.set_offset(point(offset.x, px(y)));
            }
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

    fn render_tree(&self, cx: &Context<Self>) -> AnyElement {
        let reading = self
            .selected_file
            .is_none()
            .then(|| self.reading_tree_target(cx))
            .flatten();
        let reading_tint = gpui::component::Theme::global(cx).secondary;
        let dark = gpui::component::Theme::global(cx).is_dark();
        let neutral = if dark { 0xa4b4c2 } else { 0x57606a };
        let files = self.browser.files.clone();
        let retained = self.browser.retained;
        let indices = self.browser.tree_indices.clone();
        let row_height = self.settings.row_height(
            self.settings.files.tree_row_height,
            self.settings.panels.files.font.size,
        );
        let indent_width = self.settings.files.indent_width;
        let font_size = self.settings.panels.files.font.size;
        let viewer = cx.entity().downgrade();
        let source = self
            .comparison()
            .map(|comparison| (comparison.identity(), comparison.root.clone()));
        let focus = self.focus.clone();
        let repository = self.arguments.diff.is_none();
        let icons = self.settings.files.file_icons;
        let guides = self.settings.files.indent_guides;
        let guide_offset = self.settings.files.indent_guide_offset;
        let guide_color = gpui::component::Theme::global(cx)
            .foreground
            .opacity(self.settings.files.indent_guide_opacity);
        let tree = Tree::new(&self.browser.tree, move |_, entry, _, _, _| {
            let file_index = indices.get(&entry.item().id).copied();
            let temporary = file_index.is_some() && file_index == retained;
            let color = if temporary {
                neutral
            } else {
                file_index
                    .and_then(|index| files.get(index))
                    .map_or(neutral, |file| {
                        file_style(file.status().unwrap_or("unchanged"), dark).1
                    })
            };
            let mut content = div().h_flex().items_center().gap_1().whitespace_nowrap();
            let icon = if entry.is_folder() {
                Some(if entry.is_expanded() {
                    IconName::FolderOpen
                } else {
                    IconName::Folder
                })
            } else if icons {
                file_index
                    .and_then(|index| files.get(index))
                    .map(file_icons::file_icon)
            } else {
                None
            };
            content =
                content.child(div().w(px(font_size)).flex_shrink_0().children(
                    icon.map(|icon| gpui::component::Icon::new(icon).size(px(font_size))),
                ));
            content = content.child(
                div()
                    .whitespace_nowrap()
                    .flex_shrink_0()
                    .when(temporary, |label| label.italic())
                    .child(entry.item().label.clone()),
            );
            let depth = entry.depth();
            ListItem::new(entry.item().id.clone())
                .h(px(row_height))
                .py_0()
                .text_size(px(font_size))
                .pl(px(entry.depth() as f32 * indent_width + PANEL_INSET))
                .text_color(rgb(color))
                .when(
                    reading.as_deref() == Some(entry.item().id.as_ref()),
                    |row| row.bg(reading_tint),
                )
                .when(guides && depth > 0, |row| {
                    row.child(
                        canvas(
                            |_, _, _| (),
                            move |bounds, _, window, _| {
                                let span = guide_span(
                                    f32::from(bounds.top())..f32::from(bounds.bottom()),
                                    row_height,
                                );
                                for x in ancestor_guide_positions(
                                    f32::from(bounds.left()),
                                    depth,
                                    indent_width,
                                    font_size,
                                    guide_offset,
                                ) {
                                    window.paint_quad(fill(
                                        Bounds::new(
                                            point(px(x), px(span.start)),
                                            size(px(1.), px(span.end - span.start)),
                                        ),
                                        guide_color,
                                    ));
                                }
                            },
                        )
                        .absolute()
                        .inset_0(),
                    )
                })
                .child(content)
        })
        .context_menu(move |_, entry, menu, _, _| {
            let Some((prefix, path)) = entry.item().id.split_once(':') else {
                return menu;
            };
            let Some((source, root)) = &source else {
                return menu;
            };
            let target = context_menu::FileTarget::new(
                source.clone(),
                root,
                path.to_owned(),
                prefix == "dir",
                repository,
            );
            let menu = menu.action_context(focus.clone());
            context_menu::file_menu(menu, target, viewer.clone())
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
        let dark = gpui::component::Theme::global(cx).is_dark();
        let toggle_icon =
            |name: IconName, active: bool| crate::appearance::active_icon(name, active, cx);
        let mut panel = div()
            .track_focus(&self.browser.focus)
            .key_context("JaywalkFiles")
            .relative()
            .v_flex()
            .size_full()
            .bg(gpui::component::Theme::global(cx).sidebar)
            .child(
                self.panel_header(
                    jaywalk::panel_layout::PanelId::Files,
                    self.settings.panels.files.font.size,
                    cx,
                )
                .child(div().flex_1().min_w_0().truncate().child(
                    if matches!(self.comparison_state(), LoadState::Loading)
                        && self.loading_visible(LoadingKind::Comparison)
                    {
                        "FILES · Loading…"
                    } else {
                        "FILES"
                    },
                ))
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
                                    this.browser.scope = if this.browser.scope == TreeScope::All {
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
                                .icon(toggle_icon(
                                    IconName::GitCompareArrows,
                                    self.file_evolution.visible,
                                ))
                                .when(!self.supports_evolution(), |button| button.hidden())
                                .tooltip("File evolution")
                                .toggled(self.file_evolution.visible)
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
                                .icon(toggle_icon(
                                    IconName::List,
                                    self.browser.mode == FileListMode::List,
                                ))
                                .tooltip("File list")
                                .toggled(self.browser.mode == FileListMode::List)
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
                                .icon(toggle_icon(
                                    IconName::FolderTree,
                                    self.browser.mode == FileListMode::Tree,
                                ))
                                .tooltip("Directory tree")
                                .toggled(self.browser.mode == FileListMode::Tree)
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
                panel = panel.child(div().flex_1().min_h_0().child(self.render_tree(cx)));
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
                        ("·", if dark { 0xa4b4c2 } else { 0x57606a })
                    } else {
                        file_style(file.status().unwrap_or("unchanged"), dark)
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
                    let target = self.file_target(path.clone(), false);
                    let owner = cx.entity().downgrade();
                    let focus = self.focus.clone();
                    list = list.child(
                        div()
                            .id(SharedString::from(format!("file:{}", file.path())))
                            .h_flex()
                            .items_center()
                            .gap_2()
                            .pl(px(PANEL_INSET))
                            .pr_3()
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
                            } else if self.selected_file.is_none()
                                && self.browser.reading.as_deref() == Some(file.path())
                            {
                                gpui::component::Theme::global(cx).secondary
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
                            .when(self.settings.files.file_icons, |row| {
                                row.child(
                                    gpui::component::Icon::new(file_icons::file_icon(file))
                                        .size(px(self.settings.panels.files.font.size))
                                        .flex_shrink_0(),
                                )
                            })
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
                            }))
                            .context_menu(move |menu, _, _| match &target {
                                Some(target) => context_menu::file_menu(
                                    menu.action_context(focus.clone()),
                                    target.clone(),
                                    owner.clone(),
                                ),
                                None => menu,
                            }),
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
                            if viewer.browser.reading_reveal && viewer.selected_file.is_none() {
                                viewer.browser.reading_reveal = false;
                                viewer.reveal_reading_file(cx);
                                cx.notify();
                            }
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
    fn all_files_only_expands_known_folders_and_keeps_new_descendants_closed() {
        use super::tree_items;
        use jaywalk::file_browser::present_paths;
        fn items(paths: &[&str], expanded: bool) -> Vec<TreeItem> {
            let (_, nodes) = present_paths(paths);
            tree_items(nodes, 0, &mut HashMap::new(), &mut 0., 8., 12., expanded)
        }
        fn folder<'a>(items: &'a [TreeItem], id: &str) -> &'a TreeItem {
            fn find<'a>(items: &'a [TreeItem], id: &str) -> Option<&'a TreeItem> {
                items.iter().find_map(|item| {
                    if item.id.as_ref() == id {
                        Some(item)
                    } else {
                        find(&item.children, id)
                    }
                })
            }
            find(items, id).expect("fixture folder exists")
        }
        let changed = items(&["src/api/changed.rs", "src/hidden/changed.rs"], true);
        folder(&changed, "dir:src/hidden").clone().expanded(false);
        let all = items(
            &[
                "src/api/changed.rs",
                "src/hidden/changed.rs",
                "src/new/other.rs",
                "docs/guide.md",
            ],
            false,
        );
        preserve_expansion(&all, &changed, &mut HashMap::new());
        assert!(folder(&all, "dir:src").is_expanded());
        assert!(folder(&all, "dir:src/api").is_expanded());
        assert!(!folder(&all, "dir:src/hidden").is_expanded());
        assert!(!folder(&all, "dir:src/new").is_expanded());
        assert!(!folder(&all, "dir:docs").is_expanded());
        let fresh_all = items(&["src/api/a.rs", "docs/a.md"], false);
        assert!(fresh_all.iter().all(|item| !item.is_expanded()));
    }

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

/// Match directory boundaries so folding `src/a` cannot affect `src/ab`.
fn expand_directories(items: &[TreeItem], directory: Option<&str>, expanded: bool) {
    for item in items {
        if let Some(path) = item.id.strip_prefix("dir:") {
            if directory.is_none_or(|directory| {
                path == directory
                    || path
                        .strip_prefix(directory)
                        .is_some_and(|tail| tail.starts_with('/'))
            }) {
                item.clone().expanded(expanded);
            }
            expand_directories(&item.children, directory, expanded);
        }
    }
}

impl Viewer {
    pub(super) fn keyboard_file_target(
        &self,
        window: &Window,
        cx: &App,
    ) -> Option<context_menu::FileTarget> {
        let tree = self.browser.tree.read(cx);
        if self.browser.mode == FileListMode::Tree
            && self.browser.focus.contains_focused(window, cx)
            && let Some(item) = tree.selected_item()
            && let Some((prefix, path)) = item.id.split_once(':')
        {
            return self.file_target(path.to_owned(), prefix == "dir");
        }
        self.current_file_target()
    }

    pub(super) fn set_directory_expansion(
        &mut self,
        directory: Option<&str>,
        expanded: bool,
        cx: &mut Context<Self>,
    ) {
        if self.browser.mode != FileListMode::Tree {
            return;
        }
        let selected = self
            .browser
            .tree
            .read(cx)
            .selected_item()
            .map(|item| item.id.clone());
        expand_directories(&self.browser.roots, directory, expanded);
        let items = self.browser.roots.clone();
        self.browser.tree.update(cx, |tree, cx| {
            tree.set_items(items, cx);
            let index = selected.as_ref().and_then(|id| tree.index_of(id));
            tree.set_selected_index(index, cx);
        });
        cx.notify();
    }

    pub(super) fn set_file_mode(&mut self, mode: FileListMode, cx: &mut Context<Self>) {
        self.browser.mode = mode;
        self.rebuild_browser(cx);
        self.save_layout(cx);
        cx.notify();
    }

    pub(super) fn set_tree_scope(&mut self, scope: TreeScope, cx: &mut Context<Self>) {
        self.browser.scope = scope;
        self.rebuild_browser(cx);
        self.save_layout(cx);
        cx.notify();
    }

    pub(super) fn reveal_active_file(&mut self, cx: &mut Context<Self>) {
        if self.layout.saved.files_hidden {
            self.layout.saved.files_hidden = false;
            self.rebuild_sidebar_layout(cx);
            self.save_layout(cx);
        }
        if self.selected_file.is_some() {
            self.reveal_browser_selection(cx);
        }
        let previous = self.browser.reading.clone();
        self.browser.reading = self.selected_path().or(previous.clone());
        self.reveal_reading_file(cx);
        self.browser.reading = previous;
        cx.notify();
    }
}

#[cfg(test)]
mod expansion_tests {
    use super::{TreeItem, expand_directories};
    #[test]
    fn recursive_folder_actions_include_hidden_descendants_but_not_siblings() {
        let nested = TreeItem::new("dir:src/a/deep", "deep")
            .children(vec![TreeItem::new("file:src/a/deep/f.rs", "f.rs")])
            .expanded(false);
        let first = TreeItem::new("dir:src/a", "a")
            .children(vec![nested.clone()])
            .expanded(false);
        let sibling = TreeItem::new("dir:src/ab", "ab")
            .children(vec![TreeItem::new("file:src/ab/g.rs", "g.rs")])
            .expanded(false);
        let roots = vec![first.clone(), sibling.clone()];
        expand_directories(&roots, Some("src/a"), true);
        assert!(first.is_expanded() && nested.is_expanded());
        assert!(!sibling.is_expanded());
        expand_directories(&roots, None, false);
        assert!(!first.is_expanded() && !nested.is_expanded() && !sibling.is_expanded());
        expand_directories(&roots, None, true);
        assert!(first.is_expanded() && nested.is_expanded() && sibling.is_expanded());
    }
}

#[cfg(test)]
mod guide_tests {
    use super::{ancestor_guide_positions, guide_span};
    #[test]
    fn adjacent_rows_join_even_when_label_content_is_shorter_than_the_row() {
        assert_eq!(guide_span(3.0..23.0, 26.), 0.0..26.0);
        assert_eq!(guide_span(29.0..49.0, 26.), 26.0..52.0);
        assert_eq!(guide_span(70.0..90.0, 40.), 60.0..100.0);
        assert_eq!(guide_span(104.0..136.0, 40.), 100.0..140.0);
    }
    #[test]
    fn ancestor_columns_stay_fixed_as_row_depth_and_horizontal_scroll_change() {
        for root in [8., -42.] {
            for (indent, icon) in [(12., 16.), (20., 24.)] {
                for depth in 0..6 {
                    let content_left = root + depth as f32 * indent;
                    let positions: Vec<_> =
                        ancestor_guide_positions(content_left, depth, indent, icon, -2.).collect();
                    let expected: Vec<_> = (0..depth)
                        .map(|ancestor| root + ancestor as f32 * indent + icon / 2. - 2.)
                        .collect();
                    assert_eq!(positions, expected);
                    assert!(positions.iter().all(|x| *x <= content_left));
                }
            }
        }
    }
}
