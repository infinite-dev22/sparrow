//! Build variant selector and Gradle sync status.

use crate::panel::logcat::State;
use crate::widgets::tooltip::TooltipExt;
use crate::{config::icon::SparrowIcons, widgets::clickable_icon};
use floem::{
    View,
    reactive::{SignalGet, SignalUpdate, create_rw_signal},
    views::{Decorators, dyn_container, label, scroll, stack},
};

#[derive(Clone)]
struct VariantChoice(String);
impl std::fmt::Display for VariantChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.0.is_empty() {
            f.write_str("Follow run configuration")
        } else {
            f.write_str(&self.0)
        }
    }
}
pub(super) fn variant_bar(state: State) -> impl View {
    let config = state.window.common.config;
    let model = state.model;
    let choices_state = state.clone();
    let choices = dyn_container(
        move || model.get().variants,
        move |variants| {
            let current = create_rw_signal(VariantChoice(
                choices_state.variant.get_untracked(),
            ));
            let mut items = vec![VariantChoice(String::new())];
            items.extend(variants.into_iter().map(|v| VariantChoice(v.key())));
            let state = choices_state.clone();
            crate::widgets::dropdown(current, items, config)
                .on_accept(move |choice| {
                    current.set(choice.clone());
                    state.variant.set(choice.0);
                    state.save();
                })
                .style(|s| s.width(280.0).max_width_full().flex_shrink(1.0_f32))
        },
    );
    let choices = choices.style(|s| s.min_width(0.0).flex_shrink(1.0_f32));
    let summary = state.clone();
    let details = state.clone();
    let refresh = state.clone();
    let busy = state.syncing;
    let exists = state
        .window
        .workspace
        .path
        .as_deref()
        .and_then(crate::panel::logcat::gradle::build_root)
        .is_some();
    stack((
        label(|| "Variant:").style(|s| s.flex_shrink(0.0_f32)),
        choices,
        label(move || {
            if summary.syncing.get() {
                return "Syncing Gradle variants…".into();
            }
            let error = summary.gradle_error.get();
            if !error.is_empty() {
                return error.lines().next().unwrap_or("Gradle sync failed").into();
            }
            let packages = summary.packages.get();
            if packages.is_empty() {
                "No matching variant; select a variant or sync Gradle".into()
            } else {
                packages.join(", ")
            }
        })
        .style(|s| s.text_ellipsis().min_width(0.0).flex_grow(1.0_f32))
        .tooltip(move || {
            let error = details.gradle_error.get();
            scroll(label(move || {
                if error.is_empty() {
                    details.packages.get().join(", ")
                } else {
                    error.clone()
                }
            }))
            .style(|s| s.max_width(650.0).max_height(300.0))
        }),
        clickable_icon(
            || SparrowIcons::LOGCAT_REFRESH,
            move || refresh.sync_gradle(true),
            || false,
            move || busy.get(),
            || "Sync Gradle variants",
            config,
        ),
    ))
    .style(move |s| {
        s.width_full()
            .min_width(0.0)
            .height(36.0)
            .items_center()
            .gap(8.0)
            .padding_horiz(10.0)
            .flex_shrink(0.0_f32)
            .apply_if(!exists, |s| s.hide())
    })
}
