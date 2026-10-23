//! A schedule being made or edited: its fields as the form holds them, and
//! the schedule they say.

use super::SchedulesPage;
use gpui::{App, AppContext as _, Context, Entity, Window};
use gpui_component::input::{InputEvent, InputState, TextareaState};
use onehand_core::schedule::{self, Missed, Overlap, Quick, Schedule, Starts};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// How the form says when: one of the quick choices, or an expression.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WhenKind {
    EveryHours,
    Daily,
    Weekdays,
    Weekly,
    Expression,
}

impl WhenKind {
    pub(super) const ALL: [Self; 5] = [
        Self::EveryHours,
        Self::Daily,
        Self::Weekdays,
        Self::Weekly,
        Self::Expression,
    ];

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::EveryHours => "Every N hours",
            Self::Daily => "Every day",
            Self::Weekdays => "On weekdays",
            Self::Weekly => "Every week",
            Self::Expression => "Expression…",
        }
    }
}

/// The days of the week, from Sunday, as the weekly choice names them.
pub(super) const DAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

pub(crate) struct ScheduleDraft {
    /// The schedule as it was opened; `None` for a new one.
    pub(super) original: Option<Schedule>,
    pub(super) id: String,
    pub(super) name: Entity<InputState>,
    pub(super) project: Option<PathBuf>,
    /// A session with a prompt, or else a workflow on a brief. What was
    /// typed for the other is kept until the schedule is saved.
    pub(super) prompt_kind: bool,
    pub(super) prompt: Entity<TextareaState>,
    pub(super) workflow: Option<String>,
    pub(super) title: Entity<InputState>,
    pub(super) body: Entity<TextareaState>,
    pub(super) agent: Option<String>,
    pub(super) mode: Option<String>,
    pub(super) options: BTreeMap<String, String>,
    pub(super) tools: Vec<String>,
    pub(super) labels: Entity<InputState>,
    pub(super) when: WhenKind,
    pub(super) hours: Entity<InputState>,
    pub(super) minute: Entity<InputState>,
    pub(super) time: Entity<InputState>,
    pub(super) weekday: u8,
    pub(super) expr: Entity<InputState>,
    pub(super) missed: Missed,
    pub(super) overlap: Overlap,
    pub(super) on: bool,
    /// What the form said when it was opened, to tell a change from none.
    pub(super) opened: Option<Schedule>,
}

/// A new schedule on `project`: every day at nine, a prompt, switched on.
pub(super) fn blank(project: Option<PathBuf>) -> Schedule {
    Schedule {
        id: onehand_core::task::new_id(),
        name: String::new(),
        project: project.unwrap_or_default(),
        on: true,
        when: Quick::Daily { hour: 9, minute: 0 }.expr(),
        agent: None,
        mode: None,
        tools: Vec::new(),
        labels: Vec::new(),
        missed: Missed::RunOnce,
        overlap: Overlap::Skip,
        on_since: 0,
        last_fired: None,
        said: None,
        options: BTreeMap::new(),
        starts: Starts::Prompt {
            prompt: String::new(),
        },
    }
}

