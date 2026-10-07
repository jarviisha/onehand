use super::drafts::answered;
use super::settings_dialog::apply_appearance;
use super::{APP_ID, FocusedPanel, Shell, init_keymap};
use crate::chat::ChatPane;
use crate::settings::{AgentDraft, SettingsPage};
use crate::state::{OpenWindow, Shared, WorkspaceWindow};
use crate::terminal::TerminalPanel;
use crate::workbench::Workbench;
use gpui::{App, AppContext, BorrowAppContext, Context, Focusable as _, Window, px};
use gpui_component::dock::{DockArea, DockEvent, DockItem, DockPlacement};
use gpui_component::input::{InputEvent, InputState};
use gpui_component::notification::Notification;
use gpui_component::{ResizableState, Root, WindowExt as _};
use onehand_core::config::WorkspaceConfig;
use onehand_core::config::{AppConfig, Appearance};
use onehand_core::workspace::Workspace;
use std::collections::HashMap;

impl Shell {
    pub fn new(workspace: Workspace, window: &mut Window, cx: &mut Context<Self>) -> Self {
        // No panel style is set, because none of the three docks is a tab group
        // -- each is a bare `DockItem::panel` drawing its own chrome, so the
        // library's tab bar is never built and the style that would shape it is
        // never read.
        let dock = cx.new(|cx| DockArea::new("onehand", Some(1), window, cx));
        let chat = ChatPane::new(window, cx);
        let workbench = Workbench::new(cx);
        workbench.update(cx, |panel, cx| {
            panel.set_storage(workspace.storage_dir.as_deref(), cx)
        });
        let terminal = TerminalPanel::new(cx);
        let issues_page = Self::new_issues_page(cx);
        issues_page.handle(
            &onehand_plugin_host::Request::SetStorage(workspace.storage_dir.as_deref()),
            cx,
        );

        // Restored from the workspace, which supplies the built-in arrangement
        // when there is nothing saved: both docks closed, because the
        // conversation is the window's job and a dock nobody asked for is width
        // taken from it.
        let saved = workspace.layout;
        // The saved arrangement holds one workspace-wide answer to "was the
        // terminal open", and the live question is per root -- so it seeds the
        // root that is about to be on screen and no other. Seeding
        // `terminal_root` with it too is what stops the first handover from
        // reading the restored dock as some *other* project's state and closing
        // it on the way in.
        let seed_root = workspace.active_root().map(|root| root.path.clone());
        dock.update(cx, |area, cx| {
            // Bare panels, not tab groups. `DockItem::tab` wraps its panel in a
            // `TabPanel`, whose title bar draws a tab carrying the panel's
            // title -- which for the conversation is the conversation's own
            // name, printed a second time directly above the header that says
            // it, and for the Workbench is the word "Workbench" printed over
            // the strip naming its four modes. One tab that can never have a
            // sibling is not a tab; it is a duplicate title with a chevron's
            // worth of chrome around it.
            //
            // What those title bars were also carrying moves into each panel's
            // own chrome, which is where the rest of its controls already live
            // -- `ChatPane::header` and the Workbench's mode strip.
            let center = DockItem::panel(std::sync::Arc::new(chat.clone()));
            let workbench = DockItem::panel(std::sync::Arc::new(workbench.clone()));

            area.set_center(center, window, cx);
            area.set_right_dock(
                workbench,
                Some(px(saved.workbench_w)),
                saved.workbench_open,
                window,
                cx,
            );
        });
        // The bottom dock is mounted only while the terminal is showing, so
        // there is nothing to set up here when it is not -- see
        // [`Shell::set_terminal_visible`].
        if saved.terminal_open {
            let item = DockItem::panel(std::sync::Arc::new(terminal.clone()));
            dock.update(cx, |area, cx| {
                area.set_bottom_dock(item, Some(px(saved.terminal_h)), true, window, cx);
            });
        }

        cx.subscribe_in(
            &chat,
            window,
            |shell: &mut Self, _, event: &crate::chat::pane::ChatPaneEvent, window, cx| {
                use crate::chat::pane::ChatPaneEvent as E;
                match event {
                    E::OpenFile(path) => {
                        shell
                            .workbench
                            .update(cx, |panel, cx| panel.open_file(path, window, cx));
                        // Opening a file is what makes the Workbench worth
                        // showing -- but only if it is closed, since toggling
                        // an open dock would hide the file just asked for.
                        shell.dock.update(cx, |dock, cx| {
                            if !dock.is_dock_open(DockPlacement::Right, cx) {
                                dock.toggle_dock(DockPlacement::Right, window, cx);
                            }
                        });
                    }
                    E::WorkTreeTouched => shell.refresh_worktree(cx),
                    E::AgentStarted => shell.sync_agent_started(cx),
                    E::ShowRail => shell.show_rail(cx),
                    // The visibility button and its shortcut preserve the selected mode.
                    E::ToggleWorkbench => shell.toggle_workbench(window, cx),
                    // The dock having a shell in it is the same condition
                    // `show_terminal` guards its own close with, and for the
                    // same reason: an open dock holding nothing is what closing
                    // the last tab leaves, the panel there offers *New
                    // terminal*, and this button's tooltip offers to open one
                    // too. Closing on that press would answer neither.
                    E::ToggleTerminal => {
                        let open = shell.dock.read(cx).is_dock_open(DockPlacement::Bottom, cx);
                        if open && shell.terminal.read(cx).has_shell() {
                            shell.set_terminal_visible(false, window, cx);
                        } else {
                            shell.show_terminal(window, cx);
                        }
                    }
                    // Every one of these acts on the selected project, because
                    // the page that offers them is what shows when the selected
                    // project has nothing running in it.
                    E::Project(action) => {
                        use crate::chat::pane::ProjectAction as P;
                        let root_idx = shell.window.workspace.active_root;
                        match action {
                            P::TogglePin => shell.toggle_pin(root_idx, window, cx),
                            P::ToggleUnattended => shell.toggle_unattended(root_idx, window, cx),
                            P::PickIssue => shell.begin_pick(root_idx, None, window, cx),
                            P::Worktree => shell.begin_worktree(root_idx, window, cx),
                            P::RenameBranch => shell.begin_branch_rename(window, cx),
                            P::CopyPath => shell.copy_root_path(root_idx, window, cx),
                            P::RefreshGit => shell.refresh_git(cx),
                            P::Remove => shell.remove_root(root_idx, window, cx),
                        }
                    }
                    E::Restart => shell.restart_session(window, cx),
                    E::CloseSession => shell.close_active_session(window, cx),
                    E::Rename => shell.rename_active_session(window, cx),
                    E::DeleteConversation(dir) => {
                        shell.confirm_delete_conversation(dir.clone(), window, cx)
                    }
                    E::StartSession { agent, resume } => {
                        // The uid is only wanted where the caller has to name
                        // what it just made; a click on the page has the new
                        // session in front of it.
                        let _ = shell.start_session(agent.clone(), resume.clone(), window, cx);
                    }
                    // Said the same way every other failed write is said, and
                    // for a stronger reason: a workspace that will not save can
                    // be described again, a conversation cannot.
                    E::ArchiveFailed(why) => window.push_notification(
                        Notification::error(format!("Conversation not saved — {why}")),
                        cx,
                    ),
                    // A warning rather than an error: nothing went wrong and
                    // nothing was lost -- the conversation is exactly where it
                    // was, which is the opposite of the message above.
                    E::ConversationNotDeleted(why) => window.push_notification(
                        Notification::warning(format!("Conversation not deleted — {why}")),
                        cx,
                    ),
                    E::OpenIssue { root, number } => shell.open_issue(root, *number, window, cx),
                    E::ShowSession { uid, window: at } => {
                        shell.show_session_in(*uid, *at, window, cx)
                    }
                    E::ShowProject(root) => {
                        if let Some(idx) = shell.root_index(root) {
                            shell.select_root(idx, window, cx);
                        }
                    }
                    // Deferred: the run reaches into its session, which may
                    // be what is announcing this.
                    E::ContinueWorkflow(approval) => {
                        let approval = approval.clone();
                        cx.defer(move |cx| crate::task::approve(approval, cx));
                    }
                    E::ReviseWorkflow { approval, note } => {
                        let (approval, note) = (approval.clone(), note.clone());
                        cx.defer(move |cx| crate::task::revise(approval, note, cx));
                    }
                    E::StopWorkflow(uid) => {
                        let uid = *uid;
                        cx.defer(move |cx| crate::task::stop(uid, cx));
                    }
                    E::ResumeTask(id) => shell.resume_task(id.clone(), window, cx),
                    E::DismissTask(id) => crate::task::dismiss(id, cx),
                    E::RetryTask(id) => shell.begin_retry(id.clone(), window, cx),
                    E::RetryTaskCurrent(id) => shell.begin_retry_current(id.clone(), window, cx),
                    E::AnswerReview(id) => shell.answer_review(id.clone(), window, cx),
                    E::RemoveWorktree(id) => shell.remove_worktree(id.clone(), window, cx),
                    E::StopTask(id) => {
                        let id = id.clone();
                        cx.defer(move |cx| crate::task::stop_task(&id, cx));
                    }
                    E::ShowTasks(filter) => shell.show_tasks(filter.clone(), window, cx),
                    E::RunCheck => {
                        if let Some(root) = shell.window.workspace.active_root() {
                            let root = root.path.clone();
                            shell.run_check(root, window, cx);
                        }
                    }
                    // Its project made the active one first, since a session
                    // is minted on that, but not shown: showing it would
                    // connect the session it was last on, which nobody asked
                    // for. The new session's own arrival shows the project.
                    E::ResumeIn {
                        root,
                        agent,
                        archive,
                    } => {
                        if let Some(idx) = shell.root_index(root) {
                            shell.window.workspace.select_root(idx);
                            let _ = shell.start_session(
                                Some(agent.clone()),
                                Some(archive.clone()),
                                window,
                                cx,
                            );
                        }
                    }
                }
                // A finished turn also changes what the rail's session dots
                // say, and the rail is drawn from a query rather than from
                // state the pane pushes here.
                cx.notify();
            },
        )
        .detach();

        // The rail reads session state through `Shell::session_row`, so it has
        // to be redrawn when that changes -- and the pane, which owns the
        // conversations, has no idea the rail exists.
        //
        // Guarded rather than a bare `notify`: the pane notifies on every
        // streamed chunk, and rebuilding the rail per token to redraw a dot
        // that has not moved is work for nothing. The guard compares the whole
        // of what the rail asks the pane for, so it cannot drift out of step
        // with what is drawn.
        cx.observe(&chat, |shell: &mut Self, _, cx| {
            let sessions = shell.rail_sessions(cx);
            if sessions != shell.rail_sessions {
                shell.rail_sessions = sessions;
                cx.notify();
            }
            // Guarded the same way: what an issue says about the session
            // working it moves only when a conversation comes up or goes.
            let live = shell.chat.read(cx).live_conversations(cx);
            if live != shell.live_conversations {
                shell
                    .workbench
                    .update(cx, |panel, cx| panel.live_conversations(&live, cx));
                shell.tell_issues_page(&onehand_plugin_host::Request::LiveConversations(&live), cx);
                shell.live_conversations = live;
            }
        })
        .detach();
        // What the Issues tab says of the runs on each issue moves with the
        // tasks, which change on every step; told only when it changed.
        cx.observe_global::<crate::task::Tasks>(|shell: &mut Self, cx| {
            shell.tell_issue_works(cx);
        })
        .detach();

        // The conversation header's terminal dot is a fact the docks have no
        // idea anyone outside them wants. Guarded the same way and for the same
        // reason as the rail's rows above -- more so for the terminal, which
        // notifies once per chunk of whatever is printing into it.
        cx.observe(&workbench, |shell: &mut Self, _, cx| {
            shell.sync_terminal_live(cx);
        })
        .detach();
        cx.observe(&terminal, |shell: &mut Self, _, cx| {
            shell.sync_terminal_live(cx);
        })
        .detach();
        // The panel has one thing to ask for: its own dock taken off screen.
        // It cannot do that itself -- the `DockArea` is here, and so is the
        // rule that files the open state under the project being left.
        cx.subscribe_in(
            &terminal,
            window,
            |shell: &mut Self, _, event: &crate::terminal::TerminalPanelEvent, window, cx| {
                match event {
                    crate::terminal::TerminalPanelEvent::Hide => {
                        shell.set_terminal_visible(false, window, cx);
                    }
                    // Named rather than focused: the button sits in the
                    // terminal's own strip, and the caret when it is pressed
                    // may be anywhere at all.
                    crate::terminal::TerminalPanelEvent::ToggleMaximize => {
                        shell.toggle_maximize_panel(FocusedPanel::Terminal, window, cx);
                    }
                }
            },
        )
        .detach();

        // The same two, from the Workbench's mode strip. It is mounted bare
        // too, so the library's own dock subscription never sees it -- and
        // neither of these is the panel's to do: the `DockArea` is here.
        cx.subscribe_in(
            &workbench,
            window,
            |shell: &mut Self, _, event: &crate::workbench::WorkbenchEvent, window, cx| {
                shell.on_workbench_event(event, window, cx)
            },
        )
        .detach();

        // Coming back to the window is the other moment everything on screen
        // may have gone stale: the agent kept working while the user was
        // elsewhere, and the badge on the session they are about to read
        // should not still be up when they get there.
        cx.observe_window_activation(window, |shell: &mut Self, window, cx| {
            if !window.is_window_active() {
                shell.held_commands.clear();
                shell.end_cycle(cx);
                return;
            }
            shell.chat.update(cx, |pane, cx| pane.mark_active_seen(cx));
            shell.refresh_worktree(cx);
            cx.notify();
        })
        .detach();

        // *System* means following the desktop for as long as the window is
        // open, not reading it once at boot. That is also what settles the
        // startup race: the platform can only report its default until the
        // desktop has answered the query it makes in the background, and the
        // answer arrives here.
        window
            .observe_window_appearance(|window, cx| {
                let choice = Shared::global(cx).appearance;
                if choice == Appearance::System {
                    apply_appearance(choice, Some(window), cx);
                }
            })
            .detach();

        cx.subscribe(&dock, |shell: &mut Self, _, event: &DockEvent, cx| {
            if matches!(event, DockEvent::LayoutChanged) {
                shell.save_workspace_soon(cx);
                cx.notify();
            }
        })
        .detach();

        let workspace_name =
            cx.new(|cx| InputState::new(window, cx).default_value(workspace.name.clone()));

        // Renaming auto-saves: there is no Save button on the name field, so
        // nothing would ever commit it otherwise -- debounced, because "on
        // change" here means once per keystroke.
        cx.subscribe_in(
            &workspace_name,
            window,
            |shell: &mut Self, state, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    let name = state.read(cx).value().trim().to_string();
                    if !name.is_empty() && name != shell.window.workspace.name {
                        shell.window.workspace.name = name;
                        if shell.settings_open {
                            shell.workspace_note_wanted = true;
                        }
                        shell.save_workspace_soon(cx);
                        cx.notify();
                    }
                }
            },
        )
        .detach();

        let worktree_branch =
            cx.new(|cx| InputState::new(window, cx).placeholder("feat/what-it-is-for"));

        // The folder the worktree would land in is written under this field and
        // derived from it, so it only moves if something outside the input is
        // told the input changed. The complaint above it goes at the same time:
        // it is about a name that is already being replaced.
        cx.subscribe_in(
            &worktree_branch,
            window,
            |shell: &mut Self, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    answered(&mut shell.worktree_draft);
                    cx.notify();
                }
            },
        )
        .detach();

        let branch_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("what the branch is for"));
        // The complaint above the field is about a name the user has started
        // replacing, so it goes on the first keystroke that replaces it.
        cx.subscribe_in(
            &branch_input,
            window,
            |shell: &mut Self, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    answered(&mut shell.branch_draft);
                    cx.notify();
                }
            },
        )
        .detach();

        Self {
            window: WorkspaceWindow::new(workspace),
            rail_hidden: false,
            rail_split: cx.new(|_| ResizableState::default()),
            agent_draft: AgentDraft::new(window, cx),
            settings_page: SettingsPage::default(),
            settings_open: false,
            settings_return_focus: None,
            settings_focus: cx.focus_handle(),
            keymap_editor: cx.new(|cx| crate::keymap::Editor::new(window, cx)),
            settings_view: {
                let shell = cx.weak_entity();
                cx.new(|_| crate::settings::SettingsView::new(shell))
            },
            agent_checks: HashMap::new(),
            settings_note: None,
            workspace_note_wanted: false,
            held_commands: Default::default(),
            workspace_name,
            renaming: None,
            rename_input: cx.new(|cx| {
                InputState::new(window, cx).placeholder("What this conversation is about")
            }),
            worktree_draft: None,
            branch_draft: None,
            issue_picker: None,
            label_input: cx.new(|cx| InputState::new(window, cx).placeholder("bug")),
            label_workflow: None,
            label_refused: None,
            workflow_draft: None,
            check_inputs: HashMap::new(),
            workflow_launcher: None,
            branch_input,
            worktree_branch,
            dock,
            chat,
            workbench,
            issues_page,
            terminal,
            _pending_save: None,
            _pending_warm: None,
            git_generation: 0,
            rail_sessions: Vec::new(),
            live_conversations: Vec::new(),
            issue_works: (Vec::new(), Vec::new()),
            rail_tab: crate::rail::RailTab::Projects,
            folds: HashMap::new(),
            last_panel: FocusedPanel::Chat,
            terminal_open: seed_root
                .clone()
                .map(|path| (path, saved.terminal_open))
                .into_iter()
                .collect(),
            terminal_root: seed_root,
            workbench_aside: false,
            mru: HashMap::new(),
            tab_cycle: None,
            app_maximized: None,
        }
    }
}

