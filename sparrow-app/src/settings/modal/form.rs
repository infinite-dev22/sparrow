//! Shared sections and links for settings pages.

use std::sync::Arc;

use floem::{
    IntoView, View,
    reactive::ReadSignal,
    views::{Decorators, label, stack},
};

use crate::{
    config::SparrowConfig,
    widgets::{Divider, modal_focus::ModalFocus},
};

use super::{Page, SettingsItem, catalog, owner, tokens};

pub(super) fn sections(
    items: Vec<SettingsItem>,
    searching: bool,
) -> Vec<(&'static str, Vec<SettingsItem>)> {
    let mut groups: Vec<(&str, Vec<_>)> = Vec::new();
    for item in items {
        let page = owner(&item.kind, &item.field);
        let title = if searching {
            page.title()
        } else {
            catalog::section(page, &item.field)
        };
        if let Some((_, items)) = groups.iter_mut().find(|(name, _)| *name == title)
        {
            items.push(item);
        } else {
            groups.push((title, vec![item]));
        }
    }
    groups
}

pub(super) fn section_title(
    title: &'static str,
    config: ReadSignal<Arc<SparrowConfig>>,
) -> impl View {
    let gap = tokens::length("navigation", "tree_icon_gap");
    stack((
        label(move || title).style(|s| s.font_bold().text_ellipsis().min_width(0)),
        Divider::new(config)
            .into_view()
            .style(|s| s.flex_grow(1.0_f32)),
    ))
    .style(move |s| {
        s.items_center()
            .gap(gap)
            .padding_vert(gap)
            .width_full()
            .apply_if(title.is_empty(), |s| s.hide())
    })
}

pub(super) fn link(
    keyboard: &ModalFocus,
    title: &'static str,
    action: impl Fn() + 'static,
    config: ReadSignal<Arc<SparrowConfig>>,
) -> floem::AnyView {
    keyboard
        .control(crate::widgets::action_link(move || title, action, config))
        .into_any()
}

pub(super) fn children(page: Page) -> Vec<Page> {
    page.children().collect()
}
