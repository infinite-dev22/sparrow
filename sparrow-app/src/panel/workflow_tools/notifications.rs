//! Grouped notification cards and dismissal controls.

use super::{crash_report_button, record};
use crate::panel::kind::PanelKind;
use crate::{
    config::{color::SparrowColor, icon::SparrowIcons},
    widgets::{action_link, clickable_icon, sparrow_scroll},
    window_tab::WindowTabData,
};
use floem::{
    View,
    reactive::{
        SignalGet, SignalUpdate, SignalWith, create_effect, create_memo,
        create_rw_signal,
    },
    views::{Decorators, dyn_stack, empty, label, stack},
};
use std::rc::Rc;

pub fn notifications(window: Rc<WindowTabData>) -> impl View {
    let config = window.common.config;
    let notices = window.notifications;
    let report_window = window.clone();
    let expanded = create_rw_signal(std::collections::HashSet::<u64>::new());
    let open_groups = create_rw_signal(std::collections::HashSet::<String>::new());
    let message = window.flutter_hot_reload.message;
    create_effect(move |last: Option<Option<String>>| {
        let current = message.get();
        if current.is_some() && last.as_ref() != Some(&current) {
            record(notices, "Flutter", current.as_deref().unwrap());
        }
        current
    });
    let groups = create_memo(move |_| {
        let mut groups = Vec::<String>::new();
        for notice in notices.get().iter().rev() {
            if !groups.contains(&notice.title) {
                groups.push(notice.title.clone());
            }
        }
        groups
    });
    stack((
        crate::panel::chrome::header(window,PanelKind::Notifications,"Notifications",empty()),
        stack((label(||"Timeline").style(move |s|s.color(config.get().color(SparrowColor::PANEL_FOREGROUND_DIM))),
            empty().style(|s|s.flex_grow(1.0_f32)),
            action_link(|| "Clear all", move || {
                notices.set(Vec::new());
                expanded.set(Default::default());
                open_groups.set(Default::default());
            }, config)
        )).style(|s|s.height(30.0).items_center().padding_horiz(16.0).width_full()),
        label(||"No notifications").style(move |s|s.padding(24.0).apply_if(!notices.with(|n|n.is_empty()),|s|s.hide())),
        sparrow_scroll(dyn_stack(move ||groups.get(),|title|title.clone(),move |title|{
            let count_title=title.clone();let toggle_title=title.clone();let dismiss_title=title.clone();let rows_title=title.clone();
            let report_window=report_window.clone();
            stack((
                stack((label(move ||{
                    let count=notices.with(|items|items.iter().filter(|n|n.title==count_title).count());
                    format!("{} {}  ({count})",if open_groups.with(|g|g.contains(&count_title)){"⌄"}else{"›"},count_title)
                }).style(|s|s.flex_grow(1.0_f32).font_weight(floem::text::Weight(600))),
                    clickable_icon(||SparrowIcons::CLOSE,move ||notices.update(|items|items.retain(|n|n.title!=dismiss_title)),||false,||false,||"Dismiss group",config)
                )).on_click_stop(move |_|open_groups.update(|groups|{if !groups.remove(&toggle_title){groups.insert(toggle_title.clone());}}))
                  .style(|s|s.height(28.0).width_full().items_center().padding_horiz(6.0)),
                dyn_stack(move ||{
                    let all=open_groups.with(|groups|groups.contains(&rows_title));
                    notices.with(|items|items.iter().rev().filter(|n|n.title==rows_title).take(if all{usize::MAX}else{1}).cloned().collect::<Vec<_>>())
                },|notice|notice.id,move |notice|{
                    let id=notice.id;let body=notice.body.clone();let long=body.chars().count()>360||body.lines().count()>5;
                    let report=notice.crash_report.clone();
                    let has_report=report.is_some();
                    let open_window=report_window.clone();
                    stack((
                        stack((label(move ||notice.time.clone()).style(move |s|s.flex_grow(1.0_f32).font_size(11.0).color(config.get().color(SparrowColor::EDITOR_DIM))),
                            clickable_icon(||SparrowIcons::CLOSE,move ||notices.update(|items|items.retain(|n|n.id!=id)),||false,||false,||"Dismiss notification",config)
                        )).style(|s|s.items_center().width_full()),
                        label(move ||if long&&!expanded.with(|ids|ids.contains(&id)){
                            let text=body.lines().take(5).collect::<Vec<_>>().join("\n");
                            format!("{}…",text.chars().take(360).collect::<String>())
                        }else{body.clone()})
                            .on_click_cont(move |_| {
                                if let Some(report)=&report {
                                    open_window.open_logs(Some(report.clone()));
                                }
                            })
                            .style(move |s|s.width_full().min_width(0.0).margin_top(4.0)
                                .apply_if(has_report,|s|s.cursor(floem::style::CursorStyle::Pointer))),
                        crash_report_button(report_window.clone(), notice.crash_report),
                        action_link(
                            move || if expanded.with(|ids|ids.contains(&id)) {"Show less"} else {"Show more"},
                            move || expanded.update(|ids| { if !ids.remove(&id) { ids.insert(id); } }),
                            config,
                        )
                        .style(move |s| s.align_self(Some(floem::style::AlignItems::FlexStart))
                            .margin_top(6.0).apply_if(!long, |s| s.hide()))
                    )).style(move |s|s.flex_col().width_full().padding(10.0).margin_bottom(6.0).border_radius(5.0)
                        .background(config.get().color(SparrowColor::PANEL_HOVERED_BACKGROUND).with_alpha(0.35)))
                }).style(|s|s.flex_col().width_full()),
            )).style(|s|s.flex_col().width_full().margin_bottom(10.0))
        }).style(|s|s.flex_col().width_full().padding_horiz(10.0))).style(|s|s.flex_grow(1.0_f32).min_height(0.0).width_full())
    )).style(|s|s.absolute().size_full().flex_col().font_size(13.0))
}
