//! The Plugins mode's own state — which project, what `claude` last said about
//! it, the change in flight — and how it is drawn.

use crate::cli::{self, Available, Catalog, Change, Plugin, Scope, Verb};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, AppContext as _, ClickEvent, Context, Entity, FocusHandle,
    InteractiveElement as _, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement as _, Styled, Task, Window, div,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::{ActiveTheme, Disableable as _, Selectable as _, Sizable as _, StyledExt};
use onehand_plugin_host::{Ask, Request, action, hint, status_line, switch};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
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

/// One change that went through: where it was made, whether it reaches every
/// project, and when.
struct Made {
    root: PathBuf,
    /// A change at the global scope, which every project's agent loads.
    everywhere: bool,
    at: Instant,
}

/// How many changes are kept to count from. Far past how many anybody makes
/// between two restarts.
const MADE_CAP: usize = 256;

/// How many of `made` the agent in `root`, started at `since`, has not
/// loaded: those after it started, made in this project or globally. None
/// while no agent is running — the next one to start loads all of them.
fn pending_count(made: &[Made], root: &Path, since: Option<Instant>) -> usize {
    let Some(since) = since else {
        return 0;
    };
    made.iter()
        .filter(|made| made.at > since && (made.everywhere || made.root == root))
        .count()
}

pub(crate) struct PluginsView {
    root: Option<PathBuf>,
    /// Which list is showing. Per window rather than per root, and not
    /// persisted: which list somebody was reading is not a fact about the
    /// project, and a mode that came back on the catalog would hide what is
    /// installed behind a click.
    tab: Tab,
    /// What `claude` last said about the active root, or why it could not.
    /// Cleared on a project switch, since another project's installs and
    /// settings are not this one's.
    catalog: Option<Result<Catalog, String>>,
    /// Whether the listing needs reading again before it is next drawn.
    stale: bool,
    /// The listing in flight, held only so that starting another drops it.
    _list: Option<Task<()>>,
    /// The one change in flight.
    ///
    /// **One at a time**, because every change is a rewrite of a settings file
    /// or of Claude Code's install record, and two landing together can each
    /// write a copy missing the other's.
    busy: Option<Change>,
    _change: Option<Task<()>>,
    /// Why the last change did not happen, kept until the next one does or the
    /// project changes.
    ///
    /// A standing line rather than a notification: an install that was refused
    /// — most often for a command it wants run, which is left to a person on
    /// purpose — says what to do next, and a toast that fades takes that with it.
    status: Option<String>,
    /// Where *Install* lands: the scope last picked from its caret, so the
    /// choice made once carries on to the next install.
    install_scope: Scope,
    /// The marketplace the catalog is narrowed to, or all of them.
    market: Option<String>,
    /// The marketplace search. Made on the first draw, since an input needs a
    /// window to be made in.
    query: Option<Entity<InputState>>,
    /// The installed list's search, made the same way.
    installed_query: Option<Entity<InputState>>,
    filter: Filter,
    /// Every change that went through, with the project it was made in and
    /// when. What the agent on screen has not seen is the ones after it
    /// started — which is what the pending banner counts.
    made: Vec<Made>,
    /// When the agent on screen started, as the shell last said; `None` while
    /// no session shows, when there is nothing running to be out of date.
    since: Option<Instant>,
    /// One focus handle per installed row, by plugin, so the rows are tab
    /// stops and a row keeps its focus across a re-list.
    rows: HashMap<String, FocusHandle>,
    /// How the banner's *Restart agent* reaches the shell.
    ask: Ask,
}

impl PluginsView {
    pub(crate) fn new(ask: Ask, cx: &mut App) -> Entity<Self> {
        cx.new(|_| Self {
            root: None,
            tab: Tab::Installed,
            catalog: None,
            stale: false,
            _list: None,
            busy: None,
            _change: None,
            status: None,
            install_scope: Scope::User,
            market: None,
            query: None,
            installed_query: None,
            filter: Filter::All,
            made: Vec::new(),
            since: None,
            rows: HashMap::new(),
            ask,
        })
    }

