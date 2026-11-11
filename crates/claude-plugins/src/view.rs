//! The Plugins mode's view, and the carrier of its drive: which project, what
//! `claude` last said about it and the change in flight are
//! `onehand_core::claude_plugins::drive::PluginsDrive`'s; what lives here is
//! carrying its reads and changes off the UI loop, and the drawing.

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, AppContext as _, ClickEvent, Context, Entity, FocusHandle,
    InteractiveElement as _, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement as _, Styled, Window, div,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::input::{InputEvent, InputState};
use gpui_component::{ActiveTheme, StyledExt};
use onehand_core::claude_plugins::cli::{self, Available, Catalog, Change, Plugin, Scope};
use onehand_core::claude_plugins::drive::{Effect, Input, PluginsDrive};
use onehand_ui::Short as _;
use onehand_ui::search_input;
use onehand_ui::{
    ControlSize, Refuses as _, action, choice, hint, status_ink, status_line, switch,
};
use onehand_workbench::{Ask, Request};
use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

mod details;
mod rows;
use details::note;

/// How many installed plugins are drawn. Far past what anybody installs, so it
/// bounds the work of building rows rather than editing the list; the one time
/// it bites, it says so.
const INSTALLED_CAP: usize = 200;

/// How many of the marketplaces' plugins are drawn at once. The official
/// marketplace alone offers a few hundred, and a list nobody scrolls to the end
/// of is not worth building to the end — the search is how the rest are
/// reached, and the list says how many it left out.
const MARKET_CAP: usize = 60;

/// Why a control that changes a plugin refuses while another change runs:
/// the command line takes one change at a time, and what is running is said
/// above the list.
const ONE_AT_A_TIME: &str = "Another change is running";

/// Which of the mode's two lists is showing.
///
/// Two lists and not one page, because they answer different questions — what
/// this project has, and what could be added — and stacked, the second pushed
/// the first's last rows out of reach beneath a few hundred it had nothing to
/// do with, and a search at the top of that half scrolled away with it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tab {
    Installed,
    Marketplace,
}

impl Tab {
    const ALL: [Tab; 2] = [Tab::Installed, Tab::Marketplace];

    fn label(self) -> &'static str {
        match self {
            Tab::Installed => "Installed",
            Tab::Marketplace => "Marketplace",
        }
    }
}

/// Which installed plugins are listed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Filter {
    All,
    Global,
    Project,
    Local,
    HasUpdate,
}

impl Filter {
    const ALL: [Filter; 5] = [
        Filter::All,
        Filter::Global,
        Filter::Project,
        Filter::Local,
        Filter::HasUpdate,
    ];

    fn label(self) -> &'static str {
        match self {
            Filter::All => "All",
            Filter::Global => "Global",
            Filter::Project => "Project",
            Filter::Local => "Local",
            Filter::HasUpdate => "Has update",
        }
    }

    /// A scope filter keeps a plugin installed at that scope; it is where a
    /// plugin can be removed from, which is what somebody filtering by scope
    /// is usually about to do.
    fn keeps(self, plugin: &Plugin) -> bool {
        match self {
            Filter::All => true,
            Filter::Global => plugin.installed.contains(&Scope::User),
            Filter::Project => plugin.installed.contains(&Scope::Project),
            Filter::Local => plugin.installed.contains(&Scope::Local),
            Filter::HasUpdate => plugin.update.is_some(),
        }
    }
}

