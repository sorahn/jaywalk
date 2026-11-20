//! Optional setup in the retained central workspace; no work in render.
use super::*;
use jaywalk::{cli_install, settings::Appearance, setup::UpdateSettings};
use std::path::PathBuf;

pub(super) struct State {
    updates: UpdateSettings,
    appearance: Appearance,
    message: Option<String>,
    busy: bool,
}
impl State {
    pub(super) fn new(settings: &Settings) -> Self {
        Self {
            updates: settings.updates,
            appearance: settings.ui.appearance,
            message: None,
            busy: false,
        }
    }
}
impl Viewer {
    pub(super) fn show_setup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.setup.busy {
            self.setup = State::new(&self.settings);
        }
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
                        viewer.show_page(page, window, cx);
                    }
                    Err(error) => viewer.setup.message = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }
    pub(super) fn render_setup(&self, cx: &mut Context<Self>) -> AnyElement {
        use jaywalk::commands::CommandId;
        let theme = gpui::component::Theme::global(cx);
        let mut body = div().id("setup-content").v_flex().gap_4().max_w(px(860.)).w_full()
            .child(div().text_2xl().child("Make Jaywalk yours"))
            .child(div().child("Local comparisons work without an account. Optional setup can be revisited from Set up Jaywalk in the command palette."))
            .child(div().text_lg().child("Command line"))
            .child(div().child("Install a link to jw in ~/.local/bin or a folder you choose. Existing commands are never replaced."))
            .child(Button::new("setup-cli").label("Install or check jw…").on_click(cx.listener(|v,_,w,c|v.install_cli(w,c))))
            .child(div().text_lg().child("Updates"))
            .child(div().text_color(theme.muted_foreground).child(crate::updater::status()))
            .child(div().text_color(theme.muted_foreground).child(crate::updater::source()))
            .child(div().text_color(theme.muted_foreground).child("Optional checks contact the public Jaywalk update feed; downloads come from GitHub Releases. No update connection occurs before you save an opt-in or explicitly check. Automatic checks run at the configured interval (24 hours by default)."));
        for (index, label, selected, enabled) in [
            (
                0usize,
                "Check for updates automatically",
                self.setup.updates.automatic_checks,
                true,
            ),
            (
                1,
                "Download updates automatically",
                self.setup.updates.automatic_downloads,
                self.setup.updates.automatic_checks,
            ),
            (
                2,
                "Install downloaded updates automatically on quit",
                self.setup.updates.automatic_installation,
                self.setup.updates.automatic_downloads,
            ),
        ] {
            body = body.child(
                Button::new(("setup-update", index))
                    .debug_selector(move || format!("setup-update-{index}"))
                    .label(format!("{} {label}", if selected { "✓" } else { "○" }))
                    .selected(selected)
                    .disabled(!enabled || self.setup.busy)
                    .on_click(cx.listener(move |v, _, _, cx| {
                        match index {
                            0 => {
                                v.setup.updates.automatic_checks =
                                    !v.setup.updates.automatic_checks;
                                if !v.setup.updates.automatic_checks {
                                    v.setup.updates.automatic_downloads = false;
                                    v.setup.updates.automatic_installation = false;
                                }
                            }
                            1 => {
                                v.setup.updates.automatic_downloads =
                                    !v.setup.updates.automatic_downloads;
                                if !v.setup.updates.automatic_downloads {
                                    v.setup.updates.automatic_installation = false;
                                }
                            }
                            _ => {
                                v.setup.updates.automatic_installation =
                                    !v.setup.updates.automatic_installation
                            }
                        }
                        cx.notify();
                    })),
            );
        }
        body=body.child(div().text_color(theme.muted_foreground).child("Downloading alone does not authorize installation. Leave automatic installation off to choose when to restart and update."))
            .child(div().text_lg().child("Appearance"));
        let mut appearance = div().h_flex().gap_2();
        for (id, label, mode) in [
            (0usize, "System", Appearance::System),
            (1, "Light", Appearance::Light),
            (2, "Dark", Appearance::Dark),
        ] {
            appearance = appearance.child(
                Button::new(("setup-appearance", id))
                    .label(label)
                    .selected(self.setup.appearance == mode)
                    .disabled(self.setup.busy)
                    .on_click(cx.listener(move |v, _, _, cx| {
                        v.setup.appearance = mode;
                        cx.notify();
                    })),
            );
        }
        body = body
            .child(appearance)
            .child(div().text_lg().child("Get started"));
        for command in [
            CommandId::OpenRepository,
            CommandId::ConnectAccount,
            CommandId::ShowCommands,
        ] {
            body = body.child(
                Button::new(SharedString::from(format!("setup-{}", command.key())))
                    .label(command.label())
                    .on_click(cx.listener(move |v, _, w, c| v.execute_command(command, w, c))),
            );
        }
        body = body
            .child(
                Button::new("setup-tools")
                    .label("Check Git and jj availability")
                    .on_click(cx.listener(|v, _, w, c| v.check_setup_tools(w, c))),
            )
            .child(
                div()
                    .text_color(theme.muted_foreground)
                    .child("jj is optional. No tools or accounts are installed automatically."),
            );
        if let Some(message) = &self.setup.message {
            body = body.child(div().child(message.clone()));
        }
        body = body.child(
            div()
                .h_flex()
                .gap_2()
                .child(
                    Button::new("setup-save")
                        .debug_selector(|| "setup-save".into())
                        .label("Save and continue")
                        .disabled(self.setup.busy)
                        .on_click(cx.listener(|v, _, w, c| v.finish_setup(w, c))),
                )
                .child(
                    Button::new("setup-later")
                        .debug_selector(|| "setup-later".into())
                        .ghost()
                        .label("Set up later")
                        .disabled(self.setup.busy)
                        .on_click(cx.listener(|v, _, w, c| {
                            let page = if v.arguments.welcome {
                                pages::Page::Welcome
                            } else {
                                pages::Page::Comparison
                            };
                            v.show_page(page, w, c);
                        })),
                ),
        );
        div()
            .id("setup-page")
            .debug_selector(|| "setup-page".into())
            .size_full()
            .overflow_y_scroll()
            .v_flex()
            .items_center()
            .p_6()
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
            let _=this.update_in(cx,|_,w,c|w.prompt(level,"Command-line integration",Some(&message),&["OK"],c));
        }).detach();
    }
}