    pub(crate) fn set_root(&mut self, root: &Path, cx: &mut Context<Self>) {
        if self.root.as_deref() == Some(root) {
            return;
        }
        self.root = Some(root.to_path_buf());
        self.catalog = None;
        // A refusal is about the project it happened in.
        self.status = None;
        self.stale = true;
        cx.notify();
    }

    pub(crate) fn forget_root(&mut self, root: &Path, cx: &mut Context<Self>) {
        if self.root.as_deref() == Some(root) {
            self.root = None;
            self.catalog = None;
            self.status = None;
            self._list = None;
            cx.notify();
        }
    }

    pub(crate) fn agent_started(&mut self, since: Option<Instant>, cx: &mut Context<Self>) {
        self.since = since;
        cx.notify();
    }

    /// How many changes the agent on screen has not loaded: those made in
    /// this project since it started. None while no agent is running, since a
    /// session started next will load all of them.
    ///
    /// Counts what went through this mode alone — a plugin installed from a
    /// terminal is not seen here, and is not claimed to be.
    fn pending(&self) -> usize {
        match &self.root {
            Some(root) => pending_count(&self.made, root, self.since),
            None => 0,
        }
    }

    pub(crate) fn mark_stale(&mut self, cx: &mut Context<Self>) {
        self.stale = true;
        cx.notify();
    }

    /// Ask `claude` what is installed for the active root.
    ///
    /// The answer is dropped if the root has moved on while it was out, since
    /// it names another project's installs and settings.
    fn list(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        self._list = Some(cx.spawn(async move |view, cx| {
            let listed = cx
                .background_executor()
                .spawn({
                    let root = root.clone();
                    async move { cli::list_blocking(&root) }
                })
                .await;
            let _ = view.update(cx, |view: &mut Self, cx| {
                if view.root.as_deref() == Some(root.as_path()) {
                    view.catalog = Some(listed);
                    cx.notify();
                }
            });
        }));
    }

    /// Make one change, then read the listing again — the command line's
    /// answer to a change is a sentence, and what it did is only certain from
    /// what it lists afterwards.
    ///
    /// The change was made in the project it was pressed in, so a refusal is
    /// said only while that project is still the one on screen.
    fn change(&mut self, change: Change, cx: &mut Context<Self>) {
        if self.busy.is_some() {
            return;
        }
        let Some(root) = self.root.clone() else {
            return;
        };
        self.busy = Some(change.clone());
        self.status = None;
        cx.notify();
        self._change = Some(cx.spawn(async move |view, cx| {
            let done = cx
                .background_executor()
                .spawn({
                    let root = root.clone();
                    let change = change.clone();
                    async move { cli::change_blocking(&root, &change) }
                })
                .await;
            let _ = view.update(cx, |view: &mut Self, cx| {
                view.busy = None;
                match done {
                    // Fetching a catalog changes nothing an agent loads.
                    Ok(()) if change.verb == Verb::CheckUpdates => {}
                    Ok(()) => {
                        view.made.push(Made {
                            root: root.clone(),
                            // A move out of the global scope takes it away
                            // from every project, as much as one into it adds it.
                            everywhere: change.scope == Scope::User
                                || change.verb == Verb::Move(Scope::User),
                            at: Instant::now(),
                        });
                        // Only the latest can still be pending for a running
                        // agent; the rest are history nothing reads.
                        let over = view.made.len().saturating_sub(MADE_CAP);
                        view.made.drain(..over);
                    }
                    Err(why) if view.root.as_deref() == Some(root.as_path()) => {
                        view.status = Some(why)
                    }
                    Err(_) => {}
                }
                view.stale = true;
                cx.notify();
            });
        }));
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
        self.busy.as_ref() == Some(change)
    }
}

impl Render for PluginsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Being drawn is what says the listing is worth reading, as it is for
        // the Markdown mode's walk. Not while a change is out: its own landing
        // asks for one, and a listing read halfway through it is already stale.
        if self.stale && self.busy.is_none() {
            self.stale = false;
            self.list(cx);
        }
        let body = self.body(window, cx);
        div()
            .flex_1()
            .min_h_0()
            .v_flex()
            .child(body)
            .children(self.status.clone().map(|status| status_line(status, cx)))
    }
}

