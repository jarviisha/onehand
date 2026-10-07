//! The templates on offer: the ones onehand ships, then the person's own
//! files. Runs of them are tasks (`crate::task`).

use gpui::{App, BorrowAppContext as _, Global};
use onehand_core::workflow::{self as core, Template};
use std::path::PathBuf;

/// The templates on offer, shipped first. Empty until the boot read lands.
#[derive(Default)]
pub(crate) struct Workflows {
    templates: Vec<Entry>,
}

impl Global for Workflows {}

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

/// Read the templates, off the UI thread.
pub(crate) fn boot(cx: &mut App) {
    cx.default_global::<Workflows>();
    reload_templates(cx);
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
            cx.update_global::<Workflows, _>(|p, _| p.templates = templates);
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
    cx.try_global::<Workflows>()
        .map(|p| p.templates.clone())
        .unwrap_or_default()
}

/// The workflow a run's `snapshot` was taken of, by its id, as it is on offer
/// now: what a task runs again with when it runs with what Settings say.
pub(crate) fn newest(snapshot: &Template, cx: &App) -> Result<Template, String> {
    let entry = templates(cx)
        .into_iter()
        .find(|entry| {
            entry
                .template
                .as_ref()
                .is_ok_and(|template| template.same_workflow(snapshot))
        })
        .ok_or_else(|| format!("the workflow `{}` is no longer on offer", snapshot.name))?;
    entry.template
}
