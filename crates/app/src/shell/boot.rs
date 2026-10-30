use super::drafts::answered;
use super::settings_page::apply_appearance;
use super::{APP_ID, FocusedPanel, Shell, init_keymap};
use crate::chat::ChatPane;
use crate::settings::{AgentDraft, SettingsPage};
use crate::state::{OpenWindow, Shared, WorkspaceWindow};
use crate::terminal::TerminalPanel;
use crate::workbench::Workbench;
use gpui::{App, AppContext, BorrowAppContext, Context, Focusable as _, Window, px};
use gpui_component::dock::{DockArea, DockEvent, DockLayout, DockPlacement};
use gpui_component::input::{InputEvent, InputState};
use gpui_component::{ResizableState, Root, WindowExt as _};
use onehand_core::config::WorkspaceConfig;
use onehand_core::config::{AppConfig, Appearance};
use onehand_core::workspace::Workspace;
use std::collections::HashMap;

impl Shell {
    pub fn new(workspace: Workspace, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let chat = ChatPane::new(window, cx);
        let dock = cx.new(|cx| {
            let skin = super::dock_skin::DockSkin::new(chat.entity_id(), cx);
            DockArea::new("onehand", Some(1), window, cx).with_renderer(skin)
        });
        // One ear for the Issues views, the tab's and the page's: both speak
        // to this window's shell.
        let tell = Self::issues_tell(cx);
        let workbench = Workbench::new(
            {
                let tell = tell.clone();
                move |ask, font_size, cx| crate::features::workbench_modes(ask, tell, font_size, cx)
            },
            cx,
        );
        workbench.update(cx, |panel, cx| {
            panel.set_storage(workspace.storage_dir.as_deref(), cx)
        });
        let terminal = TerminalPanel::new(cx);
        let workers_panel = cx.new(|cx| crate::chat::workers_panel::WorkersPanel::new(&chat, cx));
        let issues_page = Self::new_issues_page(tell, cx);
        let workflows_page = crate::workflows_page::WorkflowsPage::new(cx);
        cx.subscribe_in(
            &workflows_page,
            window,
            |shell: &mut Self, _, event: &crate::workflows_page::WorkflowsPageEvent, window, cx| {
                use crate::workflows_page::WorkflowsPageEvent as E;
                match event {
                    E::Run { workflow, root } => {
                        shell.begin_workflow_on(root, Some(workflow), window, cx)
                    }
                }
            },
        )
        .detach();
        let workspace_page =
            cx.new(|_| crate::workspace_page::WorkspacePage::new(chat.downgrade()));
        cx.subscribe_in(
            &workspace_page,
            window,
            |shell: &mut Self, _, event: &crate::workspace_page::WorkspacePageEvent, window, cx| {
                shell.on_workspace_page(event, window, cx)
            },
        )
        .detach();
        let tasks_page = cx.new(crate::tasks_page::TasksPage::new);
        cx.subscribe_in(
            &tasks_page,
            window,
            |shell: &mut Self, _, event: &crate::tasks_page::TasksPageEvent, window, cx| {
                shell.on_tasks_page(event, window, cx)
            },
        )
        .detach();
        // Closing the window over a workflow being written asks first, as
        // picking another one does: the edit is the page's, and goes with it.
        {
            let page = workflows_page.downgrade();
            let shell = cx.entity().downgrade();
            window.on_window_should_close(cx, move |window, cx| {
                // A save still waiting on its debounce is written now, or the
                // change made just before closing goes with the window.
                if let Some(shell) = shell.upgrade() {
                    shell.update(cx, |shell, cx| shell.flush_workspace(cx));
                }
                let Some(page) = page.upgrade().filter(|page| page.read(cx).dirty(cx)) else {
                    return true;
                };
                crate::shell::confirm_on(
                    page,
                    crate::shell::Confirm {
                        id: "close-workflow-draft",
                        title: "Discard the workflow being written?".into(),
                        description: "Its changes are not saved. Closing the window throws \
                                      them away."
                            .into(),
                        act: "Discard",
                        ..Default::default()
                    },
                    window,
                    cx,
                    |page, window, cx| {
                        page.drop_draft(cx);
                        window.remove_window();
                    },
                );
                false
            });
        }
        issues_page.handle(
            &onehand_workbench::Request::SetStorage(workspace.storage_dir.as_deref()),
            cx,
        );

        // Restored from the workspace, which supplies the built-in arrangement
        // when there is nothing saved: both docks closed, because the
        // conversation is the window's job and a dock nobody asked for is width
        // taken from it.
        let saved = workspace.layout;
        // The saved arrangement holds one workspace-wide answer to "was the
        // terminal open", and the live question is per root -- so it seeds the
        // root that is about to be on screen and no other.
        let seed_root = workspace.active_root().map(|root| root.path.clone());
        dock.update(cx, |area, cx| {
            // Each region is a group of one panel, and the area draws no tab
            // bar over any of them (see `crate::shell::dock_skin`): a single
            // tab that can never have a sibling is not a tab but a duplicate
            // title -- the conversation's name printed again above the header
            // that says it, the word "Workbench" over the strip naming its
            // modes. Each panel's own chrome carries its controls instead.
            area.set_center(DockLayout::tabs().panel(chat.clone()), window, cx);
            area.set_dock(
                DockPlacement::Right,
                DockLayout::tabs().panel(workbench.clone()),
                window,
                cx,
            );
            area.set_dock_size(DockPlacement::Right, px(saved.workbench_w), window, cx);
            // A new dock starts open.
            if !saved.workbench_open {
                area.toggle_dock(DockPlacement::Right, window, cx);
            }
        });
        // The bottom dock is mounted only while the terminal is showing, so
        // there is nothing to set up here when it is not -- see
        // [`Shell::set_terminal_visible`].
        if saved.terminal_open {
            dock.update(cx, |area, cx| {
                super::docks::mount_terminal(area, &terminal, px(saved.terminal_h), window, cx);
            });
        }

        cx.subscribe_in(
            &chat,
            window,
            |shell: &mut Self, _, event: &crate::chat::pane::ChatPaneEvent, window, cx| {
                use crate::chat::pane::ChatPaneEvent as E;
                match event {
                    E::OpenFile(path) => {
                        shell.open_file(path, window, cx);
                    }
                    E::WorkTreeTouched => shell.refresh_worktree(cx),
                    E::AgentStarted => shell.sync_agent_started(cx),
                    E::ShowRail => shell.show_rail(window, cx),
                    // The visibility button and its shortcut preserve the selected mode.
                    E::ToggleWorkbench => shell.toggle_workbench(window, cx),
                    E::ToggleWorkers => shell.toggle_workers(window, cx),
                    E::MaximizeWorkers => {
                        shell.toggle_maximize_panel(FocusedPanel::Workers, window, cx)
                    }
                    E::CloseWorker(uid) => {
                        if let Some((root, session)) = shell.locate(*uid) {
                            shell.close_session(root, session, window, cx);
                        }
                    }
                    // The key's rule, one owner: an open dock with a shell
                    // closes, anything else opens.
                    E::ToggleTerminal => shell.show_terminal(window, cx),
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
                    E::OpenAgentSettings => {
                        shell.show_settings_page(crate::settings::SettingsPage::Agents, cx);
                        shell.open_settings(window, cx);
                    }
                    E::AddFoundAgent(preset) => {
                        shell.add_found_agent(*preset, window, cx);
                        // The pane drew the no-agent state from the list it
                        // just stopped being empty; it reads the list again.
                        shell.chat.update(cx, |_, cx| cx.notify());
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
                        crate::toast::failed("Saving the conversation", why),
                        cx,
                    ),
                    // A warning rather than an error: nothing went wrong and
                    // nothing was lost -- the conversation is exactly where it
                    // was, which is the opposite of the message above.
                    E::PickRefused(why) => window.push_notification(
                        crate::toast::failed("Changing the agent's setting", why),
                        cx,
                    ),
                    E::StopRefused(why) => window.push_notification(
                        crate::toast::failed("Stopping the background item", why),
                        cx,
                    ),
                    E::RenameFailed(why) => window
                        .push_notification(crate::toast::failed("Renaming the session", why), cx),
                    E::ConversationNotDeleted(why) => window.push_notification(
                        crate::toast::failed("Deleting the conversation", why),
                        cx,
                    ),
                    E::Fork { root, spec } => shell.fork_session(root, spec.clone(), window, cx),
                    E::ForkFailed { uid, why } => {
                        if let Some(uid) = uid {
                            shell.end_session(*uid, window, cx);
                        }
                        window.push_notification(
                            crate::toast::failed("Forking the conversation", why),
                            cx,
                        );
                    }
                    E::StopWorkflow(uid) => {
                        let uid = *uid;
                        cx.defer(move |cx| crate::task::stop(uid, cx));
                    }
                    E::ShowTasks(filter) => shell.show_tasks(filter.clone(), window, cx),
                    E::ShowSchedules(only) => shell.show_schedules(only.clone(), window, cx),
                    E::RunCheck => {
                        if let Some(root) = shell.window.workspace.active_root() {
                            let root = root.path.clone();
                            shell.run_check(root, window, cx);
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
                shell.tell_issues_page(&onehand_workbench::Request::LiveConversations(&live), cx);
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
        // And with Settings: whether Resume refuses is judged on the agents
        // and modes they hold, so a refusal answered there clears at once.
        cx.observe_global::<crate::state::Shared>(|shell: &mut Self, cx| {
            shell.tell_issue_works(cx);
        })
        .detach();

        // The conversation header's terminal dot is a fact the docks have no
        // idea anyone outside them wants. Guarded the same way and for the same
        // reason as the rail's rows above: the terminal notifies for its own
        // changes and when its watch sees a command start or end.
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
        // Something holding the keyboard left the frame: a dialog closed, a
        // panel went, a page changed. The keyboard goes where it is wanted
        // rather than staying on a node no frame draws, where it answers no key.
        cx.on_focus_lost(window, |shell: &mut Self, window, cx| {
            shell.focus_lost(window, cx)
        })
        .detach();

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
                        let before =
                            std::mem::replace(&mut shell.window.workspace.name, name.clone());
                        // Put back, field and all, if its write fails.
                        shell.save_change_soon(
                            super::storage::Thing::Name,
                            super::storage::WorkspaceChange {
                                before,
                                after: name,
                                put: |shell, name, window, cx| {
                                    shell.window.workspace.name = name.clone();
                                    shell
                                        .workspace_name
                                        .update(cx, |input, cx| input.set_value(name, window, cx));
                                },
                            },
                            cx,
                        );
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
            agent_form: false,
            settings_page: SettingsPage::default(),
            settings_focus: cx.focus_handle(),
            keymap_editor: cx.new(|cx| crate::keymap::Editor::new(window, cx)),
            settings_view: {
                let shell = cx.weak_entity();
                cx.new(|_| crate::settings::SettingsView::new(shell))
            },
            agent_checks: HashMap::new(),
            held_commands: Default::default(),
            workspace_name,
            renaming: None,
            rename_input: cx.new(|cx| {
                InputState::new(window, cx).placeholder("What this conversation is about")
            }),
            worktree_draft: None,
            branch_draft: None,
            issue_picker: None,
            dialog_return: None,
            label_input: cx.new(|cx| InputState::new(window, cx).placeholder("bug")),
            label_workflow: None,
            label_refused: None,
            check_inputs: HashMap::new(),
            workflow_launcher: None,
            branch_input,
            worktree_branch,
            dock,
            chat,
            workbench,
            workers_panel,
            right: super::workers::RightPanel::Workbench,
            issues_page,
            workflows_page,
            schedules_page: crate::schedules_page::SchedulesPage::new(cx),
            tasks_page,
            workspace_page,
            terminal,
            _pending_save: None,
            saves: Default::default(),
            docks_saved: None,
            _pending_warm: None,
            git_generation: 0,
            rail_sessions: Vec::new(),
            live_conversations: Vec::new(),
            issue_works: (Vec::new(), Vec::new()),
            rail: crate::rail::RailState::new(window, cx),
            folds: HashMap::new(),
            last_panel: FocusedPanel::Chat,
            terminal_homes: super::dock_rules::TerminalHomes::seeded(
                seed_root,
                saved.terminal_open,
            ),
            page_hid_workbench: false,
            mru: HashMap::new(),
            tab_cycle: None,
            app_maximized: None,
            presentation: super::presentation::Presentation::Conversation,
            workbench_drawn: None,
            terminal_drawn: None,
            stepped_aside: false,
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
        cx.update_global::<Shared, _>(|shared, _| shared.recents.touch(dir));
        super::storage::log_failed(super::storage::save_recents(cx), cx);
    }
    open_window(workspace, cx);
}

/// The smallest the window may be dragged to: the rail's minimum beside the
/// conversation's, and a transcript still a few turns tall. Pixels, because it
/// is a bound measured against the screen, not something drawn inside a zoom.
const WINDOW_MIN: gpui::Size<gpui::Pixels> = gpui::size(gpui::px(720.), gpui::px(480.));

/// Open a window for `workspace` and register it.
fn open_window(workspace: Workspace, cx: &mut App) {
    let storage_dir = workspace.storage_dir.clone();
    let name = workspace.name.clone();
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
            window_min_size: Some(WINDOW_MIN),
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
                    storage_dir: storage_dir.clone(),
                    handle: handle.into(),
                    shell,
                });
            });
            // The projects it has switched on for unattended runs are looked at
            // now, so a row that cannot work says so from the first frame and
            // not from the first tick, half an hour in. After the push, because
            // the look finds projects through this registry.
            crate::unattended::recheck(cx);
            if let Some(dir) = &storage_dir {
                super::storage::remember_name(dir, &name, cx);
            }
        });
    })
    .detach();
}