/// Seed the first workspace: the positional CLI argument is the project root,
/// else the current directory.
pub fn seed_workspace() -> Workspace {
    let root = std::env::args()
        .nth(1)
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_default();
    Workspace::seeded(root)
}

/// Open `workspace` in a new window -- unless a window already shows that
/// storage directory, in which case focus it. Storage dirs are canonicalized on
/// the way in, so symlink and `..` aliases deduplicate.
pub fn open_or_focus(workspace: Workspace, cx: &mut App) {
    if let Some(dir) = workspace.storage_dir.clone() {
        if let Some(handle) = Shared::global(cx).window_for(&dir) {
            handle
                .update(cx, |_, window, _| window.activate_window())
                .ok();
            return;
        }
        cx.update_global::<Shared, _>(|shared, _| {
            shared.recents.touch(dir);
            if let Err(e) = shared.recents.save() {
                eprintln!("onehand: failed to save recents: {e}");
            }
        });
    }
    open_window(workspace, cx);
}

/// Open a window for `workspace` and register it.
fn open_window(workspace: Workspace, cx: &mut App) {
    let storage_dir = workspace.storage_dir.clone();
    cx.spawn(async move |cx| {
        let options = gpui::WindowOptions {
            // The window's identity to the desktop, and the only reason the app
            // has an icon at all.
            //
            // Nothing about a window carries a picture on Linux. The compositor
            // gets one by matching what the window calls itself against an
            // installed desktop entry -- Wayland compares this string to the
            // entry's file name, X11 compares the `WM_CLASS` it becomes to
            // `StartupWMClass`. Left unset, the window answers with nothing to
            // match, so every entry on disk is unreachable and the result is
            // the generic placeholder no matter how many icons are installed.
            app_id: Some(APP_ID.into()),
            ..Default::default()
        };
        // Filled in by the window builder below and read out after it, because
        // the shell does not exist until then and the registry entry needs it.
        let mut built: Option<gpui::WeakEntity<Shell>> = None;
        let handle = cx
            .open_window(options, |window, cx| {
                let shell = cx.new(|cx| {
                    let mut shell = Shell::new(workspace, window, cx);
                    shell.refresh_git(cx);
                    // Point every panel at the seeded root before the first
                    // frame. Sessions are not persisted, so there is never one
                    // to connect here -- what this settles is which project the
                    // Workbench, the terminal and the empty chat are about,
                    // which was nothing at all until the first rail click.
                    shell.show_active_session(window, cx);
                    // The empty project also needs a live focus path, or no
                    // app shortcut (including Settings) can reach the shell.
                    shell.chat.focus_handle(cx).focus(window, cx);
                    shell
                });
                built = Some(shell.downgrade());
                cx.new(|cx| Root::new(shell, window, cx))
            })
            .expect("failed to open window");

        cx.update(|cx| {
            // A window with no shell is not a thing this can build, so there is
            // nothing to degrade to and nothing worth reporting -- but the
            // registry is what deduplicates windows, so an entry is filed either
            // way rather than the whole window being dropped from it.
            let shell = built.expect("the window was built without a shell");
            cx.update_global::<Shared, _>(|shared, _| {
                shared.windows.push(OpenWindow {
                    storage_dir,
                    handle: handle.into(),
                    shell,
                });
            });
            // The projects it has switched on for unattended runs are looked at
            // now, so a row that cannot work says so from the first frame and
            // not from the first tick, half an hour in. After the push, because
            // the look finds projects through this registry.
            crate::unattended::recheck(cx);
        });
    })
    .detach();
}