pub(crate) struct PluginsView {
    /// Which project is listed, what `claude` last said of it, the one
    /// change in flight (one at a time, because every change is a rewrite of
    /// a settings file or of Claude Code's install record, and two landing
    /// together can each write a copy missing the other's), why the last one
    /// was refused, where *Install* lands, and every change that went through.
    drive: PluginsDrive,
    /// Which list is showing. Per window rather than per root, and not
    /// persisted: which list somebody was reading is not a fact about the
    /// project, and a mode that came back on the catalog would hide what is
    /// installed behind a click.
    tab: Tab,
    /// The marketplace the catalog is narrowed to, or all of them.
    market: Option<String>,
    /// The marketplace search. Made on the first draw, since an input needs a
    /// window to be made in.
    query: Option<Entity<InputState>>,
    /// The installed list's search, made the same way.
    installed_query: Option<Entity<InputState>>,
    filter: Filter,
    /// One focus handle per installed row, by plugin, so the rows are tab
    /// stops and a row keeps its focus across a re-list.
    rows: HashMap<String, FocusHandle>,
    /// One focus handle per marketplace row, by place, so those rows are tab
    /// stops too.
    offered: Vec<FocusHandle>,
    /// How the banner's *Restart agent* reaches the shell.
    ask: Ask,
}

impl PluginsView {
    pub(crate) fn new(ask: Ask, cx: &mut App) -> Entity<Self> {
        cx.new(|_| Self {
            drive: PluginsDrive::default(),
            tab: Tab::Installed,
            market: None,
            query: None,
            installed_query: None,
            filter: Filter::All,
            rows: HashMap::new(),
            offered: Vec::new(),
            ask,
        })
    }

    pub(crate) fn set_root(&mut self, root: &Path, cx: &mut Context<Self>) {
        self.drive.set_root(root);
        cx.notify();
    }

    pub(crate) fn forget_root(&mut self, root: &Path, cx: &mut Context<Self>) {
        self.drive.forget_root(root);
        cx.notify();
    }

    pub(crate) fn agent_started(&mut self, since: Option<Instant>, cx: &mut Context<Self>) {
        self.drive.agent_started(since);
        cx.notify();
    }

    pub(crate) fn mark_stale(&mut self, cx: &mut Context<Self>) {
        self.drive.mark_stale();
        cx.notify();
    }

    /// Make one change, unless one is on its way.
    fn change(&mut self, change: Change, cx: &mut Context<Self>) {
        let effects = self.drive.change(change);
        self.perform(effects, cx);
    }

    /// Carry out the drive's effects, in order, each off the UI loop, with
    /// its answer reported back.
    fn perform(&mut self, effects: Vec<Effect>, cx: &mut Context<Self>) {
        for effect in effects {
            match effect {
                Effect::List { root, token } => {
                    cx.spawn(async move |view, cx| {
                        let listed = cx
                            .background_executor()
                            .spawn(async move { cli::list_blocking(&root) })
                            .await;
                        let _ = view.update(cx, |view: &mut Self, cx| {
                            view.landed(Input::Listed { token, listed }, cx)
                        });
                    })
                    .detach();
                }
                Effect::Change { root, change } => {
                    cx.spawn(async move |view, cx| {
                        let done = cx
                            .background_executor()
                            .spawn({
                                let (root, change) = (root.clone(), change.clone());
                                async move { cli::change_blocking(&root, &change) }
                            })
                            .await;
                        let _ = view.update(cx, |view: &mut Self, cx| {
                            let now = Instant::now();
                            view.landed(
                                Input::Changed {
                                    root,
                                    change,
                                    done,
                                    now,
                                },
                                cx,
                            )
                        });
                    })
                    .detach();
                }
            }
        }
        cx.notify();
    }

    /// Report what the world said, and carry out what the drive answers.
    fn landed(&mut self, input: Input, cx: &mut Context<Self>) {
        let effects = self.drive.handle(input);
        self.perform(effects, cx);
    }

