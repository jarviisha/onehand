use super::drafts::answered;
use super::settings_page::apply_appearance;
use super::{CloseGuard, FocusedPanel, Shell, init_keymap};
use crate::chat::ChatPane;
use crate::settings::{AgentDraft, HostDraft, SettingsPage, TelegramDraft};
use crate::state::{Shared, WorkspaceWindow};
use crate::terminal::TerminalPanel;
use crate::workbench::Workbench;
use gpui::{App, AppContext, BorrowAppContext, Context, Window, px};
use gpui_component::dock::{DockArea, DockEvent};
use gpui_component::input::{InputEvent, InputState};
use gpui_component::{ResizableState, WindowExt as _};
use onehand_core::config::{AppConfig, Appearance};
use onehand_core::workspace::Workspace;

mod open;
use open::open;
pub use open::{open_or_focus, seed_workspace};
use std::collections::HashMap;
use std::sync::Arc;

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
        let terminal = TerminalPanel::new(cx);
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
        let board_page = cx.new(crate::board_page::BoardPage::new);
        cx.subscribe_in(
            &board_page,
            window,
            |shell: &mut Self, _, event: &crate::board_page::BoardPageEvent, window, cx| {
                shell.on_board_page(event, window, cx)
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
        let close_guard: CloseGuard = {
            let page = workflows_page.downgrade();
            let shell = cx.entity().downgrade();
            std::rc::Rc::new(move |window: &mut Window, cx: &mut gpui::App| {
                // A save still waiting on its debounce is written now, or the
                // change made just before closing goes with the window.
                if let Some(shell) = shell.upgrade() {
                    shell.update(cx, |shell, cx| shell.flush_workspace(cx));
                }
                crate::projects::flush(cx);
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
            })
        };
        {
            let guard = close_guard.clone();
            window.on_window_should_close(cx, move |window, cx| guard(window, cx));
        }

        // Restored from the workspace, which supplies the built-in arrangement
        // when there is nothing saved: both docks closed, because the
        // conversation is the window's job and a dock nobody asked for is width
        // taken from it.
        let saved = workspace.layout;
        // The saved arrangement holds one workspace-wide answer to "was the
        // terminal open", and the live question is per root -- so it seeds the
        // root that is about to be on screen and no other.
        let seed_root = workspace.active_root().map(|root| root.path.clone());
        workbench.update(cx, |panel, cx| panel.set_shown(saved.workbench_open, cx));
        dock.update(cx, |area, cx| {
            let (chat, workbench) = (Arc::new(chat.clone()), Arc::new(workbench.clone()));
            super::workbench_slot::mount(area, chat, workbench, &saved, window, cx);
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
                    E::AgentStarted => {
                        // A new attach: what the session it replaced waited
                        // on its host for is over.
                        shell.prune_host_waits(cx);
                        shell.sync_agent_started(cx);
                    }
                    E::PageChanged => shell.prune_host_waits(cx),
                    E::StartFromPage(send) => shell.start_from_page(send.clone(), window, cx),
                    E::ShowSession(uid) => shell.show_session(*uid, window, cx),
                    // Every one of these acts on the selected project, because
                    // the page that offers them is only drawn for it.
                    E::Project(action) => {
                        use crate::chat::pane::ProjectAction as P;
                        let root_idx = shell.window.workspace.active_root;
                        match action {
                            P::TogglePin => shell.toggle_pin(root_idx, window, cx),
                            P::Rename => shell.begin_project_rename(root_idx, window, cx),
                            P::Worktree => shell.begin_worktree(root_idx, window, cx),
                            P::RenameBranch => shell.begin_branch_rename(window, cx),
                            P::CopyPath => shell.copy_root_path(root_idx, window, cx),
                            P::Settings => shell.project_settings_of(root_idx, window, cx),
                            P::RefreshGit => shell.refresh_git(cx),
                            P::Offload => shell.begin_offload(root_idx, window, cx),
                            P::RunHere => shell.run_here_again(root_idx, window, cx),
                            P::Remove => shell.remove_root(root_idx, window, cx),
                        }
                    }
                    E::Worktree(act) => shell.worktree_act(act.clone(), window, cx),
                    E::Restart => shell.restart_session(window, cx),
                    E::CloseSession => shell.close_active_session(window, cx),
                    E::Rename => shell.rename_active_session(window, cx),
                    E::EndHostSession {
                        host,
                        session,
                        label,
                    } => shell.ask_end_on_host(
                        host.clone(),
                        session.clone(),
                        label.clone(),
                        window,
                        cx,
                    ),
                    E::EndOnHost => {
                        if let Some(uid) = shell.active_session_uid() {
                            shell.ask_end_session(uid, window, cx);
                        }
                    }
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
                    E::OpenConversation(session) => shell.open_conversation(session, window, cx),
                    E::StartSession { agent, resume } => {
                        // The uid is only wanted where the caller has to name
                        // what it just made; a click on the page has the new
                        // session in front of it.
                        let _ = shell.start_session(agent.clone(), resume.clone(), window, cx);
                    }
                    E::WatchHostSession { agent, session } => {
                        shell.watch_host_session(agent.clone(), session.clone(), window, cx)
                    }
                    // Said the same way every other failed write is said, and
                    // for a stronger reason: a workspace that will not save can
                    // be described again, a conversation cannot.
                    E::ArchiveFailed(why) => window.push_notification(
                        crate::toast::failed("Saving the conversation", why),
                        cx,
                    ),
                    E::CodeFailed(why) => window.push_notification(
                        crate::toast::failed("Sending the code to the host", why),
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
                    E::SwitchRefused(why) => {
                        window.push_notification(crate::toast::warning(why), cx)
                    }
                    E::Fork { root, spec } => shell.fork_session(root, spec.clone(), window, cx),
                    E::ForkFailed { uid, why } => {
                        let lead = uid.and_then(crate::control::lead_of);
                        let fx = shell.window.workspace.fork_failed(*uid, lead, why.clone());
                        shell.carry_workspace(fx, window, cx);
                    }
                    E::LeftConversation { uid, label } => {
                        let label = label
                            .clone()
                            .unwrap_or_else(|| shell.session_label(*uid, cx));
                        let fx = shell.window.workspace.left_on_host(*uid, label);
                        shell.carry_workspace(fx, window, cx);
                    }
                    E::Watching { uid, id } => shell.window.workspace.watching(*uid, id.clone()),
                    E::StopWorkflow(uid) => {
                        let uid = *uid;
                        cx.defer(move |cx| crate::task::stop(uid, cx));
                    }
                    E::HostBusy { uid, host } => shell.host_busy(*uid, host.clone(), window, cx),
                    E::HostLost { uid, host } => shell.host_lost(*uid, host.clone(), window, cx),
                    E::RestartHost(host) => shell.ask_restart_host(host.clone(), window, cx),
                    E::TakeHostCode(path) => shell.ask_take_host_code(path.clone(), window, cx),
                    E::UpdateHost(path) => shell.update_host_from_laptop(path.clone(), cx),
                    E::RunCheck => shell.run_check(window, cx),
                    E::ShowBoard(project) => shell.show_board(project.clone(), window, cx),
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
        // Who holds each offloaded project's code: the lock on the docks,
        // the rail's mark and the project page's box follow it.
        cx.default_global::<crate::offload::Offloads>();
        cx.observe_global::<crate::offload::Offloads>(|shell: &mut Self, cx| {
            shell.sync_offload(cx);
        })
        .detach();
        // The workspace's own offloaded projects, read once the shell is up.
        cx.defer_in(window, |shell: &mut Self, _, cx| shell.sync_offload(cx));

        // The title bar's terminal dot is a fact the docks have no
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
                let choice = Shared::global(cx).settings.appearance;
                if choice == Appearance::System {
                    apply_appearance(choice, Some(window), cx);
                }
            })
            .detach();

        cx.subscribe(&dock, |_: &mut Self, _, event: &DockEvent, cx| {
            // Redrawn, not saved: the frame reads the sizes back, and saves
            // only what changed the layout the person wants.
            if matches!(event, DockEvent::LayoutChanged) {
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
            |shell: &mut Self, state, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::Change) {
                    // Put back, field and all, if its write fails.
                    let name = state.read(cx).value().to_string();
                    let fx = shell.window.workspace.rename(&name);
                    shell.carry_workspace(fx, window, cx);
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

        let mut shell = Self {
            window: WorkspaceWindow::new(workspace),
            rail_hidden: false,
            rail_split: cx.new(|_| ResizableState::default()),
            agent_draft: AgentDraft::new(window, cx),
            agent_form: false,
            host_draft: HostDraft::new(window, cx),
            telegram_draft: TelegramDraft::new(window, cx),
            settings_page: SettingsPage::default(),
            settings_project: None,
            page_starts: Default::default(),
            page_waits: Default::default(),
            settings_focus: cx.focus_handle(),
            keymap_editor: cx.new(|cx| crate::keymap::Editor::new(window, cx)),
            settings_view: {
                let shell = cx.weak_entity();
                cx.new(|_| crate::settings::SettingsView::new(shell))
            },
            held_commands: Default::default(),
            close_guard,
            workspace_name,
            renaming: None,
            project_rename: None,
            told_issue_files: HashMap::new(),
            rename_input: cx.new(|cx| {
                InputState::new(window, cx).placeholder("What this conversation is about")
            }),
            worktree_draft: None,
            branch_draft: None,
            host_project: None,
            host_waits: Default::default(),
            host_listening: Default::default(),
            offload_draft: None,
            build_host_draft: None,
            route_draft: None,
            route_suggestions: HashMap::new(),
            offload_locks: HashMap::new(),
            dialog_return: None,
            check_inputs: HashMap::new(),
            board_refused: None,
            workflow_launcher: None,
            branch_input,
            worktree_branch,
            dock,
            chat,
            workbench,
            issues_page,
            workflows_page,
            schedules_page: crate::schedules_page::SchedulesPage::new(cx),
            tasks_page,
            board_page,
            workspace_page,
            terminal,
            _pending_save: None,
            layout_saved: None,
            terminal_live: false,
            _pending_warm: None,
            _host_files: None,
            rail_sessions: Vec::new(),
            live_conversations: Vec::new(),
            #[cfg(test)]
            refused: Vec::new(),
            #[cfg(test)]
            saves_asked: 0,
            rail: crate::rail::RailState::new(cx),
            palette: crate::palette::Palette::new(window, cx),
            folds: HashMap::new(),
            last_panel: FocusedPanel::Chat,
            terminal_homes: super::dock_rules::TerminalHomes::seeded(
                seed_root,
                saved.terminal_open,
            ),
            page_hid_workbench: false,
            settings_from: None,
            mru: HashMap::new(),
            tab_cycle: None,
            app_maximized: None,
            presentation: super::presentation::Presentation::Conversation,
            workbench_drawn: None,
            terminal_drawn: None,
            stepped_aside: false,
        };
        // Each folder's issues file, before anything asks for one.
        shell.tell_issue_files_whole(shell.issue_files(cx), cx);
        shell
    }
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
        // After the tasks are where they are kept, since it repoints them.
        .chain(onehand_core::workspace::migrate_storage_blocking(seed()))
    {
        eprintln!("onehand: {problem}");
    }
    let (cfg, config_path) = AppConfig::load_resolved();
    let mono = cfg.font.monospace.clone();
    let appearance = cfg.appearance;
    let remote = cfg.remote.clone();
    crate::control::boot(&cfg.control);
    cx.set_global(Shared::from_config(cfg, config_path));
    // After the migration, which brings every workspace's projects in.
    let store = onehand_core::project::file::store_dir();
    let (projects, unread) = onehand_core::project::file::load_all_blocking(&store);
    for problem in unread {
        eprintln!("onehand: {problem}");
    }
    cx.update_global::<Shared, _>(|shared, _| {
        shared.projects = onehand_core::project::ProjectsDrive::new(projects, store);
    });
    // The settings, workspace and conversation-name writes still queued at
    // quit would die with the process: waited for, as the task files are,
    // once the windows are gone and their last writes are queued.
    let writer = Shared::global(cx).writer.clone();
    cx.on_app_quit(move |cx| {
        // A project change still waiting on its burst, queued now so the wait
        // below covers it.
        crate::projects::flush(cx);
        let writer = writer.clone();
        async move {
            if !writer.flush(crate::task::QUIT_WAIT) {
                eprintln!("onehand: a settings or workspace write may not have landed");
            }
        }
    })
    .detach();
    init_keymap(cx);
    // After the keymap, whose commands the menus offer.
    super::frame::install_menus(cx);
    // After the global exists, because that is where the bridge is filed, and
    // before the first window, so a channel that takes a moment to answer has
    // already been asked by the time there is anything to announce.
    crate::remote::boot(&remote, cx);
    crate::workflow::boot(cx);
    crate::task::boot(cx);
    // Who each forge acts as, which a start with forge steps is judged on.
    crate::accounts::check(cx);
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

    // The most recent workspace that loads wins over the CLI seed --
    // reopening where the user left off beats reopening where the shortcut
    // points; with none, the seed is made and written.
    let launch = onehand_core::workspace::launch_blocking(
        Shared::global(cx).recents.listed(),
        seed_workspace(),
        &Shared::global(cx).projects,
    );

    super::render::KeyRelease::register(cx);
    open(launch, cx);
}

/// What the ids of projects made at this launch are minted from: the clock
/// and the process, so two launches never start from one seed.
fn seed() -> u64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos() as u64);
    nanos ^ (u64::from(std::process::id()) << 32)
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
/// - **Prune.** The window list exists so that opening a workspace already on
///   screen focuses it instead of duplicating it. A closed window left in that
///   list makes `window_for` hand back a dead handle, and `open_or_focus` then
///   "focuses" nothing and returns -- so opening a workspace whose window was
///   closed silently does nothing at all.
/// - **Quit.** GPUI's Linux backend only stops its event loop when `quit()` is
///   called (`gpui_linux::platform::LinuxPlatform::quit`); there is no
///   quit-on-last-window default. Without this the process outlives its last
///   window, with no UI and no way to reach it.
fn watch_window_close(cx: &mut App) {
    cx.on_window_closed(|cx, closed| {
        crate::windows::forget(closed, cx);
        // The window is already gone by the time this runs, so the count is
        // the count *after* the close.
        if cx.windows().is_empty() {
            cx.quit();
        }
    })
    .detach();
}