/// Install global state and open the first window.
pub fn boot(cx: &mut App) {
    // First, before anything reads or writes the config directory: what
    // follows assumes no other onehand is moving its files at the same time.
    let dir = onehand_core::config::config_dir();
    if onehand_core::instance::hold_lock(&dir).is_err() {
        eprintln!("onehand is already running on {}", dir.display());
        std::process::exit(1);
    }
    // Behind the lock, and before anything reads the templates.
    for problem in onehand_core::workflow::store::migrate_old_dir_blocking()
        .into_iter()
        .chain(onehand_core::task::files::migrate_old_dir_blocking())
    {
        eprintln!("onehand: {problem}");
    }
    let (cfg, config_path) = AppConfig::load_resolved();
    let mono = cfg.font.monospace.clone();
    let appearance = cfg.appearance;
    let remote = cfg.remote.clone();
    let unattended = cfg.unattended.clone();
    cx.set_global(Shared::from_config(cfg, config_path));
    init_keymap(cx);
    // After the global exists, because that is where the bridge is filed, and
    // before the first window, so a channel that takes a moment to answer has
    // already been asked by the time there is anything to announce.
    crate::remote::boot(&remote, cx);
    crate::unattended::boot(&unattended, cx);
    crate::workflow::boot(cx);
    crate::task::boot(cx);
    // Before a mode is chosen, because choosing one applies whichever of the
    // two configs this installs.
    crate::theme::install(cx);
    // Before the font scan, not after: choosing a mode loads a whole theme
    // config, and one naming a font family would land on top of whatever the
    // scan had just resolved.
    apply_appearance(appearance, None, cx);
    use_installed_mono(mono.as_deref(), cx);
    watch_window_close(cx);

    // The most recent storage dir wins over the CLI seed -- reopening where the
    // user left off beats reopening where the shortcut points; a broken or
    // missing recent falls straight through.
    let recent = Shared::global(cx)
        .recents
        .recent_workspaces
        .first()
        .cloned()
        .and_then(|dir| {
            WorkspaceConfig::load_from(&dir)
                .found()
                .map(|cfg| Workspace::from_config(cfg, dir))
        });

    open_or_focus(recent.unwrap_or_else(seed_workspace), cx);
}

