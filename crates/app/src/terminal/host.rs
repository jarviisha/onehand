//! The dock on a project that lives on a host.
//!
//! The host service runs the shells and keeps each one's recent output, so a
//! tab is a grid fed by its terminal's stream over the host's attach: showing
//! the project asks which terminals the host keeps for its folder and opens a
//! tab for each one not already shown. Closing a tab ends its shell there; a
//! tab whose stream ended (the shell exited, the attach was lost) only goes,
//! and anything still running there comes back the next time the project is
//! shown.
//!
//! The calls are tokio I/O, so they run on the ACP runtime and their answers
//! cross back on a oneshot. A stream is read on a thread of the grid's own.

use super::{Home, TerminalPanel};
use crate::controls::Short as _;
use crate::hosts;
use gpui::{
    AnyElement, AnyWindowHandle, AppContext as _, Context, IntoElement, ParentElement,
    SharedString, Styled, Task, Window, div,
};
use gpui_component::StyledExt as _;
use gpui_component::button::ButtonVariants as _;
use onehand_core::host::client::HostConn;
use onehand_core::host::wire::{HostTerm, TermInfo};

/// How the attach for the host project on screen is coming along.
#[derive(Default)]
pub(super) struct Reach {
    /// What stands where the tabs would be while there are none.
    said: Option<Said>,
    /// Asking the host for its terminals, until they are tabs.
    asking: Option<Task<()>>,
    /// A shell asked for while the host was being asked: opened once it has
    /// answered with none, in this window.
    pub(super) open_after: Option<AnyWindowHandle>,
}

enum Said {
    /// Something on its way, said as the line waiting for it.
    Waiting(SharedString),
    /// What failed, standing until *Try again* or the project is shown again.
    Failed(SharedString),
}

impl Reach {
    /// Whether nothing stands in place of the tabs.
    pub(super) fn is_quiet(&self) -> bool {
        self.said.is_none()
    }
}

impl TerminalPanel {
    /// The project on screen and its host, when it is on one.
    fn on_host(&self) -> Option<(Home, onehand_core::workspace::HostName)> {
        let home = self.root.clone()?;
        let host = home.0.clone()?;
        Some((home, host))
    }

    /// Ask the host the project on screen is on for the terminals it keeps
    /// for its folder, attaching first when the attach is not up: saying
    /// meanwhile what it waits on and, if it fails, why. A busy host is
    /// waited on as everywhere else. What lands for a project no longer on
    /// screen is dropped. Asked on arriving at the project and on showing the
    /// dock; while one ask is under way another is not made.
    pub(crate) fn show_host(&mut self, cx: &mut Context<Self>) {
        let Some((home, host)) = self.on_host() else {
            self.host = Reach::default();
            return;
        };
        if self.host.asking.is_some() {
            return;
        }
        if hosts::live(cx, &host).is_none() {
            self.host.said = Some(Said::Waiting(format!("Attaching to {host}…").into()));
        }
        self.host.asking = Some(cx.spawn(async move |panel, cx| {
            let say = |said: Said, cx: &mut gpui::AsyncApp| {
                let _ = panel.update(cx, |panel: &mut Self, cx| {
                    if panel.root.as_ref() == Some(&home) {
                        panel.host.said = Some(said);
                        cx.notify();
                    }
                });
            };
            let waiting =
                |line: String, cx: &mut gpui::AsyncApp| say(Said::Waiting(line.into()), cx);
            match hosts::attach_waiting(&host, cx, waiting).await {
                Ok(conn) => {
                    let asking = conn.clone();
                    let listed = cx
                        .update(|cx| hosts::on_runtime(cx, async move { asking.term_list().await }))
                        .await
                        .map_err(|e| e.to_string());
                    let _ = panel.update(cx, |panel: &mut Self, cx| {
                        panel.listed(home, conn, listed, cx)
                    });
                }
                Err(why) => {
                    say(Said::Failed(why.into()), cx);
                    let _ = panel.update(cx, |panel: &mut Self, _| panel.host.asking = None);
                }
            }
        }));
    }

    /// The host's terminals, asked over `conn`, have landed: each one in this
    /// folder not already a tab becomes one, and a shell asked for meanwhile
    /// is opened if there were none.
    fn listed(
        &mut self,
        home: Home,
        conn: HostConn,
        listed: Result<Vec<TermInfo>, String>,
        cx: &mut Context<Self>,
    ) {
        if self.root.as_ref() != Some(&home) {
            return;
        }
        let asked = self.host.open_after.take();
        self.host = Reach::default();
        let terms = match listed {
            Ok(terms) => terms,
            Err(why) => {
                self.host.said = Some(Said::Failed(
                    format!("Could not list the terminals: {why}").into(),
                ));
                cx.notify();
                return;
            }
        };
        // A tab read over an attach that has ended since shows nothing more;
        // its terminal comes back below, over this one.
        if let Some(set) = self.shells.get_mut(&home) {
            set.drop_where(|_, tab| tab.on_host.as_ref().is_some_and(|(c, _)| c.is_closed()));
        }
        let shown = self.host_terms(&home);
        let (add, open) = from_listing(&terms, &home.1, &shown, asked.is_some());
        for term in add {
            self.add_host_tab(home.clone(), conn.clone(), term, cx);
        }
        if let Some(window) = asked
            && open
        {
            self.open_on_host(window, cx);
        }
        cx.notify();
    }

    /// The host terminals `home` shows as tabs.
    fn host_terms(&self, home: &Home) -> Vec<HostTerm> {
        self.shells.get(home).map_or_else(Vec::new, |set| {
            set.tabs
                .iter()
                .filter_map(|tab| tab.on_host.as_ref().map(|(_, term)| *term))
                .collect()
        })
    }

