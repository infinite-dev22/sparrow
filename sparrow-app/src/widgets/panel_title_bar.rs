use floem::{
    View,
    text::Weight,
    views::{Decorators, label, stack},
};

/// Keeps the panel title and optional content beside a fixed group of actions.
/// The content can shrink or scroll within the remaining space.
pub fn panel_title_bar(
    title: &'static str,
    content: impl View + 'static,
    actions: impl View + 'static,
) -> impl View {
    stack((
        stack((
            label(move || title).style(|s| {
                s.font_weight(Weight(600))
                    .min_width(0.0)
                    .text_ellipsis()
                    .selectable(false)
            }),
            content.style(|s| s.min_width(0.0)),
        ))
        .style(|s| s.items_center().flex_grow(1.0_f32).min_width(0.0).gap(12.0)),
        actions.style(|s| s.flex_shrink(0.0_f32)),
    ))
    .style(|s| {
        s.height(32.0)
            .width_full()
            .min_width(0.0)
            .items_center()
            .padding_horiz(12.0)
            .gap(12.0)
            .flex_shrink(0.0_f32)
    })
}