/// Install global state and open the first window.
pub fn boot(cx: &mut App) {
    // First, so a launch waiting on the socket `main` bound is answered soon.
    crate::instance::listen(cx);
    // Before any window, whose tooltip overlay it has to see made.
    crate::controls::put_tooltips_away(cx);
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
    crate::control::boot(&cfg.control);
    cx.set_global(Shared::from_config(cfg, config_path));
    // The settings, workspace and conversation-name writes still queued at
    // quit would die with the process: waited for, as the task files are,
    // once the windows are gone and their last writes are queued.
    let writer = Shared::global(cx).writer.clone();
    cx.on_app_quit(move |_| {
        let writer = writer.clone();
        async move {
            if !writer.flush(crate::task::QUIT_WAIT) {
                eprintln!("onehand: a settings or workspace write may not have landed");
            }
        }
    })
    .detach();
    init_keymap(cx);
    // After the global exists, because that is where the bridge is filed, and
    // before the first window, so a channel that takes a moment to answer has
    // already been asked by the time there is anything to announce.
    crate::remote::boot(&remote, cx);
    crate::unattended::boot(&unattended, cx);
    crate::workflow::boot(cx);
    crate::task::boot(cx);
    crate::schedules::boot(cx);
    // Before the theme names the bundled families, and before the monospace
    // scan looks for them among what is installed.
    crate::fonts::register(cx);
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

    super::render::KeyRelease::register(cx);
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
/// was, correctly, asking for mono. The theme now names the family the app
/// ships instead, so the scan's real job is checking a configured one.
///
/// The scan is done once at boot, before any window exists, because the theme
/// is a global — but the answer is kept, since changing the appearance loads a
/// fresh theme config and [`apply_appearance`] has to put it back.
/// `[font].monospace` from the config file is the first preference — this is
/// the one part of that section the app now reads; `size`, `scale`, `sans` and
/// `fallbacks` still parse and go nowhere.
fn use_installed_mono(configured: Option<&str>, cx: &mut App) {
    let installed = cx.text_system().all_font_names();
    // The theme's default is the bundled family, which is registered before
    // this runs, so it resolves on every machine; the ladder behind it only
    // matters if registering failed.
    let default = gpui_component::ActiveTheme::theme(cx)
        .mono_font_family
        .to_string();
    let preferred = configured
        .into_iter()
        .chain(std::iter::once(default.as_str()));

    if let Some(family) = onehand_core::config::resolve_monospace(preferred, &installed) {
        // Through `update`, so the layer under the library, where rendered
        // Markdown reads its family, has it too.
        let named = family.clone();
        gpui_component::Theme::update(cx, |theme| theme.mono_font_family = named.into());
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