/// Point the theme's monospace family at one this machine actually has.
///
/// Everything the transcript sets in mono — diffs, commands, terminal output,
/// `IN`/`OUT` wells, the permission card's command — asks for it by
/// `cx.theme().mono_font_family`, and a family the system does not have is not
/// an error: the text simply comes out in the body face, with nothing on screen
/// or in the log to say the request went nowhere. The component library's
/// default is one hard-coded name per platform, and on Linux that name is
/// DejaVu Sans Mono, which many distributions do not ship — so on those
/// machines every well in the transcript was sans while the code that drew it
/// was, correctly, asking for mono.
///
/// The scan is done once at boot, before any window exists, because the theme
/// is a global — but the answer is kept, since changing the appearance loads a
/// fresh theme config and [`apply_appearance`] has to put it back.
/// `[font].monospace` from the config file is the first preference — this is
/// the one part of that section the app now reads; `size`, `scale`, `sans` and
/// `fallbacks` still parse and go nowhere.
fn use_installed_mono(configured: Option<&str>, cx: &mut App) {
    let installed = cx.text_system().all_font_names();
    // The theme's own default is a preference too, not a thing to override:
    // where it resolves, it is what this platform's users expect to read code
    // in, and it should win over anything merely popular.
    let default = gpui_component::ActiveTheme::theme(cx)
        .mono_font_family
        .to_string();
    let preferred = configured
        .into_iter()
        .chain(std::iter::once(default.as_str()));

    if let Some(family) = onehand_core::config::resolve_monospace(preferred, &installed) {
        gpui_component::Theme::global_mut(cx).mono_font_family = family.clone().into();
        cx.update_global::<Shared, _>(|shared, _| shared.mono_family = Some(family));
    }
}

/// Keep the window registry honest, and end the process with the last window.
///
/// Two jobs, one hook, because they are the same fact arriving:
///
/// - **Prune.** `Shared.windows` exists so that opening a workspace already on
///   screen focuses it instead of duplicating it. A closed window left in that
///   list makes `window_for` hand back a dead handle, and `open_or_focus` then
///   "focuses" nothing and returns -- so *Open workspace…* on a folder whose
///   window was closed silently does nothing at all.
/// - **Quit.** GPUI's Linux backend only stops its event loop when `quit()` is
///   called (`gpui_linux::platform::LinuxPlatform::quit`); there is no
///   quit-on-last-window default. Without this the process outlives its last
///   window, with no UI and no way to reach it.
fn watch_window_close(cx: &mut App) {
    cx.on_window_closed(|cx, closed| {
        cx.update_global::<Shared, _>(|shared, _| {
            shared.windows.retain(|w| w.handle.window_id() != closed);
        });
        // The window is already gone by the time this runs, so the count is
        // the count *after* the close.
        if cx.windows().is_empty() {
            cx.quit();
        }
    })
    .detach();
}
