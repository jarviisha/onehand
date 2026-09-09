//! Composition root for every plugin compiled into the Onehand binary.
//!
//! Two explicit, ordered lists and nothing else. The order is the user-visible
//! Workbench order, declared here rather than inherited from filesystem or
//! linker order — which is the one thing a registry was buying, and the only
//! one worth keeping. Everything a registry checked is checked by the compiler
//! instead: `impl WorkbenchMode` *is* the declaration, this is one binary so
//! cargo is the version check, and a list built and returned in one call has no
//! window in which something could register late.

use gpui::{App, Pixels};
use onehand_plugin_api::PluginId;
use onehand_plugin_host::{Ask, RemoteChannelFactory, WorkbenchMode};

/// The Workbench modes this window draws, in the order they sit on the strip.
///
/// One list per window rather than one for the process, because a mode holds
/// entities: a view is bound to the window that renders it, and a second window
/// showing the same one would be the same entity mounted twice.
pub fn workbench_modes(ask: Ask, font_size: Pixels, cx: &mut App) -> Vec<Box<dyn WorkbenchMode>> {
    vec![
        Box::new(onehand_workbench_editor::Mode::new(cx)),
        Box::new(onehand_workbench_files::Mode::new(ask.clone(), cx)),
        Box::new(onehand_workbench_markdown::Mode::new(ask.clone(), cx)),
        Box::new(onehand_workbench_neovim::Mode::new(ask, font_size, cx)),
    ]
}

/// How a remote channel named in the config is opened.
///
/// A function rather than a list because the caller arrives holding an ID and
/// wants a way to connect: there is nothing else about a channel this binary
/// has to say, and a menu of them would be a list nobody reads.
pub fn remote_channel(id: PluginId) -> Option<RemoteChannelFactory> {
    (id == onehand_remote_telegram::CHANNEL_ID).then_some(onehand_remote_telegram::create_channel)
}

#[cfg(test)]
mod tests {
    use super::*;
    use onehand_plugin_api::{TERMINAL_KEY_CONTEXT, WORKBENCH_KEY_CONTEXT, WorkbenchModeSpec};

    /// The two facts the panel reads off a mode rather than working out from
    /// its ID, asserted against the modes that declare them.
    ///
    /// Neovim is the one that hosts a live PTY, so it is the one that takes the
    /// terminal's key context and refuses the rem scale. Asserted here rather
    /// than left to the panel, which no longer knows.
    ///
    /// The strip's *order* has no test any more, and needs none: it was worth
    /// asserting while registration went through a registry that could reorder
    /// it, and it is now the literal order of the list above.
    #[test]
    fn only_a_live_grid_takes_the_terminal_context_and_refuses_the_rem_scale() {
        let declared: Vec<WorkbenchModeSpec> = vec![
            onehand_workbench_editor::SPEC,
            onehand_workbench_files::SPEC,
            onehand_workbench_markdown::SPEC,
            onehand_workbench_neovim::SPEC,
        ];
        for spec in &declared {
            let grid = spec.id == onehand_workbench_neovim::SPEC.id;
            assert_eq!(
                (spec.key_context, spec.rem_zoom),
                if grid {
                    (TERMINAL_KEY_CONTEXT, false)
                } else {
                    (WORKBENCH_KEY_CONTEXT, true)
                },
                "{} declares the wrong pair",
                spec.label
            );
        }
    }

    #[test]
    fn only_the_registered_remote_channel_has_a_factory() {
        assert!(remote_channel(onehand_remote_telegram::CHANNEL_ID).is_some());
        assert!(remote_channel(PluginId::new("remote.discord")).is_none());
    }
}
