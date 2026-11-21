//! Optional setup in the retained central workspace; no work in render.
use super::*;
use jaywalk::{cli_install, settings::Appearance, setup::UpdateSettings};
use std::path::PathBuf;

pub(super) struct State {
    updates: UpdateSettings,
    appearance: Appearance,
    message: Option<String>,
    busy: bool,
    cli: Option<Result<cli_install::Discovery, String>>,
    cli_task: Option<Task<()>>,
}
impl State {
    pub(super) fn new(settings: &Settings) -> Self {
        Self {
            updates: settings.updates,
            appearance: settings.ui.appearance,
            message: None,
            busy: false,
            cli: None,
            cli_task: None,
        }
    }
}
impl Viewer {
    pub(super) fn show_setup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.setup.busy {
            self.setup = State::new(&self.settings);
        }
        self.check_setup_cli(window, cx);
        self.show_page(pages::Page::Setup, window, cx);
    }
    fn finish_setup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.setup.busy {
            return;
        }
        self.setup.busy = true;
        let explicit = self.arguments.config.clone();
        let updates = self.setup.updates;
        let appearance = self.setup.appearance;
        cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let source =
                        jaywalk::settings_reload::SettingsSource::resolve(explicit.as_deref())?;
                    source.initialize()?;
                    jaywalk::setup::save(source.path(), true, updates, appearance)?;
                    source.read()
                })
                .await;
            let _ = this.update_in(cx, |viewer, window, cx| {
                viewer.setup.busy = false;
                match result {
                    Ok(settings) => {
                        viewer.receive_settings(settings, None, window, cx);
                        let page = if viewer.arguments.welcome {
                            pages::Page::Welcome
                        } else {
                            pages::Page::Comparison
                        };
                        if viewer.page == pages::Page::Setup {
                            viewer.show_page(page, window, cx);
                        }
                    }
                    Err(error) => viewer.setup.message = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }
    /// Read the filesystem off the UI thread before revealing the setup form.
    /// Retain results only in this setup state; replacing it cancels its task.
    pub(super) fn check_setup_cli(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.setup.cli = None;
        self.setup.cli_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let home = std::env::var_os("HOME")
                        .map(PathBuf::from)
                        .ok_or("Home directory is unavailable")?;
                    let directory = std::env::current_dir().map_err(|e| e.to_string())?;
                    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
                    // A temporary/translocated app can still detect an existing CLI;
                    // the explicit install action retains the permanent-app guard.
                    let source = executable
                        .parent()
                        .filter(|p| p.ends_with("Contents/MacOS"))
                        .map(|p| p.join("jw"));
                    cli_install::discover(
                        &home,
                        std::env::var_os("PATH").as_deref(),
                        &directory,
                        source.as_deref(),
                    )
                })
                .await;
            let _ = this.update(cx, |v, cx| {
                v.setup.cli = Some(result);
                cx.notify();
            });
        }));
    }
    pub(super) fn render_setup(&self, width: f32, cx: &mut Context<Self>) -> AnyElement {
        use gpui::component::{Sizable, checkbox::Checkbox};
        use gpui::prelude::FluentBuilder;
        use jaywalk::commands::CommandId;
        let theme = gpui::component::Theme::global(cx);
        let muted = theme.muted_foreground;
        let Some(discovery) = &self.setup.cli else {
            return div()
                .id("setup-loading")
                .size_full()
                .v_flex()
                .items_center()
                .justify_center()
                .text_color(muted)
                .child("Checking command-line integration…")
                .into_any_element();
        };
        let wide = width >= self.settings.ui.font_size * 64.;
        let (installed, label, detail) = match discovery {
            Ok(cli_install::Discovery::Available {
                path,
                on_path,
                current_app,
            }) => (
                true,
                if *on_path {
                    "jw command available"
                } else {
                    "jw command installed"
                },
                format!(
                    "{}{}",
                    path.display(),
                    if !on_path {
                        " · Confirm this folder is on your terminal PATH."
                    } else if !current_app {
                        " · Existing command; CLI options shows its location."
                    } else {
                        ""
                    }
                ),
            ),
            Ok(cli_install::Discovery::Missing) => (
                false,
                "Install the jw command",
                "Open repositories and diffs from your terminal.".into(),
            ),
            Err(error) => (false, "Check command-line integration", error.clone()),
        };
        let mut local = div()
            .id("setup-local")
            .debug_selector(|| "setup-local".into())
            .v_flex()
            .min_w_0()
            .gap_3()
            .when(wide, |el| el.flex_1())
            .when(!wide, |el| el.w_full())
            .child(div().text_lg().font_medium().child("Command line"))
            .child(
                Checkbox::new("setup-cli-complete")
                    .debug_selector(|| "setup-cli-complete".into())
                    .label(label)
                    .checked(installed)
                    .disabled(true)
                    .tab_stop(false),
            )
            .child(div().text_sm().text_color(muted).child(detail))
            .child(
                Button::new("setup-cli")
                    .debug_selector(|| "setup-cli-action".into())
                    .self_start()
                    .small()
                    .label(if installed {
                        "CLI options…"
                    } else {
                        "Install jw…"
                    })
                    .on_click(cx.listener(|v, _, w, c| v.install_cli(w, c))),
            )
            .child(div().pt_2().text_lg().font_medium().child("Appearance"));
        let mut appearance = div().h_flex().gap_2();
        for (id, label, mode) in [
            (0usize, "System", Appearance::System),
            (1, "Light", Appearance::Light),
            (2, "Dark", Appearance::Dark),
        ] {
            appearance = appearance.child(
                Button::new(("setup-appearance", id))
                    .small()
                    .label(label)
                    .selected(self.setup.appearance == mode)
                    .disabled(self.setup.busy)
                    .on_click(cx.listener(move |v, _, _, cx| {
                        v.setup.appearance = mode;
                        cx.notify();
                    })),
            );
        }
        local = local
            .child(appearance)
            .child(div().pt_2().text_lg().font_medium().child("Get started"));
        for command in [
            CommandId::OpenRepository,
            CommandId::ConnectAccount,
            CommandId::ShowCommands,
        ] {
            local = local.child(
                Button::new(SharedString::from(format!("setup-{}", command.key())))
                    .self_start()
                    .small()
                    .ghost()
                    .label(command.label())
                    .on_click(cx.listener(move |v, _, w, c| v.execute_command(command, w, c))),
            );
        }
        local = local
            .child(
                Button::new("setup-tools")
                    .self_start()
                    .small()
                    .ghost()
                    .label("Check Git and jj")
                    .on_click(cx.listener(|v, _, w, c| v.check_setup_tools(w, c))),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(muted)
                    .child("jj and hosting accounts are optional."),
            );
        let mut updates=div().id("setup-updates").debug_selector(||"setup-updates".into()).v_flex().min_w_0().gap_3().when(wide,|el|el.flex_1())
            .child(div().text_lg().font_medium().child("Updates"))
            .child(div().text_sm().text_color(muted).child("Opt in to automatic checks, downloads or installation. Nothing update-related connects until you save an opt-in or explicitly check."));
        for (index, label, selected, enabled) in [
            (
                0usize,
                "Check automatically",
                self.setup.updates.automatic_checks,
                true,
            ),
            (
                1,
                "Download automatically",
                self.setup.updates.automatic_downloads,
                self.setup.updates.automatic_checks,
            ),
            (
                2,
                "Install automatically on quit",
                self.setup.updates.automatic_installation,
                self.setup.updates.automatic_downloads,
            ),
        ] {
            updates = updates.child(
                Checkbox::new(("setup-update", index))
                    .debug_selector(move || format!("setup-update-{index}"))
                    .label(label)
                    .checked(selected)
                    .disabled(!enabled || self.setup.busy)
                    .on_click(cx.listener(move |v, checked: &bool, _, cx| {
                        match index {
                            0 => {
                                v.setup.updates.automatic_checks = *checked;
                                if !checked {
                                    v.setup.updates.automatic_downloads = false;
                                    v.setup.updates.automatic_installation = false;
                                }
                            }
                            1 => {
                                v.setup.updates.automatic_downloads = *checked;
                                if !checked {
                                    v.setup.updates.automatic_installation = false;
                                }
                            }
                            _ => v.setup.updates.automatic_installation = *checked,
                        }
                        cx.notify();
                    })),
            );
        }
        updates=updates.child(div().text_sm().text_color(muted).child(format!("Checks run every {} hours. Downloading alone never authorizes installation; choose when to restart.",self.setup.updates.check_interval_hours)))
            .child(div().pt_2().text_sm().text_color(muted).child("Update source"))
            .child(div().text_sm().text_color(muted).child(crate::updater::source()))
            .child(div().text_sm().text_color(muted).child("Downloads use GitHub Releases. No hosting account is required."))
            .child(div().text_sm().text_color(muted).child(crate::updater::status()));
        let columns = div()
            .id("setup-columns")
            .gap_6()
            .when(wide, |el| el.h_flex())
            .when(!wide, |el| el.v_flex())
            .items_start()
            .child(local)
            .child(updates);
        let mut body=div().id("setup-content").debug_selector(||"setup-content".into()).v_flex().w_full().max_w(px(1050.)).gap_4()
            .child(div().text_2xl().font_medium().child("Make Jaywalk yours"))
            .child(div().text_sm().text_color(muted).child("A few optional choices to get started. You can revisit setup anytime from the command palette."))
            .child(columns);
        if let Some(message) = &self.setup.message {
            body = body.child(div().text_sm().child(message.clone()));
        }
        body = body.child(
            div()
                .h_flex()
                .justify_between()
                .gap_2()
                .pt_3()
                .border_t_1()
                .border_color(theme.border)
                .child(
                    Button::new("setup-later")
                        .debug_selector(|| "setup-later".into())
                        .ghost()
                        .label("Skip for now")
                        .disabled(self.setup.busy)
                        .on_click(cx.listener(|v, _, w, c| {
                            let page = if v.arguments.welcome {
                                pages::Page::Welcome
                            } else {
                                pages::Page::Comparison
                            };
                            v.show_page(page, w, c);
                        })),
                )
                .child(
                    Button::new("setup-save")
                        .debug_selector(|| "setup-save".into())
                        .label("Save and continue")
                        .disabled(self.setup.busy)
                        .on_click(cx.listener(|v, _, w, c| v.finish_setup(w, c))),
                ),
        );
        div()
            .id("setup-page")
            .debug_selector(|| "setup-page".into())
            .size_full()
            .overflow_y_scroll()
            .v_flex()
            .items_center()
            .p_4()
            .when(wide, |el| el.p_6())
            .child(body)
            .into_any_element()
    }
    fn check_setup_tools(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.setup.busy {
            return;
        }
        self.setup.busy = true;
        cx.spawn_in(window, async move |this, cx| {
            let message = cx
                .background_spawn(async move {
                    ["git", "jj"]
                        .map(|tool| match jaywalk::setup::tool_version(tool) {
                            Ok(version) => version,
                            Err(error) => error,
                        })
                        .join("\n")
                })
                .await;
            let _ = this.update(cx, |v, cx| {
                v.setup.busy = false;
                v.setup.message = Some(message);
                cx.notify();
            });
        })
        .detach();
    }
    pub(super) fn install_cli(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.spawn_in(window,async move |this,cx| {
            let result=cx.background_spawn(async move {
                let executable=std::env::current_exe().map_err(|e|e.to_string())?;
                let source=cli_install::bundled_launcher(&executable)?;
                let home=std::env::var_os("HOME").map(PathBuf::from).ok_or("Home directory is unavailable")?;
                let mut detail=format!("Bundled command: {}\n\n",source.display());
                let mut found=false;
                for directory in cli_install::candidate_directories(&home,std::env::var_os("PATH").as_deref()) {
                    let path=directory.join("jw");
                    let status=cli_install::status(&path,&source)?;
                    if status!=cli_install::Status::Missing { found=true; detail.push_str(&format!("{}: {status:?}",path.display())); if let Ok(target)=std::fs::read_link(&path) { detail.push_str(&format!(" → {}",target.display())); } detail.push('\n'); }
                }
                if !found{detail.push_str("No jw command found in the app's PATH or common installation folders.\n");}
                detail.push_str(&cli_install::manual_instructions(&source));
                detail.push_str("\n\nInstall into ~/.local/bin, or choose an existing directory. Finder's PATH may differ from your shell's. Verify in Terminal with command -v jw and jw --version.");
                Ok::<_,String>((source,home.join(".local/bin"),detail))
            }).await;
            let (source,default,detail)=match result {Ok(value)=>value,Err(error)=> {let _=this.update_in(cx,|_,w,c|w.prompt(PromptLevel::Warning,"CLI setup",Some(&error),&["OK"],c));return;}};
            let prompt=match this.update_in(cx,|_,w,c|w.prompt(PromptLevel::Info,"Install or check jw",Some(&detail),&["Close","Install in ~/.local/bin","Choose directory…","Copy manual instructions"],c)) {Ok(p)=>p,Err(_)=>return};
            let destination=match prompt.await {
                Ok(3)=> { let instructions=cli_install::manual_instructions(&source); let _=this.update(cx,|_,cx|cx.write_to_clipboard(ClipboardItem::new_string(instructions))); return; },
                Ok(1)=>default,
                Ok(2)=> {
                    let receiver=match this.update_in(cx,|_,_,c|c.prompt_for_paths(PathPromptOptions{files:false,directories:true,multiple:false,prompt:Some("Install jw here".into())})){Ok(receiver)=>receiver,Err(_)=>return};
                    match receiver.await {Ok(Ok(Some(mut paths))) if paths.len()==1=>paths.remove(0),_=>return}
                }
                _=>return,
            };
            let result=cx.background_spawn(async move {cli_install::install(&source,&destination)}).await;
            let (level,message)=match result {Ok(path)=>(PromptLevel::Info,format!("Installed {}\n\nKeep that directory on your shell's PATH. Verify with command -v jw and jw --version. Moving the app requires reinstalling the link.",path.display())),Err(error)=>(PromptLevel::Warning,error)};
            let _=this.update_in(cx,|v,w,c|{v.check_setup_cli(w,c);w.prompt(level,"Command-line integration",Some(&message),&["OK"],c)});
        }).detach();
    }
}
