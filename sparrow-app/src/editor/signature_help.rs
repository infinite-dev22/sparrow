//! Editor signature help popup.

use std::rc::Rc;

use crate::{config::color::SparrowColor, window_tab::WindowTabData};
use floem::{
    View,
    event::EventListener,
    peniko::kurbo::Rect,
    reactive::{SignalGet, SignalUpdate, SignalWith, create_rw_signal},
    style::Position,
    views::{Decorators, container, scroll::PropagatePointerWheel},
};

pub(crate) fn signature_help(window_tab_data: Rc<WindowTabData>) -> impl View {
    use floem::text::{Attrs, AttrsList, FamilyOwned, TextLayout, Weight};
    use lsp_types::{ParameterInformation, ParameterLabel};

    let sig = window_tab_data.common.signature_help;
    let config = window_tab_data.common.config;
    let layout_rect = create_rw_signal(Rect::ZERO);

    // Helper: byte range of the active param within the label.
    fn active_range(
        label: &str,
        params: Option<&[ParameterInformation]>,
        active: Option<u32>,
    ) -> Option<(usize, usize)> {
        let idx = active? as usize;
        let params = params?;
        let param = params.get(idx)?;
        match &param.label {
            ParameterLabel::Simple(s) => {
                let start = label.find(s)?;
                Some((start, start + s.len()))
            }
            ParameterLabel::LabelOffsets(offsets) => {
                let [s, e] = *offsets;
                Some((
                    utf16_to_byte(label, s as usize),
                    utf16_to_byte(label, e as usize),
                ))
            }
        }
    }

    fn utf16_to_byte(s: &str, u16_off: usize) -> usize {
        let mut count = 0usize;
        for (byte_idx, ch) in s.char_indices() {
            if count >= u16_off {
                return byte_idx;
            }
            count += ch.len_utf16();
        }
        s.len()
    }

    let rich = floem::views::rich_text(move || {
        let sh = sig.get();
        let cfg = config.get();
        let Some(signature) = sh.current_signature() else {
            return TextLayout::new();
        };
        let family: Vec<FamilyOwned> =
            FamilyOwned::parse_list(&cfg.ui.font_family).collect();
        let base_attrs = Attrs::new()
            .family(&family)
            .font_size(cfg.ui.font_size() as f32)
            .color(cfg.color(SparrowColor::EDITOR_FOREGROUND));
        let mut attrs_list = AttrsList::new(base_attrs.clone());

        if let Some((start, end)) = active_range(
            &signature.label,
            signature.parameters.as_deref(),
            sh.active_parameter,
        ) {
            let active_attrs = base_attrs
                .color(cfg.color(SparrowColor::EDITOR_FOCUS))
                .weight(Weight::BOLD);
            attrs_list.add_span(start..end, active_attrs);
        }

        let mut layout = TextLayout::new();
        layout.set_text(&signature.label, attrs_list, None);
        layout
    });

    floem::views::stack((container(rich).style(move |s| {
        let cfg = config.get();
        s.padding_horiz(10.0)
            .padding_vert(6.0)
            .border(1.0)
            .border_radius(6.0)
            .border_color(cfg.color(SparrowColor::SPARROW_BORDER))
            .background(cfg.color(SparrowColor::PANEL_BACKGROUND))
            .box_shadow_blur(4.0)
            .box_shadow_color(cfg.color(SparrowColor::SPARROW_DROPDOWN_SHADOW))
            .max_width(600.0)
            .set(PropagatePointerWheel, false)
    }),))
    .on_resize(move |rect| {
        layout_rect.set(rect);
    })
    .on_event_stop(EventListener::PointerMove, |_| {})
    .on_event_stop(EventListener::PointerDown, |_| {})
    .style(move |s| {
        if !sig.with(|s| s.is_active()) {
            return s.hide();
        }
        match window_tab_data.signature_help_origin() {
            Some(origin) => s
                .position(Position::Absolute)
                .margin_left(origin.x as f32)
                .margin_top(origin.y as f32),
            None => s.hide(),
        }
    })
    .debug_name("Signature Help Layer")
}
