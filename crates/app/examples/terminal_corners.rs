//! Visual comparison of the old quadratic corners and the shared quad renderer.
//! Each pair is old (left), new (right). Groups exercise plain backgrounds,
//! alternating cell backgrounds, translucent strokes and an outer content mask.
use gpui::{
    App, AppContext, Bounds, ContentMask, Context, Hsla, IntoElement, ParentElement, PathBuilder,
    Pixels, Point, Render, Styled, Window, canvas, div, fill, point, px, rgb, size,
};
use gpui_terminal::box_drawing::draw_box_character;

struct Gallery;

impl Render for Gallery {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        eprintln!("corner_gallery scale_factor={}", window.scale_factor());
        div()
            .size_full()
            .bg(rgb(0x202530))
            .text_color(rgb(0xd8dee9))
            .p_4()
            .child("Old / new pairs: plain | cell backgrounds | alpha 0.5 | clipped")
            .children(
                [(8.4, 16.), (10.8, 21.), (14.4, 28.), (21.6, 42.)].map(|(width, height)| {
                    div()
                        .child(format!("Cell {width} x {height} logical pixels"))
                        .child(
                            canvas(
                                |_, _, _| (),
                                move |bounds, _, window, _| {
                                    for group in 0..4 {
                                        for reference in [true, false] {
                                            let origin = bounds.origin
                                                + point(
                                                    px(group as f32 * 250.
                                                        + if reference { 0. } else { 125. }
                                                        + 0.37),
                                                    px(4.625),
                                                );
                                            let cell = size(px(width), px(height));
                                            let panel = Bounds::new(
                                                origin,
                                                size(cell.width * 5., cell.height * 3.),
                                            );
                                            let clip = (group == 3).then_some(ContentMask {
                                                bounds: Bounds::new(
                                                    origin
                                                        + point(
                                                            cell.width * 0.65,
                                                            cell.height * 0.55,
                                                        ),
                                                    size(cell.width * 3.95, cell.height * 1.9),
                                                ),
                                            });
                                            window.paint_quad(fill(panel, rgb(0x202530)));
                                            window.with_content_mask(clip, |window| {
                                                for (row, line) in
                                                    ["╭───╮", "│   │", "╰───╯"].iter().enumerate()
                                                {
                                                    for (col, ch) in line.chars().enumerate() {
                                                        let bounds = Bounds::new(
                                                            origin
                                                                + point(
                                                                    cell.width * col as f32,
                                                                    cell.height * row as f32,
                                                                ),
                                                            cell,
                                                        );
                                                        if group == 1 {
                                                            window.paint_quad(fill(
                                                                bounds,
                                                                rgb(if (row + col) % 2 == 0 {
                                                                    0x355d83
                                                                } else {
                                                                    0x704839
                                                                }),
                                                            ));
                                                        }
                                                        let mut color: Hsla = rgb(0xd8dee9).into();
                                                        if group == 2 {
                                                            color.a = 0.5;
                                                        }
                                                        if reference && "╭╮╯╰".contains(ch)
                                                        {
                                                            old_corner(ch, bounds, color, window);
                                                        } else {
                                                            draw_box_character(
                                                                ch, bounds, color, cell.width,
                                                                window,
                                                            );
                                                        }
                                                    }
                                                }
                                            });
                                        }
                                    }
                                },
                            )
                            .w_full()
                            .h(px(height * 3. + 14.)),
                        )
                }),
            )
    }
}

// The former stroke geometry, retained only in this comparison example.
fn old_corner(ch: char, bounds: Bounds<Pixels>, color: Hsla, window: &mut Window) {
    let center = bounds.center();
    let direction = point(
        if matches!(ch, '╭' | '╰') { 1. } else { -1. },
        if matches!(ch, '╭' | '╮') { 1. } else { -1. },
    );
    let at = |x: Pixels, y: Pixels| -> Point<Pixels> {
        center + point(x * direction.x, y * direction.y)
    };
    let thickness = px((f32::from(bounds.size.width) * 0.15).round().max(1.));
    let mut path = PathBuilder::stroke(thickness);
    path.move_to(at(px(0.), bounds.size.height / 2. + px(1.)));
    path.line_to(at(px(0.), bounds.size.height * 0.4));
    path.curve_to(at(bounds.size.width * 0.4, px(0.)), center);
    path.line_to(at(bounds.size.width / 2. + px(1.), px(0.)));
    window.paint_path(path.build().unwrap(), color);
}

fn main() {
    gpui_platform::application()
        .with_assets(onehand::assets::Assets)
        .run(|cx: &mut App| {
            gpui_component::init(cx);
            cx.open_window(
                gpui::WindowOptions {
                    app_id: Some("onehand-terminal-corners".into()),
                    window_bounds: Some(gpui::WindowBounds::Windowed(Bounds::new(
                        point(px(0.), px(0.)),
                        size(px(1040.), px(650.)),
                    ))),
                    ..Default::default()
                },
                |_, cx| cx.new(|_| Gallery),
            )
            .unwrap();
            cx.activate(true);
        });
}