    fn query(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<InputState> {
        if let Some(query) = &self.query {
            return query.clone();
        }
        let query = cx.new(|cx| InputState::new(window, cx).placeholder("Search the marketplaces"));
        cx.subscribe(&query, |_, _, _: &InputEvent, cx| cx.notify())
            .detach();
        self.query = Some(query.clone());
        query
    }

    fn installed_query(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        if let Some(query) = &self.installed_query {
            return query.clone();
        }
        let query =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search installed plugins"));
        cx.subscribe(&query, |_, _, _: &InputEvent, cx| cx.notify())
            .detach();
        self.installed_query = Some(query.clone());
        query
    }

    /// Whether `change` is the one in flight, so the control that asked for
    /// it — and only that one — shows it working.
    fn running(&self, change: &Change) -> bool {
        self.drive.busy() == Some(change)
    }
}

impl Render for PluginsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Being drawn is what says the listing is worth reading, as it is for
        // the Markdown mode's walk. Not while a change is out: its own landing
        // asks for one, and a listing read halfway through it is already stale.
        let effects = self.drive.list_if_due();
        self.perform(effects, cx);
        let body = self.body(window, cx);
        let refused = self.drive.refused().map(str::to_string);
        div()
            .flex_1()
            .min_h_0()
            .v_flex()
            .child(body)
            .children(refused.map(|why| status_line(why, status_ink(cx).warning)))
    }
}

impl PluginsView {
    fn body(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        if self.drive.root().is_none() {
            return hint("No project selected", cx);
        }
        let query = self.query(window, cx);
        let installed_query = self.installed_query(window, cx);
        // Focus handles first, while nothing else here is borrowed: a row is
        // a tab stop, and its handle has to outlive the frame it is drawn in.
        if let Some(Ok(catalog)) = self.drive.catalog() {
            let ids: Vec<String> = catalog.installed.iter().map(|p| p.id.clone()).collect();
            self.rows.retain(|id, _| ids.contains(id));
            for id in ids {
                self.rows
                    .entry(id)
                    .or_insert_with(|| cx.focus_handle().tab_stop(true));
            }
        }
        while self.offered.len() < MARKET_CAP {
            self.offered.push(cx.focus_handle().tab_stop(true));
        }
        // *Has update* is offered only while something has one. Once the
        // last is updated the chip goes, and a filter left on it would keep
        // an empty list with nothing highlighted to say why — so it falls
        // back to all.
        if self.filter == Filter::HasUpdate
            && let Some(Ok(catalog)) = self.drive.catalog()
            && !catalog.installed.iter().any(|p| p.update.is_some())
        {
            self.filter = Filter::All;
        }
        let catalog = match self.drive.catalog() {
            None => return hint("Reading Claude Code's plugins…", cx),
            Some(Err(why)) => return self.failed(why.clone(), cx),
            Some(Ok(catalog)) => catalog,
        };
        let labels: [SharedString; 2] = [
            format!("Installed {}", catalog.installed.len()).into(),
            format!("Marketplace {}", catalog.available.len()).into(),
        ];
        let tabs = switch(
            "plugins-tab",
            &labels,
            Tab::ALL
                .iter()
                .position(|tab| *tab == self.tab)
                .unwrap_or(0),
            ControlSize::Short,
            cx.listener(|view: &mut Self, i: &usize, _, cx| {
                view.tab = Tab::ALL[*i];
                cx.notify();
            }),
            cx,
        );

        // Only the list on screen is built: the other is a click away and
        // costs nothing until it is shown.
        let (controls, rows): (AnyElement, Vec<AnyElement>) =
            match self.tab {
                Tab::Installed => {
                    let needle = installed_query.read(cx).value().trim().to_lowercase();
                    let shown: Vec<&Plugin> = catalog
                        .installed
                        .iter()
                        .filter(|plugin| self.filter.keeps(plugin))
                        .filter(|plugin| {
                            needle.is_empty()
                                || plugin.id.to_lowercase().contains(&needle)
                                || plugin
                                    .inventory
                                    .description
                                    .as_deref()
                                    .is_some_and(|d| d.to_lowercase().contains(&needle))
                        })
                        .collect();
                    // The cut is of what the filter and the search left, not
                    // of everything installed: counted from the whole list, a
                    // search matching three said fifty more were hidden.
                    let cut = shown.len().saturating_sub(INSTALLED_CAP);
                    let shown: Vec<&Plugin> = shown.into_iter().take(INSTALLED_CAP).collect();
                    // On first, off after: what a session here gets is what is read
                    // first, and a plugin turned off is a record of a choice, kept
                    // below the ones in use rather than mixed in among them.
                    let (on, off): (Vec<&Plugin>, Vec<&Plugin>) = shown
                        .into_iter()
                        .partition(|plugin| plugin.in_force(Scope::Local));
                    let mut rows: Vec<AnyElement> = on
                        .iter()
                        .enumerate()
                        .map(|(i, plugin)| self.installed_row(i, plugin, window, cx))
                        .collect();
                    if !off.is_empty() {
                        rows.push(
                            div()
                                .px_2()
                                .pt_3()
                                .pb_1()
                                .text_xs()
                                .font_medium()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!("Disabled · {}", off.len()))
                                .into_any_element(),
                        );
                        rows.extend(off.iter().enumerate().map(|(i, plugin)| {
                            self.installed_row(on.len() + i, plugin, window, cx)
                        }));
                    }
                    if rows.is_empty() {
                        rows.push(note(
                            if catalog.installed.is_empty() {
                                "No plugins reach this project yet".to_string()
                            } else if needle.is_empty() {
                                "No plugins match this filter".to_string()
                            } else {
                                format!("No plugins match “{needle}”")
                            },
                            cx,
                        ));
                    }
                    if cut > 0 {
                        rows.push(note(format!("{cut} more not shown"), cx));
                    }
                    (self.installed_controls(&installed_query, catalog, cx), rows)
                }
                Tab::Marketplace => {
                    let needle = query.read(cx).value().trim().to_lowercase();
                    let matching: Vec<&Available> = catalog
                        .available
                        .iter()
                        .filter(|plugin| {
                            self.market
                                .as_ref()
                                .is_none_or(|market| &plugin.marketplace == market)
                        })
                        .filter(|plugin| plugin.matches(&needle))
                        .collect();
                    let cut = matching.len().saturating_sub(MARKET_CAP);
                    let mut rows: Vec<AnyElement> = matching
                        .iter()
                        .take(MARKET_CAP)
                        .enumerate()
                        .map(|(i, plugin)| self.available_row(i, plugin, catalog, cx))
                        .collect();
                    if rows.is_empty() {
                        rows.push(note(
                            match needle.is_empty() {
                                true => "No plugins in this marketplace yet".to_string(),
                                false => format!("No plugins match “{needle}”"),
                            },
                            cx,
                        ));
                    }
                    if cut > 0 {
                        rows.push(note(format!("{cut} more not shown; narrow the search"), cx));
                    }
                    (self.market_controls(&query, catalog, cx), rows)
                }
            };