    /// Start a shell in the host project on screen. While its terminals are
    /// still being asked for, or the attach is not up, it waits for them:
    /// one is started only if there were none.
    pub(super) fn open_on_host(&mut self, window: AnyWindowHandle, cx: &mut Context<Self>) {
        let Some((home, host)) = self.on_host() else {
            return;
        };
        if self.host.asking.is_some() {
            self.host.open_after = Some(window);
            return;
        }
        let Some(conn) = hosts::live(cx, &host) else {
            self.host.open_after = Some(window);
            return self.show_host(cx);
        };
        let (cols, rows) = onehand_terminal_ui::start_size();
        let (asking, cwd) = (conn.clone(), home.1.clone());
        let opening = hosts::on_runtime(cx, async move { asking.term_open(cwd, cols, rows).await });
        cx.spawn(async move |panel, cx| {
            let opened = opening.await.map_err(|e| e.to_string());
            let _ = cx.update_window(window, |_, window, cx| {
                panel.update(cx, |panel: &mut Self, cx| {
                    panel.opened(home, conn, opened, window, cx)
                })
            });
        })
        .detach();
    }

    /// A shell asked for on a host has started, or could not: its tab is
    /// added, taking the caret if the dock held it, or why is said as a
    /// failed start here is.
    fn opened(
        &mut self,
        home: Home,
        conn: HostConn,
        opened: Result<HostTerm, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Started for a project no longer on screen, it runs on there and is
        // a tab the next time that project is shown.
        if self.root.as_ref() != Some(&home) {
            return;
        }
        match opened {
            // Already a tab: a listing landed while it was being started.
            Ok(term) if self.host_terms(&home).contains(&term) => {}
            Ok(term) => {
                let held_focus = self.focus_handle.contains_focused(window, cx)
                    || self.focus_handle.is_focused(window);
                self.add_host_tab(home, conn, term, cx);
                self.status = None;
                if held_focus {
                    self.focus_active(window, cx);
                }
            }
            Err(e) if self.has_shell() => {
                use gpui_component::WindowExt as _;
                window.push_notification(crate::toast::failed("Opening a shell", e), cx);
            }
            Err(e) => self.status = Some(e),
        }
        cx.notify();
    }

    /// Show `term` as the newest tab of `home`, read and written over `conn`.
    pub(super) fn add_host_tab(
        &mut self,
        home: Home,
        conn: HostConn,
        term: HostTerm,
        cx: &mut Context<Self>,
    ) {
        let panel = cx.entity().downgrade();
        let resize = conn.clone();
        let ended = home.clone();
        // The id the tab is pushed with below.
        let id = self.shells.entry(home.clone()).or_default().next;
        let tab = onehand_terminal_ui::stream_tab(
            "shell".into(),
            conn.term_watch(term),
            conn.term_writer(term),
            move |cols, rows| resize.term_resize(term, cols as u16, rows as u16),
            crate::zoom::term_font_size(self.zoom),
            crate::theme::dock_surface(cx),
            cx,
            move |window, cx| {
                let _ = panel.update(cx, |panel: &mut Self, cx| {
                    panel.stream_ended(&ended, id, window, cx)
                });
            },
        );
        self.shells
            .entry(home)
            .or_default()
            .push(tab, Some((conn, term)));
        self.watch_running(cx);
        cx.notify();
    }

    /// The stream of tab `id` ended: that tab goes, and nothing is asked of the
    /// host. Focus moves only when it was inside the dock, as for a shell of
    /// this machine that exits.
    fn stream_ended(&mut self, home: &Home, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let held_focus = self.focus_handle.contains_focused(window, cx);
        let Some(set) = self.shells.get_mut(home) else {
            return;
        };
        let gone = set.drop_where(|_, tab| tab.id == id);
        if gone && held_focus && self.root.as_ref() == Some(home) {
            self.focus_active(window, cx);
        }
        cx.notify();
    }

    /// What stands where the tabs would be on a host project with none: the
    /// line an attach waits on, or why it failed with *Try again*.
    pub(super) fn host_said(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        Some(match self.host.said.as_ref()? {
            Said::Waiting(line) => onehand_ui::hint(line.clone(), cx),
            Said::Failed(why) => div()
                .flex_1()
                .min_h_0()
                .v_flex()
                .items_center()
                .justify_center()
                .gap_2()
                .child(
                    crate::controls::action("terminal-try-again")
                        .secondary()
                        .short()
                        .label("Try again")
                        .on_click(cx.listener(|panel: &mut Self, _, _, cx| {
                            panel.show_host(cx);
                            cx.notify();
                        })),
                )
                .child(onehand_ui::status_line(
                    why.clone(),
                    crate::theme::status_ink(cx).danger,
                ))
                .into_any_element(),
        })
    }
}

/// What a listing of the host's terminals comes to for the folder `folder`,
/// whose tabs already show `shown`: the terminals to add as tabs, and whether
/// to start one because a shell was `asked` for and there is none.
pub(super) fn from_listing(
    listed: &[TermInfo],
    folder: &std::path::Path,
    shown: &[HostTerm],
    asked: bool,
) -> (Vec<HostTerm>, bool) {
    let add: Vec<HostTerm> = listed
        .iter()
        .filter(|info| info.cwd == folder && !shown.contains(&info.term))
        .map(|info| info.term)
        .collect();
    let open = asked && shown.is_empty() && add.is_empty();
    (add, open)
}
