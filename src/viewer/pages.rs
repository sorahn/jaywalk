//! Window-local destinations. Page content remains owned while another page is
//! visible; shared account sessions stay in the application service. A future
//! tab strip can select these destinations without changing their lifetimes.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Page {
    Comparison,
    Welcome,
    Accounts,
    PullRequests,
}
/// Only applies to the exact retained standard widget. Source ranges survive
/// Base retiring the hidden surface's window selection registration.
pub(super) struct RetainedSelection {
    state: Entity<DiffState>,
    lines: gpui::component::diff::DiffLineRange,
}
impl Viewer {
    pub(crate) fn is_repository_free(&self) -> bool {
        self.arguments.welcome
    }
    pub(crate) fn show_retained_comparison(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_page(Page::Comparison, window, cx);
    }
    pub(super) fn new_pull_requests(
        &mut self,
        picking: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<crate::hosting_ui::PullRequestsPage>> {
        let root = self.repository_directory();
        let path = match self
            .arguments
            .config
            .clone()
            .map(Ok)
            .unwrap_or_else(jaywalk::settings::default_path)
        {
            Ok(path) => path.with_file_name("recent-pull-requests.json"),
            Err(error) => {
                self.report_open_error(error, cx);
                return None;
            }
        };
        let mut candidates = cx.global::<crate::recents::Recents>().repositories.clone();
        if let Some(root) = &root {
            candidates.insert(0, root.clone());
        }
        let page = cx.new(|cx| {
            let mut page =
                crate::hosting_ui::PullRequestsPage::new(root, path, candidates, window, cx);
            if picking {
                page.as_picker();
            }
            page
        });
        cx.subscribe_in(
            &page,
            window,
            move |viewer, _, event, window, cx| match event {
                crate::hosting_ui::Event::Leave if picking => viewer.close_palette(window, cx),
                crate::hosting_ui::Event::Leave => viewer.navigate_view(false, window, cx),
                crate::hosting_ui::Event::Picked(selection) => {
                    let (account, item) = selection.as_ref();
                    viewer.close_palette(window, cx);
                    viewer.show_pull_requests(window, cx);
                    if let Some(page) = &viewer.pull_requests_page {
                        page.update(cx, |page, cx| {
                            page.open_selected(account.clone(), item.clone(), window, cx);
                        });
                    }
                }
                crate::hosting_ui::Event::Accounts => {
                    if picking {
                        viewer.close_palette(window, cx);
                    }
                    viewer.show_accounts(jaywalk::commands::CommandId::Accounts, window, cx);
                }
                crate::hosting_ui::Event::Repository(path) => viewer.open_repository_path(
                    path.clone(),
                    crate::windows::OpenDisposition::Configured,
                    cx,
                ),
            },
        )
        .detach();
        Some(page)
    }
    pub(super) fn show_pull_requests(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pull_requests_page.is_none() {
            self.pull_requests_page = self.new_pull_requests(false, window, cx);
            if self.pull_requests_page.is_none() {
                return;
            }
        }
        self.show_page(Page::PullRequests, window, cx);
        self.focus_page(window, cx);
    }
    pub(super) fn show_accounts(
        &mut self,
        command: jaywalk::commands::CommandId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.account_page.is_none() {
            if !cx.has_global::<jaywalk::accounts::application::Accounts>() {
                jaywalk::accounts::application::init(cx);
            }
            let page = cx.new(|cx| crate::account_ui::AccountPanel::new(command, window, cx));
            cx.subscribe_in(
                &page,
                window,
                |viewer, _, _: &crate::account_ui::LeavePage, window, cx| {
                    if viewer.navigation.history.can_back() {
                        viewer.navigate_view(false, window, cx);
                    } else {
                        let page = if viewer.arguments.welcome {
                            Page::Welcome
                        } else {
                            Page::Comparison
                        };
                        viewer.show_page(page, window, cx);
                    }
                },
            )
            .detach();
            self.account_page = Some(page);
        }
        self.show_page(Page::Accounts, window, cx);
        self.account_page
            .as_ref()
            .expect("the account page was constructed")
            .update(cx, |page, cx| page.show(command, window, cx));
    }
    pub(super) fn show_page(&mut self, page: Page, window: &mut Window, cx: &mut Context<Self>) {
        if self.page != page {
            // A comparison can still be loading. Record a retained destination
            // so Back returns to those same requests/editors rather than reloading.
            self.checkpoint_page_departure(cx);
            self.activate_page(page, window, cx);
            self.checkpoint_navigation(cx);
        }
    }
    pub(super) fn activate_page(
        &mut self,
        page: Page,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.page == Page::Comparison && page != Page::Comparison {
            self.page_selection = self
                .code()
                .and_then(|code| code.standard.as_ref().ok()?.as_ref())
                .and_then(|state| {
                    state
                        .read(cx)
                        .selected_lines()
                        .map(|lines| RetainedSelection {
                            state: state.clone(),
                            lines,
                        })
                });
        }
        self.page = page;
        if page == Page::Comparison
            && let Some(selection) = self.page_selection.take()
        {
            let current = self
                .code()
                .and_then(|code| code.standard.as_ref().ok()?.as_ref());
            if current.is_some_and(|state| state.entity_id() == selection.state.entity_id()) {
                selection.state.update(cx, |state, cx| {
                    state.set_selected_lines(Some(selection.lines), cx)
                });
            }
        }
        if page == Page::Welcome {
            self.welcome.get_or_insert_default();
        }
        self.fluid_drag = None;
        self.focus_page(window, cx);
        cx.notify();
    }
    pub(super) fn focus_page(&self, window: &mut Window, cx: &mut Context<Self>) {
        if self.page == Page::PullRequests
            && let Some(page) = &self.pull_requests_page
        {
            page.update(cx, |page, cx| page.focus(window, cx));
        } else if self.page == Page::Accounts
            && let Some(accounts) = &self.account_page
        {
            accounts.update(cx, |page, cx| page.focus(window, cx));
        } else {
            window.focus(&self.focus, cx);
        }
    }
    pub(super) fn render_page_header(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .h_flex()
            .items_center()
            .flex_shrink_0()
            .h(px(self.settings.ui.panel_header_height))
            .border_b_1()
            .border_color(gpui::component::Theme::global(cx).border)
            .child(self.render_navigation_buttons(cx))
            .child(div().px_3().text_sm().child(match self.page {
                Page::Welcome => "Welcome",
                Page::Accounts => "Accounts",
                Page::PullRequests => "Pull requests",
                Page::Comparison => unreachable!("comparisons keep their existing header"),
            }))
            .into_any_element()
    }
}
