//! The *Composer cards* page: every card and popup on the composer as a
//! numbered specimen, at the stack's real width, on the reading surface.
use super::composer::{ComposerLook, item};
use super::*;
use gpui::{AnyElement, Pixels};

impl Labs {
    fn specimen(
        p: &Palette,
        radius: Pixels,
        n: usize,
        name: &'static str,
        note: &'static str,
        stack: impl IntoElement,
    ) -> impl IntoElement {
        v_flex()
            .gap_2()
            .child(
                h_flex()
                    .gap_2()
                    .child(div().text_xs().text_color(p.muted).child(format!("{n:02}")))
                    .child(div().font_medium().text_color(p.text).child(name))
                    .child(
                        div()
                            .flex_1()
                            .truncate()
                            .text_xs()
                            .text_color(p.muted)
                            .child(note),
                    ),
            )
            .child(
                v_flex()
                    .p_4()
                    .rounded(radius)
                    .border_1()
                    .border_color(p.hairline)
                    .bg(p.page)
                    .child(
                        v_flex()
                            .w_full()
                            .max_w(rems(COMPOSER_MAX))
                            .mx_auto()
                            .gap_2p5()
                            .child(stack),
                    ),
            )
    }

    pub(super) fn composer_gallery(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let radius = cx.theme().radius_lg;
        let mono = cx.theme().mono_font_family.clone();
        let idle =
            |cx: &App| Self::composer_card(p, cx, ComposerLook::default()).into_any_element();
        let typed = |cx: &App, chip: Option<&'static str>, text: &'static str| {
            Self::composer_card(
                p,
                cx,
                ComposerLook {
                    text: Some(text),
                    open_chip: chip,
                    ..Default::default()
                },
            )
            .into_any_element()
        };
        let stack = || v_flex().w_full().gap_2p5();
        let mut n = 0;
        let mut next = || {
            n += 1;
            n
        };

        let files = vec![
            item("src/backoff.rs", "modified")
                .mono()
                .icon(IconName::File)
                .group("Changed"),
            item("tests/flaky.rs", "modified")
                .mono()
                .icon(IconName::File),
            item("src/retry.rs", "")
                .mono()
                .icon(IconName::File)
                .group("Files"),
            item("src/retry/policy.rs", "").mono().icon(IconName::File),
            item("benches/retry.rs", "").mono().icon(IconName::File),
            item("docs/retry.md", "").mono().icon(IconName::File),
            item("README.md", "").mono().icon(IconName::File),
            item("Cargo.toml", "").mono().icon(IconName::File),
        ];
        let commands = vec![
            item("/review", "Review the working tree")
                .mono()
                .group("Agent"),
            item("/compact", "Summarise the conversation so far").mono(),
            item("/init", "Write a CLAUDE.md for this project").mono(),
            item("/run-check", "Run the project's check command")
                .mono()
                .group("onehand"),
        ];
        let settings = vec![
            item("Sonnet 5", "fast, most tasks")
                .current()
                .group("Model"),
            item("Opus 5.5", "slower, hardest tasks"),
            item("Haiku 4.5", "quickest"),
            item("Low", "").group("Effort"),
            item("High", "").current(),
        ];
        let plus = vec![
            item("Attach files…", "").icon(IconName::Plus),
            item("Mention a file", "@").icon(IconName::File),
            item("Run a command", "/").icon(IconName::SquareTerminal),
            item("Run a workflow…", "").icon(IconName::GalleryVerticalEnd),
        ];
        let modes = vec![
            item("Ask before edits", "every edit and command asks").current(),
            item("Accept edits", "commands still ask"),
            item("Plan only", "reads, never writes"),
            item("Bypass permissions", "nothing asks"),
        ];
        let branches = vec![
            item("main", "3 changes").mono().current().group("Branches"),
            item("fix/retry-clock", "2 ahead").mono(),
            item("feat/charts", "").mono(),
            item("New branch…", "").icon(IconName::Plus),
        ];

        let left = |popup: AnyElement| h_flex().child(popup);
        let right = |popup: AnyElement| h_flex().justify_end().child(popup);

        Self::column("composer-gallery").child(
            Self::inner(p)
                .max_w(rems(GALLERY_MAX))
                .child(div().text_color(p.text2).child(
                    "Everything that rests on the composer. Pinned cards stack in the order they were asked; popups open above the input and float over the transcript.",
                ))
                .child(div().text_sm().font_medium().child("Pinned above the composer"))
                .child(Self::specimen(p, radius, next(), "Permission · command", "a long command is capped in its well", stack().child(Self::permission_command(p, cx, mono.clone())).child(idle(cx))))
                .child(Self::specimen(p, radius, next(), "Permission · edit", "the diff keeps its signs, not just colour", stack().child(Self::permission_edit(p, cx, mono.clone())).child(idle(cx))))
                .child(Self::specimen(p, radius, next(), "Question · one choice", "click a row; tabs for a form with several fields", stack().child(self.question_single(p, cx)).child(idle(cx))))
                .child(Self::specimen(p, radius, next(), "Question · any choices", "click to toggle; Submit counts", stack().child(self.question_multi(p, cx)).child(idle(cx))))
                .child(Self::specimen(p, radius, next(), "Question · free text", "the description once, a short placeholder", stack().child(Self::question_text(p, cx)).child(idle(cx))))
                .child(Self::specimen(
                    p, radius,
                    next(),
                    "Queued prompt while a turn runs",
                    "Stop stays; Queue joins it over a draft",
                    stack()
                        .child(Self::queued(p))
                        .child(Self::composer_card(p, cx, ComposerLook { text: Some("Also bump the crate version"), running: true, ..Default::default() })),
                ))
                .child(Self::specimen(p, radius, next(), "Reconnecting", "the transcript stays; sending waits", stack().child(Self::connecting(p)).child(idle(cx))))
                .child(Self::specimen(
                    p, radius,
                    next(),
                    "Stacked",
                    "permission, then question, then the queue, then the composer",
                    stack()
                        .child(Self::permission_command(p, cx, mono.clone()))
                        .child(Self::question_text(p, cx))
                        .child(Self::queued(p))
                        .child(Self::composer_card(p, cx, ComposerLook { running: true, ..Default::default() })),
                ))
                .child(div().text_sm().font_medium().child("Opening from the composer"))
                .child(Self::specimen(p, radius, next(), "+ menu", "the way to @ and / when an IME swallows them", stack().child(left(Self::popup(cx, p, mono.clone(), Some(MENU_W), "Add to the prompt", plus, 0, None, None).into_any_element())).child(Self::composer_card(p, cx, ComposerLook { open_chip: Some("plus"), ..Default::default() }))))
                .child(Self::specimen(p, radius, next(), "@ mention", "changed files first; capped, says how many are left", stack().child(Self::popup(cx, p, mono.clone(), None, "Mention a file", files, 0, Self::nav_keys(), None)).child(typed(cx, None, "Look at @"))))
                .child(Self::specimen(p, radius, next(), "/ command", "the agent's commands, then onehand's", stack().child(Self::popup(cx, p, mono.clone(), None, "Run a command", commands, 1, Self::nav_keys(), None)).child(typed(cx, None, "/c"))))
                .child(Self::specimen(p, radius, next(), "@ with no match", "says what it looked for", stack().child(Self::popup(cx, p, mono.clone(), None, "Mention a file", vec![], 0, None, Some("No matches for \u{201c}flakey\u{201d}".into()))).child(typed(cx, None, "Look at @flakey"))))
                .child(Self::specimen(p, radius, next(), "Model and effort", "one popup, two groups, the current one checked", stack().child(left(Self::popup(cx, p, mono.clone(), Some(MENU_WIDE_W), "Settings", settings, 0, None, None).into_any_element())).child(Self::composer_card(p, cx, ComposerLook { open_chip: Some("model"), ..Default::default() }))))
                .child(Self::specimen(p, radius, next(), "Permission mode", "opens from the strip, right-aligned to it", stack().child(right(Self::popup(cx, p, mono.clone(), Some(MENU_WIDE_W), "Mode", modes, 0, None, None).into_any_element())).child(Self::composer_card(p, cx, ComposerLook { open_chip: Some("mode"), ..Default::default() }))))
                .child(Self::specimen(p, radius, next(), "Branch", "opens from the strip, left-aligned to it", stack().child(left(Self::popup(cx, p, mono.clone(), Some(MENU_W), "Branch", branches, 0, None, None).into_any_element())).child(Self::composer_card(p, cx, ComposerLook { open_chip: Some("branch"), ..Default::default() }))))
                .child(div().text_sm().font_medium().child("The composer itself"))
                .child(Self::specimen(p, radius, next(), "Empty", "Send is spent until there is something to send", idle(cx)))
                .child(Self::specimen(p, radius, next(), "Attachments", "a tray inside the card, each removable", Self::composer_card(p, cx, ComposerLook { text: Some("Why does the CI log show a timeout?"), tray: true, ..Default::default() })))
                .child(Self::specimen(p, radius, next(), "Running, nothing typed", "Stop alone", Self::composer_card(p, cx, ComposerLook { running: true, ..Default::default() }))),
        )
    }
}