impl ScheduleDraft {
    /// The form on `s`; `new` when it is not saved yet.
    pub(super) fn open(
        s: &Schedule,
        new: bool,
        window: &mut Window,
        cx: &mut Context<SchedulesPage>,
    ) -> Self {
        let mut input = |text: &str, hint: &'static str| {
            let state = cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(hint)
                    .default_value(text.to_string())
            });
            cx.subscribe(&state, |_, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            })
            .detach();
            state
        };
        let name = input(&s.name, "Nightly code review");
        let title = input(
            match &s.starts {
                Starts::Workflow { title, .. } => title,
                Starts::Prompt { .. } => "",
            },
            "What to do, in one line",
        );
        let labels = input(&s.labels.join(", "), "None");
        let expr = input(&s.when, "minute hour day-of-month month day-of-week");
        let quick = Quick::read(&s.when);
        let (hours, minute, time, weekday) = match quick {
            Some(Quick::EveryHours { hours, minute }) => (hours, minute, (9, 0), 1),
            Some(Quick::Daily { hour, minute } | Quick::Weekdays { hour, minute }) => {
                (3, 0, (hour, minute), 1)
            }
            Some(Quick::Weekly {
                weekday,
                hour,
                minute,
            }) => (3, 0, (hour, minute), weekday),
            None => (3, 0, (9, 0), 1),
        };
        let hours = input(&hours.to_string(), "3");
        let minute = input(&format!("{minute:02}"), "00");
        let time = input(&format!("{:02}:{:02}", time.0, time.1), "09:00");
        let when = match quick {
            Some(Quick::EveryHours { .. }) => WhenKind::EveryHours,
            Some(Quick::Daily { .. }) => WhenKind::Daily,
            Some(Quick::Weekdays { .. }) => WhenKind::Weekdays,
            Some(Quick::Weekly { .. }) => WhenKind::Weekly,
            None => WhenKind::Expression,
        };
        let mut area = |text: &str| {
            let state = cx.new(|cx| {
                TextareaState::new(window, cx)
                    .auto_grow(4, 16)
                    .default_value(text.to_string())
            });
            cx.subscribe(&state, |_, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            })
            .detach();
            state
        };
        let (prompt_kind, prompt, workflow, body) = match &s.starts {
            Starts::Prompt { prompt } => (true, area(prompt), None, area("")),
            Starts::Workflow { workflow, body, .. } => {
                (false, area(""), Some(workflow.clone()), area(body))
            }
        };
        Self {
            original: (!new).then(|| s.clone()),
            id: s.id.clone(),
            name,
            project: (!s.project.as_os_str().is_empty()).then(|| s.project.clone()),
            prompt_kind,
            prompt,
            workflow,
            title,
            body,
            agent: s.agent.clone(),
            mode: s.mode.clone(),
            options: s.options.clone(),
            tools: s.tools.clone(),
            labels,
            when,
            hours,
            minute,
            time,
            weekday,
            expr,
            missed: s.missed,
            overlap: s.overlap,
            on: s.on,
            opened: None,
        }
    }

    /// The expression the form says, or why it says none.
    pub(super) fn when_expr(&self, cx: &App) -> Result<String, String> {
        let text = |state: &Entity<InputState>| state.read(cx).value().trim().to_string();
        let number = |state: &Entity<InputState>, what: &str, max: u8| {
            text(state)
                .parse::<u8>()
                .ok()
                .filter(|n| *n <= max)
                .ok_or_else(|| format!("Give {what} as a number up to {max}"))
        };
        let time = || {
            let typed = text(&self.time);
            let (hour, minute) = typed
                .split_once(':')
                .and_then(|(h, m)| Some((h.parse::<u8>().ok()?, m.parse::<u8>().ok()?)))
                .filter(|(h, m)| *h < 24 && *m < 60 && typed.len() == 5)
                .ok_or("Give the time as HH:MM, such as 09:00")?;
            Ok::<_, String>((hour, minute))
        };
        let quick = match self.when {
            WhenKind::Expression => return Ok(text(&self.expr)),
            WhenKind::EveryHours => {
                let hours = number(&self.hours, "the hours", 23)?;
                if hours == 0 {
                    return Err("Give the hours as a number from 1".to_string());
                }
                Quick::EveryHours {
                    hours,
                    minute: number(&self.minute, "the minute", 59)?,
                }
            }
            WhenKind::Daily => {
                let (hour, minute) = time()?;
                Quick::Daily { hour, minute }
            }
            WhenKind::Weekdays => {
                let (hour, minute) = time()?;
                Quick::Weekdays { hour, minute }
            }
            WhenKind::Weekly => {
                let (hour, minute) = time()?;
                Quick::Weekly {
                    weekday: self.weekday,
                    hour,
                    minute,
                }
            }
        };
        Ok(quick.expr())
    }

    /// The schedule the form says, kept fields of the one opened carried.
    pub(super) fn schedule(&self, cx: &App) -> Schedule {
        let text = |state: &Entity<InputState>| state.read(cx).value().trim().to_string();
        let area = |state: &Entity<TextareaState>| state.read(cx).value().trim().to_string();
        let starts = match self.prompt_kind {
            true => Starts::Prompt {
                prompt: area(&self.prompt),
            },
            false => Starts::Workflow {
                workflow: self.workflow.clone().unwrap_or_default(),
                title: text(&self.title),
                body: area(&self.body),
            },
        };
        let original = self.original.as_ref();
        Schedule {
            id: self.id.clone(),
            name: text(&self.name),
            project: self.project.clone().unwrap_or_default(),
            on: self.on,
            when: self.when_expr(cx).unwrap_or_default(),
            agent: self.agent.clone(),
            mode: self.mode.clone(),
            tools: self.tools.clone(),
            labels: onehand_core::issues::parse_labels(&text(&self.labels)),
            missed: self.missed,
            overlap: self.overlap,
            on_since: original.map_or(0, |o| o.on_since),
            last_fired: original.and_then(|o| o.last_fired),
            said: original.and_then(|o| o.said.clone()),
            options: self.options.clone(),
            starts,
        }
    }

    /// Why it cannot be saved, a sentence each.
    pub(super) fn problems(&self, cx: &App) -> Vec<String> {
        let mut said = schedule::problems(&self.schedule(cx));
        if let Err(why) = self.when_expr(cx) {
            said.retain(|p| !p.starts_with("When:"));
            said.push(why);
        }
        said
    }

    /// Whether the form says anything it did not say when opened.
    pub(super) fn dirty(&self, cx: &App) -> bool {
        self.opened
            .as_ref()
            .is_some_and(|opened| *opened != self.schedule(cx))
    }
}
