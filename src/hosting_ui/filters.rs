//! Keyboard-only filter steps share discovery state but keep their own query.
use super::*;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Step {
    #[default]
    Results,
    Filters,
    Scope,
    Account,
    Repository,
}
#[derive(Clone)]
enum FilterChoice {
    Step(Step),
    Scope(Option<Scope>),
    Account(Option<AccountKey>),
    Refresh,
}
impl PullRequestsPage {
    pub(super) fn account_label(&self, cx: &App) -> String {
        self.account.as_ref().map_or_else(
            || "All accounts".into(),
            |key| {
                self.accounts(cx)
                    .iter()
                    .find(|account| &account.identity.key == key)
                    .map(|account| format!("{} · {}", key.provider.name(), account.identity.label))
                    .unwrap_or_else(|| "Unavailable account".into())
            },
        )
    }
    fn filter_choices(&self, cx: &App) -> Vec<(String, FilterChoice)> {
        match self.step {
            Step::Results => vec![],
            Step::Filters => vec![
                ("Scope…".into(), FilterChoice::Step(Step::Scope)),
                (
                    format!("Account… · {}", self.account_label(cx)),
                    FilterChoice::Step(Step::Account),
                ),
                (
                    "Repository… · Current checkout remotes".into(),
                    FilterChoice::Step(Step::Repository),
                ),
                ("Refresh pull requests".into(), FilterChoice::Refresh),
            ],
            Step::Scope => vec![
                (
                    "My open PRs".into(),
                    FilterChoice::Scope(Some(Scope::Authored)),
                ),
                (
                    "Review requested · GitHub".into(),
                    FilterChoice::Scope(Some(Scope::ReviewRequested)),
                ),
                ("Recent PRs".into(), FilterChoice::Scope(None)),
                (
                    "Current repository…".into(),
                    FilterChoice::Step(Step::Repository),
                ),
            ],
            Step::Account => std::iter::once(("All accounts".into(), FilterChoice::Account(None)))
                .chain(self.accounts(cx).into_iter().map(|account| {
                    (
                        format!(
                            "{} · {}",
                            account.identity.key.provider.name(),
                            account.identity.label
                        ),
                        FilterChoice::Account(Some(account.identity.key)),
                    )
                }))
                .collect(),
            Step::Repository => self
                .remotes
                .iter()
                .map(|remote| {
                    (
                        format!("{} · {}", remote.provider.name(), remote.repository),
                        FilterChoice::Scope(Some(Scope::Repository(
                            remote.provider,
                            remote.repository.clone(),
                        ))),
                    )
                })
                .collect(),
        }
    }
    fn enter_step(&mut self, step: Step, window: &mut Window, cx: &mut Context<Self>) {
        if step == Step::Results {
            self.filter_stack.clear();
        } else if step != self.step {
            self.filter_stack.push(self.step);
        }
        self.show_step(step, window, cx);
    }
    fn show_step(&mut self, step: Step, window: &mut Window, cx: &mut Context<Self>) {
        self.step = step;
        if step == Step::Results {
            self.focus(window, cx);
        } else {
            self.filter_input.update(cx, |input, cx| {
                input.set_query("", window, cx);
                input.focus(window, cx);
            });
        }
        cx.notify();
    }
    pub(super) fn back_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let step = self.filter_stack.pop().unwrap_or(Step::Results);
        self.show_step(step, window, cx);
    }
    fn apply_filter(&mut self, choice: FilterChoice, window: &mut Window, cx: &mut Context<Self>) {
        match choice {
            FilterChoice::Step(step) => {
                self.enter_step(step, window, cx);
                return;
            }
            FilterChoice::Scope(scope) => {
                let query = self.input.read(cx).query(cx).to_string();
                self.set_scope(scope, window, cx);
                self.input
                    .update(cx, |input, cx| input.set_query(query, window, cx));
            }
            FilterChoice::Account(account) => {
                self.account = account;
                self.refresh(window, cx);
            }
            FilterChoice::Refresh => self.force_refresh(window, cx),
        }
        self.enter_step(Step::Results, window, cx);
    }
    pub(super) fn filter_command(&self, cx: &mut Context<Self>) -> Command {
        let choices = self.filter_choices(cx);
        let labels = choices
            .iter()
            .map(|(label, _)| CommandItem::new().label(label.clone()))
            .collect::<Vec<_>>();
        let weak = cx.entity().downgrade();
        let cancel = weak.clone();
        Command::new(&self.filter_input)
            .placeholder(match self.step {
                Step::Filters => "Change PR filters…",
                Step::Scope => "Choose PR scope…",
                Step::Account => "Choose hosting account…",
                Step::Repository => "Choose a repository from this checkout…",
                Step::Results => unreachable!("results use the PR command"),
            })
            .items(labels)
            .on_confirm(move |index, window, cx| {
                if let Some((_, choice)) = choices.get(index.row) {
                    let _ =
                        weak.update(cx, |page, cx| page.apply_filter(choice.clone(), window, cx));
                }
            })
            .on_cancel(move |window, cx| {
                let _ = cancel.update(cx, |page, cx| page.back_filter(window, cx));
            })
    }
    pub(super) fn filter_context(&self, cx: &App) -> [String; 2] {
        let title = match self.step {
            Step::Filters => "PR filters",
            Step::Scope => "PR scope",
            Step::Account => "Hosting account",
            Step::Repository => "Current repository remotes",
            Step::Results => unreachable!("results use PR context"),
        };
        let hint = if self.step == Step::Repository && self.remotes.is_empty() {
            "No recognized remotes · Paste a PR URL in results, or open a repository"
        } else if self.step == Step::Filters {
            "Choose a filter · Escape returns to PR results"
        } else {
            "Choose a value · Escape goes back one step"
        };
        [format!("{title} · {}", self.account_label(cx)), hint.into()]
    }
    pub(super) fn toggle_filters(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.enter_step(
            if self.step == Step::Results {
                Step::Filters
            } else {
                Step::Results
            },
            window,
            cx,
        );
        cx.stop_propagation();
    }
    pub(super) fn filter_escape(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.step == Step::Results {
            cx.propagate();
        } else {
            self.back_filter(window, cx);
            cx.stop_propagation();
        }
    }
}