        div()
            .flex_1()
            .min_h_0()
            .v_flex()
            .gap_4()
            .p_4()
            .child(tabs)
            .child(controls)
            .child(
                div()
                    // Keyed by the tab, so each list keeps a scroll of its own
                    // instead of the one showing inheriting the other's.
                    .id(self.tab.label())
                    .key_context("PluginList")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    // One hairline box, its rows divided by hairlines, so the
                    // inventory reads as one list rather than loose cards.
                    .child(
                        div()
                            .v_flex()
                            .rounded(cx.theme().radius)
                            .border_1()
                            .border_color(cx.theme().border)
                            .overflow_hidden()
                            .children(rows.into_iter().enumerate().map(|(i, row)| {
                                div()
                                    .when(i > 0, |row| {
                                        row.border_t_1().border_color(cx.theme().border)
                                    })
                                    .child(row)
                            })),
                    ),
            )
            // What is happening, while it happens, and what waits on a
            // restart: under the list, so neither moves it as it comes and
            // goes.
            .children(self.drive.busy().map(|change| note(change.doing(), cx)))
            .children(self.pending_banner(cx))
            .into_any_element()
    }

    /// A listing that failed: why, and the way to ask again — otherwise the
    /// only retry is leaving the mode and coming back, which nothing says.
    fn failed(&self, why: String, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex_1()
            .v_flex()
            .gap_2()
            .p_2()
            .child(status_line(why, status_ink(cx).warning))
            .child(
                div().px_2().child(
                    action("plugins-retry")
                        .short()
                        .secondary()
                        .label("Try again")
                        .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                            view.drive.retry();
                            cx.notify()
                        })),
                ),
            )
            .into_any_element()
    }

    /// The catalog's search, and a chip per marketplace to narrow it by —
    /// said once above the list rather than on every row under it. Offered
    /// only where there is more than one marketplace to choose between.
    fn market_controls(
        &self,
        query: &Entity<InputState>,
        catalog: &Catalog,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut markets: Vec<String> = catalog
            .available
            .iter()
            .map(|plugin| plugin.marketplace.clone())
            .collect();
        markets.sort();
        markets.dedup();
        let current = self.market.clone();
        let chips = (markets.len() > 1).then(|| {
            let all = choice(
                "plugins-market-all",
                "All",
                current.is_none(),
                ControlSize::Short,
                cx,
            )
            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                view.market = None;
                cx.notify();
            }));
            let each = markets.into_iter().enumerate().map(|(i, market)| {
                let on = current.as_deref() == Some(market.as_str());
                choice(
                    ("plugins-market", i),
                    market.clone(),
                    on,
                    ControlSize::Short,
                    cx,
                )
                .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                    view.market = Some(market.clone());
                    cx.notify();
                }))
            });
            div().h_flex().flex_wrap().gap_1().child(all).children(each)
        });
        filter_bar(query, chips, cx)
    }

    /// What the agent on screen has not loaded, and the way to load it —
    /// only while there is something, so an idle panel carries no line about
    /// restarting.
    fn pending_banner(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let pending = self.drive.pending();
        if pending == 0 {
            return None;
        }
        let ask = self.ask.clone();
        let changes = if pending == 1 {
            "change applies"
        } else {
            "changes apply"
        };
        Some(
            div()
                .h_flex()
                .items_center()
                .gap_2()
                .px_3()
                .py_2()
                .rounded(cx.theme().radius)
                .border_1()
                .border_color(cx.theme().border)
                .text_size(onehand_ui::tokens::CHROME_TEXT)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(format!("{pending} {changes} when the agent restarts")),
                )
                .child(
                    action("plugins-restart")
                        .short()
                        .secondary()
                        .label("Restart the agent")
                        .refuses(self.drive.busy().is_some().then_some(ONE_AT_A_TIME))
                        .on_click(move |_, window, cx| ask(&Request::RestartAgent, window, cx)),
                )
                .into_any_element(),
        )
    }

    /// The installed list's search and its filter chips. *Has update* is
    /// offered only while a plugin has one: a filter that can only ever come
    /// back empty is a control that says nothing.
    fn installed_controls(
        &self,
        query: &Entity<InputState>,
        catalog: &Catalog,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let any_update = catalog.installed.iter().any(|p| p.update.is_some());
        let current = self.filter;
        let chips = Filter::ALL
            .into_iter()
            .enumerate()
            .filter(|(_, filter)| *filter != Filter::HasUpdate || any_update)
            .map(|(i, filter)| {
                choice(
                    ("plugins-filter", i),
                    filter.label(),
                    filter == current,
                    ControlSize::Short,
                    cx,
                )
                .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                    view.filter = filter;
                    cx.notify();
                }))
            });
        filter_bar(
            query,
            Some(div().h_flex().flex_wrap().gap_1().children(chips)),
            cx,
        )
    }
}

/// A list's filter bar: the search taking the free width, then its choices,
/// wrapping under it when the dock is narrow.
fn filter_bar(query: &Entity<InputState>, choices: Option<gpui::Div>, cx: &App) -> AnyElement {
    div()
        .h_flex()
        .flex_wrap()
        .items_center()
        .gap_2()
        .child(
            div()
                .flex_1()
                .min_w(SEARCH_MIN)
                .child(search_input(query, cx).short()),
        )
        .children(choices)
        .into_any_element()
}

/// The narrowest the search gets before the filter bar wraps.
const SEARCH_MIN: gpui::Rems = gpui::rems(10.);

#[cfg(test)]
mod tests;