impl PluginsView {
    fn body(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        if self.root.is_none() {
            return hint("No project root", cx);
        }
        let query = self.query(window, cx);
        let installed_query = self.installed_query(window, cx);
        // Focus handles first, while nothing else here is borrowed: a row is
        // a tab stop, and its handle has to outlive the frame it is drawn in.
        if let Some(Ok(catalog)) = &self.catalog {
            let ids: Vec<String> = catalog.installed.iter().map(|p| p.id.clone()).collect();
            self.rows.retain(|id, _| ids.contains(id));
            for id in ids {
                self.rows
                    .entry(id)
                    .or_insert_with(|| cx.focus_handle().tab_stop(true));
            }
        }
        // *Has update* is offered only while something has one. Once the
        // last is updated the chip goes, and a filter left on it would keep
        // an empty list with nothing highlighted to say why — so it falls
        // back to all.
        if self.filter == Filter::HasUpdate
            && let Some(Ok(catalog)) = &self.catalog
            && !catalog.installed.iter().any(|p| p.update.is_some())
        {
            self.filter = Filter::All;
        }
        let catalog = match &self.catalog {
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
            gpui_component::Size::Small,
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
                                "Nothing installed reaches this project"
                            } else {
                                "No plugin matches"
                            },
                            cx,
                        ));
                    }
                    if cut > 0 {
                        rows.push(note(format!("… {cut} more not shown"), cx));
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
                        rows.push(note("No plugin matches the search", cx));
                    }
                    if cut > 0 {
                        rows.push(note(
                            format!("… {cut} more not shown — narrow the search"),
                            cx,
                        ));
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
            // What is happening, while it happens — only then, so an idle
            // panel carries no line above its list.
            .children(self.busy.as_ref().map(|change| note(change.doing(), cx)))
            .children(self.pending_banner(cx))
            .child(controls)
            .child(
                div()
                    // Keyed by the tab, so each list keeps a scroll of its own
                    // instead of the one showing inheriting the other's.
                    .id(self.tab.label())
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    // One hairline box, its rows divided by hairlines, so the
                    // inventory reads as one list rather than loose cards.
                    .child(
                        div()
                            .v_flex()
                            .rounded(cx.theme().radius_lg)
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
            .child(status_line(why, cx))
            .child(
                div().px_2().child(
                    action("plugins-retry")
                        .xsmall()
                        .outline()
                        .label("Try again")
                        .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                            view.catalog = None;
                            view.mark_stale(cx)
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
            let all = action("plugins-market-all")
                .xsmall()
                .ghost()
                .label("All")
                .selected(current.is_none())
                .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                    view.market = None;
                    cx.notify();
                }));
            let each = markets.into_iter().enumerate().map(|(i, market)| {
                let on = current.as_deref() == Some(market.as_str());
                action(("plugins-market", i))
                    .xsmall()
                    .ghost()
                    .label(market.clone())
                    .selected(on)
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                        view.market = Some(market.clone());
                        cx.notify();
                    }))
            });
            div().h_flex().flex_wrap().gap_1().child(all).children(each)
        });
        div()
            .v_flex()
            .gap_2()
            .child(Input::new(query).small())
            .children(chips)
            .into_any_element()
    }

    /// What the agent on screen has not loaded, and the way to load it —
    /// only while there is something, so an idle panel carries no line about
    /// restarting.
    fn pending_banner(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let pending = self.pending();
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
                .text_sm()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(format!("{pending} {changes} when the agent restarts")),
                )
                .child(
                    action("plugins-restart")
                        .small()
                        .outline()
                        .label("Restart agent")
                        .disabled(self.busy.is_some())
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
                action(("plugins-filter", i))
                    .xsmall()
                    .ghost()
                    .label(filter.label())
                    .selected(filter == current)
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                        view.filter = filter;
                        cx.notify();
                    }))
            });
        div()
            .v_flex()
            .gap_2()
            .child(Input::new(query).small())
            .child(div().h_flex().flex_wrap().gap_1().children(chips))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests;
