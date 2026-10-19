//! Every command the keymap knows: its id, the label Settings shows, its
//! default keys, what it does, and where it acts.

use super::{CLOSE_SESSION, Command, NEW_SESSION, RESTART};
use crate::shell::*;

pub const COMMANDS: &[Command] = &[
    command!("settings", "Open Settings", ["ctrl-,"], OpenSettings),
    command!(
        "toggle_rail",
        "Show / hide navigation rail",
        ["ctrl-shift-b"],
        ToggleRail
    ),
    command!(
        "toggle_workbench",
        "Show / hide Workbench (previous mode)",
        ["ctrl-shift-j"],
        ToggleWorkbenchVisibility
    ),
    command!(
        "editor",
        "Open / focus Editor",
        ["ctrl-shift-e"],
        ToggleWorkbench
    ),
    command!(
        "markdown",
        "Open / focus Markdown",
        ["ctrl-shift-m"],
        ToggleMarkdown
    ),
    command!(
        "neovim",
        "Open / focus Neovim",
        ["ctrl-shift-n"],
        OpenNeovim
    ),
    // Unshifted on purpose: gpui names a key by the keysym the layout produces
    // with the modifiers applied, so shift over the backtick arrives as
    // `ctrl-~` and a `ctrl-shift-`` binding never matches anything.
    command!(
        "terminal",
        "Show / hide terminal",
        ["ctrl-`"],
        ToggleTerminal
    ),
    command!(
        "composer",
        "Focus composer",
        ["ctrl-shift-a"],
        FocusComposer
    ),
    command!(
        NEW_SESSION,
        "New session (default agent, current project)",
        ["ctrl-shift-o"],
        NewSession
    ),
    command!(
        RESTART,
        "Restart agent (confirm again mid-turn)",
        ["ctrl-shift-r"],
        RestartSession
    ),
    command!(
        CLOSE_SESSION,
        "Close session (confirm again mid-turn)",
        ["ctrl-shift-w"],
        CloseSession
    ),
    command!(
        "run_workflow",
        "Run a workflow on the current project",
        [],
        RunWorkflow
    ),
    command!("show_tasks", "Show the Tasks page", [], ShowTasks),
    command!(
        "maximize",
        "Maximize focused panel / restore",
        ["ctrl-shift-k"],
        ToggleMaximize
    ),
    command!(
        "save",
        "Save open file",
        ["ctrl-s"],
        SaveFile,
        "Shell && !Terminal && !Dialog && !Settings",
        "Outside terminal"
    ),
    command!(
        "zoom_in",
        "Zoom focused panel in",
        ["ctrl-=", "ctrl-+"],
        ZoomIn
    ),
    command!("zoom_out", "Zoom focused panel out", ["ctrl--"], ZoomOut),
    command!(
        "zoom_reset",
        "Reset focused panel zoom",
        ["ctrl-0"],
        ZoomReset
    ),
    command!(
        "next_session",
        "Next session (most recent first)",
        ["ctrl-tab"],
        NextSession
    ),
    command!(
        "previous_session",
        "Previous session",
        ["ctrl-shift-tab"],
        PrevSession
    ),
    command!(
        "session_1",
        "Session 1",
        ["ctrl-1"],
        SelectSession { index: 0 }
    ),
    command!(
        "session_2",
        "Session 2",
        ["ctrl-2"],
        SelectSession { index: 1 }
    ),
    command!(
        "session_3",
        "Session 3",
        ["ctrl-3"],
        SelectSession { index: 2 }
    ),
    command!(
        "session_4",
        "Session 4",
        ["ctrl-4"],
        SelectSession { index: 3 }
    ),
    command!(
        "session_5",
        "Session 5",
        ["ctrl-5"],
        SelectSession { index: 4 }
    ),
    command!(
        "session_6",
        "Session 6",
        ["ctrl-6"],
        SelectSession { index: 5 }
    ),
    command!(
        "session_7",
        "Session 7",
        ["ctrl-7"],
        SelectSession { index: 6 }
    ),
    command!(
        "session_8",
        "Session 8",
        ["ctrl-8"],
        SelectSession { index: 7 }
    ),
    command!(
        "session_9",
        "Session 9",
        ["ctrl-9"],
        SelectSession { index: 8 }
    ),
    command!(
        "focus_rail_search",
        "Search sessions and projects in the rail",
        ["ctrl-k"],
        crate::rail::FocusRailSearch,
        "Shell && !Terminal && !Dialog",
        "Outside terminal"
    ),
    command!(
        "rail_next_session",
        "Next session in the rail",
        ["alt-down"],
        crate::rail::RailNext,
        "Shell && !Terminal && !Dialog",
        "Outside terminal"
    ),
    command!(
        "rail_previous_session",
        "Previous session in the rail",
        ["alt-up"],
        crate::rail::RailPrev,
        "Shell && !Terminal && !Dialog",
        "Outside terminal"
    ),
    command!(
        "rail_open",
        "Open the session, or fold the project",
        ["enter"],
        crate::rail::RailOpen,
        "Rail && !Input",
        "Rail list"
    ),
    command!(
        "rail_up",
        "Move to the row above",
        ["up"],
        crate::rail::RailUp,
        "Rail && !Input",
        "Rail list"
    ),
    command!(
        "rail_down",
        "Move to the row below",
        ["down"],
        crate::rail::RailDown,
        "Rail && !Input",
        "Rail list"
    ),
    command!(
        "rail_fold",
        "Fold the project",
        ["left"],
        crate::rail::RailFold,
        "Rail && !Input",
        "Rail list"
    ),
    command!(
        "rail_unfold",
        "Unfold the project",
        ["right"],
        crate::rail::RailUnfold,
        "Rail && !Input",
        "Rail list"
    ),
    command!(
        "issues_up",
        "Pick the issue above",
        ["up"],
        onehand_workbench_issues::IssuesUp,
        "Issues && !Input",
        "Issues"
    ),
    command!(
        "issues_down",
        "Pick the issue below",
        ["down"],
        onehand_workbench_issues::IssuesDown,
        "Issues && !Input",
        "Issues"
    ),
    command!(
        "issues_close",
        "Close the form, the review or the issue, or go back to the list",
        ["escape"],
        onehand_workbench_issues::IssuesClose,
        "Issues && !Dialog && !Input",
        "Issues"
    ),
    command!(
        "plugin_open",
        "Open the plugin's details",
        ["enter"],
        onehand_workbench_plugins::PluginOpen,
        "PluginList && !Input",
        "Plugins list"
    ),
    command!(
        "plugin_flip",
        "Turn the plugin on or off here",
        ["space"],
        onehand_workbench_plugins::PluginFlip,
        "PluginList && !Input",
        "Plugins list"
    ),
    command!(
        "question_up",
        "Move to the choice above",
        ["up"],
        crate::chat::transcript::QuestionUp,
        "QuestionCard && !Input",
        "Question card"
    ),
    command!(
        "question_down",
        "Move to the choice below",
        ["down"],
        crate::chat::transcript::QuestionDown,
        "QuestionCard && !Input",
        "Question card"
    ),
    command!(
        "question_take",
        "Take the choice",
        ["enter"],
        crate::chat::transcript::QuestionTake,
        "QuestionCard && !Input",
        "Question card"
    ),
    command!(
        "question_skip",
        "Skip the question",
        ["escape"],
        crate::chat::transcript::QuestionSkip,
        "QuestionCard && !Dialog && !Input",
        "Question card"
    ),
    command!(
        "question_1",
        "Choice 1",
        ["1"],
        crate::chat::transcript::QuestionPick { index: 0 },
        "QuestionCard && !Input",
        "Question card"
    ),
    command!(
        "question_2",
        "Choice 2",
        ["2"],
        crate::chat::transcript::QuestionPick { index: 1 },
        "QuestionCard && !Input",
        "Question card"
    ),
    command!(
        "question_3",
        "Choice 3",
        ["3"],
        crate::chat::transcript::QuestionPick { index: 2 },
        "QuestionCard && !Input",
        "Question card"
    ),
    command!(
        "question_4",
        "Choice 4",
        ["4"],
        crate::chat::transcript::QuestionPick { index: 3 },
        "QuestionCard && !Input",
        "Question card"
    ),
    command!(
        "question_5",
        "Choice 5",
        ["5"],
        crate::chat::transcript::QuestionPick { index: 4 },
        "QuestionCard && !Input",
        "Question card"
    ),
    command!(
        "question_6",
        "Choice 6",
        ["6"],
        crate::chat::transcript::QuestionPick { index: 5 },
        "QuestionCard && !Input",
        "Question card"
    ),
    command!(
        "question_7",
        "Choice 7",
        ["7"],
        crate::chat::transcript::QuestionPick { index: 6 },
        "QuestionCard && !Input",
        "Question card"
    ),
    command!(
        "question_8",
        "Choice 8",
        ["8"],
        crate::chat::transcript::QuestionPick { index: 7 },
        "QuestionCard && !Input",
        "Question card"
    ),
    command!(
        "question_9",
        "Choice 9",
        ["9"],
        crate::chat::transcript::QuestionPick { index: 8 },
        "QuestionCard && !Input",
        "Question card"
    ),
    command!(
        "permission_allow",
        "Allow once",
        ["enter"],
        crate::chat::transcript::PermissionAllow,
        "PermissionCard && !Input",
        "Permission card"
    ),
    command!(
        "permission_deny",
        "Deny",
        ["escape"],
        crate::chat::transcript::PermissionDeny,
        "PermissionCard && !Dialog && !Input",
        "Permission card"
    ),
    command!(
        "tasks_close",
        "Close the task, back to the cards",
        ["escape"],
        crate::chat::pane::CloseTaskDetail,
        "TasksPage && !Dialog && !Input",
        "Tasks page"
    ),
    command!(
        "workflows_close",
        "Close the workflow form",
        ["escape"],
        crate::workflows_page::CloseWorkflowForm,
        "WorkflowsPage && !Dialog && !Input",
        "Workflows page"
    ),
    // `A > B` scores at `B`'s depth, so these tie with the input's own arrow
    // and Tab bindings and win by being registered later. The composer adds
    // `ChatComposer` only while a list is open, so otherwise the keys still
    // move the caret.
    command!(
        "completion_previous",
        "Previous suggestion",
        ["up"],
        CompletionPrev,
        "ChatComposer > Input",
        "Composer suggestions"
    ),
    command!(
        "completion_next",
        "Next suggestion",
        ["down"],
        CompletionNext,
        "ChatComposer > Input",
        "Composer suggestions"
    ),
    command!(
        "completion_accept",
        "Accept suggestion",
        ["tab"],
        CompletionAccept,
        "ChatComposer > Input",
        "Composer suggestions (Enter also accepts)"
    ),
    command!(
        "paste",
        "Paste text / attach image or file",
        ["ctrl-v"],
        PasteHere,
        "ChatComposerCard > Input",
        "Composer"
    ),
    command!(
        "cycle_mode",
        "Next session mode",
        ["shift-tab"],
        CycleMode,
        "ChatComposerCard > Input",
        "Composer"
    ),
];
