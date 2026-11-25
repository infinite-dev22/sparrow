use std::sync::Arc;
use floem::{event::EventListener, peniko::Color, reactive::{RwSignal, ReadSignal, SignalGet, SignalUpdate}, style::{AlignItems, CursorStyle, Position}, views::{container, h_stack, label, Decorators}, IntoView, View};
use floem::views::empty;
use crate::config::{SparrowConfig, color::SparrowColor};

pub use super::editor_input::{TextInput, TextInputBuilder};

/// Compact, themed single-line field for forms, filters and dialogs.
///
/// All field appearance lives here. Callers only supply content and layout.
/// Floem handles editing, selection, IME and clipboard behavior.
///
/// ```ignore
/// text_input(name, config).placeholder("Project name")
/// ```

pub fn text_input(
    value: RwSignal<String>,
    config: ReadSignal<Arc<SparrowConfig>>,
) -> floem::views::TextInput {
    let input = floem::views::text_input(value);
    let id = input.id();
    input
        .on_click_stop(move |_| id.request_focus())
        .style(move |s| {
            let config = config.get();
            let foreground = config.color(SparrowColor::EDITOR_FOREGROUND);
            let accent = config.color(SparrowColor::EDITOR_CARET);
            s.width_full()
                .min_width(0.0)
                .height(28.0)
                .items_center()
                .padding_horiz(6.0)
                .padding_vert(0.0)
                .color(foreground)
                .background(Color::TRANSPARENT)
                .border(1.0)
                .border_color(config.color(SparrowColor::SPARROW_BORDER))
                .border_radius(4.0)
                .cursor(CursorStyle::Text)
                .hover(move |s| s.border_color(foreground.multiply_alpha(0.5)))
                .focus(move |s| s.border_color(accent))
        })
}

pub fn m3_text_input(
    value: RwSignal<String>,
    floating_label: impl Into<String> + Clone + 'static,
    prefix: Option<impl Into<String> + Clone + 'static>,
    config: ReadSignal<Arc<SparrowConfig>>,
) -> impl IntoView {
    let is_focused = RwSignal::new(false);
    let label_text = floating_label.into();

    // 1. Create the inner input and grab its ViewId
    let input = floem::views::text_input(value);
    let id = input.id();

    let styled_input = input
        .on_event_cont(EventListener::FocusGained, move |_| is_focused.set(true))
        .on_event_cont(EventListener::FocusLost, move |_| is_focused.set(false))
        .style(move |s| {
            let config = config.get();
            s.width_full()
                .min_width(0.0)
                .height(24.0)
                .padding(0.0)
                .border(0.0)
                .color(config.color(SparrowColor::EDITOR_FOREGROUND))
                .background(Color::TRANSPARENT)
                .cursor(CursorStyle::Text)
        });

    // 2. Build the inner row (Optional Prefix + TextInput)
    let prefix_view = match prefix {
        Some(p) => {
            let p_str: String = p.into();
            label(move || p_str.clone())
                .style(move |s| {
                    let config = config.get();
                    s.color(config.color(SparrowColor::EDITOR_FOREGROUND).multiply_alpha(0.7))
                        .padding_right(6.0)
                        .cursor(CursorStyle::Text)
                })
                .into_any()
        }
        None => empty().into_any(),
    };

    let inner_row = h_stack((prefix_view, styled_input))
        .style(|s| s.width_full().items_center());

    // 3. M3 Floating Label (sits on the top border when focused or non-empty)
    // 3. M3 Floating Label (sits on the top border when focused or non-empty)
    let floating_label_view = label(move || label_text.clone()).style(move |s| {
        let config = config.get();
        let focused = is_focused.get();
        let has_text = !value.get().is_empty();
        let active = focused || has_text;

        let fg = config.color(SparrowColor::EDITOR_FOREGROUND);
        let accent = config.color(SparrowColor::EDITOR_CARET);
        let bg = config.color(SparrowColor::EDITOR_BACKGROUND);

        s.position(Position::Absolute)
            .inset_left(12.0)
            .padding_horiz(4.0)
            .background(bg)
            .border_radius(2.0)
            .cursor(CursorStyle::Text)
            .apply_if(active, |s| {
                s.inset_top(-8.0)
                    .font_size(11.0)
                    .color(if focused { accent } else { fg.multiply_alpha(0.7) })
            })
            .apply_if(!active, |s| {
                s.inset_top(15.0)
                    .font_size(14.0)
                    .color(fg.multiply_alpha(0.6))
            })
    });

    // 4. Outer M3 Container (48px height, 4px radius, 1px -> 2px focus border)
    container((inner_row, floating_label_view))
        .on_click_stop(move |_| id.request_focus())
        .style(move |s| {
            let config = config.get();
            let focused = is_focused.get();
            let fg = config.color(SparrowColor::EDITOR_FOREGROUND);
            let accent = config.color(SparrowColor::EDITOR_CARET);
            let border = config.color(SparrowColor::SPARROW_BORDER);

            s.width_full()
                .min_width(0.0)
                .height(48.0) // Standard M3 touch-target height
                .padding_horiz(14.0)
                .items_center()
                .border_radius(4.0)
                .cursor(CursorStyle::Text)
                .border(if focused { 2.0 } else { 1.0 })
                .border_color(if focused { accent } else { border })
                .hover(move |s| {
                    if !focused {
                        s.border_color(fg.multiply_alpha(0.8))
                    } else {
                        s
                    }
                })
        })
}
