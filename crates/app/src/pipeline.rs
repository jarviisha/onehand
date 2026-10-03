//! Pipeline runs, as the app drives them: the templates on offer, the runs
//! under way, and the runs a previous session left unfinished.
//!
//! Everything a run decides is `onehand_core::pipeline`'s; what is here is
//! what needs a session, a timer or a window.

use gpui::{App, BorrowAppContext as _, Global, SharedString};
use onehand_core::pipeline::{self as core, PipelineRun, Template, files};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

mod driver;
pub(crate) use driver::{approve, revise, start, stop};

/// Every pipeline run in this process, and the templates they start from.
pub(crate) struct Pipelines {
    /// The templates on offer, shipped first. Empty until the boot read lands.
    templates: Vec<Entry>,
    /// Runs under way, by the uid of the session they drive.
    runs: HashMap<u64, driver::Driven>,
    /// Runs that ended short of a step's own outcome — their agent stopped or
    /// their session went, this process or a previous one — and wait for a
    /// person to resume or discard them.
    unfinished: Vec<PipelineRun>,
    writer: files::Writer,
}

impl Default for Pipelines {
    fn default() -> Self {
        Self {
            templates: Vec::new(),
            runs: HashMap::new(),
            unfinished: Vec::new(),
            writer: files::Writer::spawn(),
        }
    }
}

impl Global for Pipelines {}

/// How long quitting waits for the run files still being written.
const QUIT_WAIT: std::time::Duration = std::time::Duration::from_secs(2);

/// A template on offer: one onehand ships, or one of the person's own files,
/// read or why it could not be.
#[derive(Clone)]
pub(crate) struct Entry {
    pub(crate) template: Result<Template, String>,
    /// The file it is kept in; `None` for one onehand ships, which is read-only.
    pub(crate) file: Option<PathBuf>,
}

impl Entry {
    /// Its name, or the file's when it could not be read.
    pub(crate) fn name(&self) -> String {
        match (&self.template, &self.file) {
            (Ok(template), _) => template.name.clone(),
            (Err(_), Some(file)) => file
                .file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
            (Err(_), None) => String::new(),
        }
    }
}

/// Read the templates and the unfinished runs, off the UI thread.
pub(crate) fn boot(cx: &mut App) {
    cx.default_global::<Pipelines>();
    // The process exits right after its last window closes, and a run's last
    // save or its file's removal still queued would die with it: a finished
    // run offered for resuming at the next launch. The wait is in the future,
    // which runs once the windows are gone and their sessions have let go of
    // their runs, so those last writes are already queued ahead of it.
    let writer = cx.global::<Pipelines>().writer.clone();
    cx.on_app_quit(move |_| {
        let writer = writer.clone();
        async move {
            if !writer.flush(QUIT_WAIT) {
                eprintln!("onehand: a pipeline run's last write may not have landed");
            }
        }
    })
    .detach();
    reload_templates(cx);
    cx.spawn(async move |cx| {
        let found = cx
            .background_executor()
            .spawn(async { files::load_all_blocking(&files::runs_dir()) })
            .await;
        cx.update(|cx| {
            let runs: Vec<PipelineRun> = found
                .into_iter()
                .filter_map(|(path, read)| {
                    read.inspect_err(|why| {
                        eprintln!(
                            "onehand: a pipeline run in {} was not read: {why}",
                            path.display()
                        )
                    })
                    .ok()
                })
                .collect();
            cx.update_global::<Pipelines, _>(|p, _| p.unfinished.extend(runs));
            cx.refresh_windows();
        });
    })
    .detach();
}

/// Read the templates again: the shipped ones, then every file of the
/// person's own.
pub(crate) fn reload_templates(cx: &mut App) {
    cx.spawn(async move |cx| {
        let own = cx
            .background_executor()
            .spawn(async { core::store::load_all_blocking(&core::store::dir()) })
            .await;
        cx.update(|cx| {
            let templates = core::builtin::all()
                .into_iter()
                .map(|template| Entry {
                    template: Ok(template),
                    file: None,
                })
                .chain(own.into_iter().map(|(file, template)| Entry {
                    template,
                    file: Some(file),
                }))
                .collect();
            cx.update_global::<Pipelines, _>(|p, _| p.templates = templates);
            cx.refresh_windows();
        });
    })
    .detach();
}

/// How many templates a list or a menu of them draws before it says how many
/// more there are. Far more than anybody keeps; it is there so a folder of
/// generated files cannot freeze the page.
pub(crate) const TEMPLATES_SHOWN: usize = 50;

/// The templates on offer, shipped first.
pub(crate) fn templates(cx: &App) -> Vec<Entry> {
    cx.try_global::<Pipelines>()
        .map(|p| p.templates.clone())
        .unwrap_or_default()
}

/// Where the run on session `uid` stands, as its strip draws it.
pub(crate) struct Shown {
    pub(crate) name: SharedString,
    pub(crate) steps: Vec<SharedString>,
    pub(crate) at: usize,
    /// It waits for *Continue* or *Revise…*.
    pub(crate) awaiting: bool,
}

/// Where the run on session `uid` stands, if one drives it.
pub(crate) fn shown(uid: u64, cx: &App) -> Option<Shown> {
    let run = &cx.try_global::<Pipelines>()?.runs.get(&uid)?.run;
    Some(Shown {
        name: run.template.name.clone().into(),
        steps: run
            .template
            .steps
            .iter()
            .map(|step| step.label.clone().into())
            .collect(),
        at: run.step,
        awaiting: run.awaiting_approval(),
    })
}

/// An unfinished run, as a project page offers it.
#[derive(Clone, PartialEq)]
pub(crate) struct Unfinished {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) title: String,
    pub(crate) step: String,
}

/// The unfinished runs started from, or working in, the project at `root`.
pub(crate) fn unfinished_in(root: &Path, cx: &App) -> Vec<Unfinished> {
    let Some(p) = cx.try_global::<Pipelines>() else {
        return Vec::new();
    };
    p.unfinished
        .iter()
        .filter(|run| run.setup.repo == root || run.setup.dir == root)
        .map(|run| Unfinished {
            id: run.id.clone(),
            name: run.template.name.clone(),
            title: run.brief.title.clone(),
            step: run
                .current()
                .map_or_else(String::new, |step| step.label.clone()),
        })
        .collect()
}

/// Take the unfinished run `id` off the list, to resume it.
pub(crate) fn take_unfinished(id: &str, cx: &mut App) -> Option<PipelineRun> {
    let taken = cx.update_global::<Pipelines, _>(|p, _| {
        let at = p.unfinished.iter().position(|run| run.id == id)?;
        Some(p.unfinished.remove(at))
    });
    cx.refresh_windows();
    taken
}

/// Put a run back on the unfinished list, its file kept.
pub(crate) fn park(run: PipelineRun, cx: &mut App) {
    let file = files::run_file(&files::runs_dir(), &run.id);
    cx.update_global::<Pipelines, _>(|p, _| {
        p.writer
            .send(files::FileOp::Save(file, Box::new(run.clone())));
        p.unfinished.push(run);
    });
    cx.refresh_windows();
}

/// Drop the unfinished run `id` and its file.
pub(crate) fn discard(id: &str, cx: &mut App) {
    if take_unfinished(id, cx).is_some() {
        let file = files::run_file(&files::runs_dir(), id);
        cx.update_global::<Pipelines, _>(|p, _| p.writer.send(files::FileOp::Remove(file)));
    }
}
