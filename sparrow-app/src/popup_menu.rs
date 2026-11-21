//! Themed, in-window menus shared by the editor, tool windows and toolbar.
use crate::{config::color::SparrowColor, window_tab::CommonData};
use floem::{
    IntoView, View, ViewId,
    action::{add_overlay, remove_overlay},
    event::{Event, EventListener, EventPropagation},
    keyboard::{Key, NamedKey},
    kurbo::Point,
    reactive::{RwSignal, SignalGet, SignalUpdate, SignalWith, create_rw_signal},
    views::{Decorators, dyn_stack, label, scroll, stack},
};
use std::{
    cell::RefCell,
    rc::{Rc, Weak},
};

#[derive(Clone)]
pub enum MenuEntry {
    Separator,
    Item(MenuItem),
    SubMenu(Menu),
}
#[derive(Clone)]
pub struct Menu {
    title: String,
    entries: Vec<MenuEntry>,
}
#[derive(Clone)]
pub struct MenuItem {
    title: String,
    enabled: bool,
    action: Option<Rc<dyn Fn()>>,
}
impl From<MenuItem> for MenuEntry {
    fn from(item: MenuItem) -> Self {
        Self::Item(item)
    }
}
impl From<Menu> for MenuEntry {
    fn from(menu: Menu) -> Self {
        Self::SubMenu(menu)
    }
}
impl Menu {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            entries: Vec::new(),
        }
    }
    pub fn entry(mut self, entry: impl Into<MenuEntry>) -> Self {
        self.entries.push(entry.into());
        self
    }
    pub fn separator(self) -> Self {
        self.entry(MenuEntry::Separator)
    }
    pub fn into_native(self) -> floem::menu::Menu {
        let mut menu = floem::menu::Menu::new(self.title);
        for entry in self.entries {
            menu = match entry {
                MenuEntry::Separator => menu.separator(),
                MenuEntry::SubMenu(sub) => menu.entry(sub.into_native()),
                MenuEntry::Item(item) => {
                    let mut native =
                        floem::menu::MenuItem::new(item.title).enabled(item.enabled);
                    if let Some(action) = item.action {
                        native = native.action(move || action());
                    }
                    menu.entry(native)
                }
            };
        }
        menu
    }
}
impl MenuItem {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            enabled: true,
            action: None,
        }
    }
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
    pub fn action(mut self, action: impl Fn() + 'static) -> Self {
        self.action = Some(Rc::new(action));
        self
    }
}
impl MenuEntry {
    fn enabled(&self) -> bool {
        match self {
            Self::Separator => false,
            Self::Item(item) => item.enabled,
            Self::SubMenu(menu) => !menu.entries.is_empty(),
        }
    }
    fn title(&self) -> &str {
        match self {
            Self::Separator => "",
            Self::Item(item) => &item.title,
            Self::SubMenu(menu) => &menu.title,
        }
    }
}
thread_local! {
    static HOST: RefCell<Weak<CommonData>> = RefCell::new(Weak::new());
    static POINTER: RefCell<Point> = const {RefCell::new(Point::ZERO)};
    static OVERLAY: RefCell<Option<ViewId>> = const {RefCell::new(None)};
}
pub fn register(common: Rc<CommonData>) {
    HOST.with(|host| *host.borrow_mut() = Rc::downgrade(&common));
}
pub fn pointer(point: Point) {
    POINTER.with(|last| *last.borrow_mut() = point);
}
fn dismiss(id: ViewId, restore: Option<ViewId>) {
    remove_overlay(id);
    OVERLAY.with(|overlay| {
        if *overlay.borrow() == Some(id) {
            *overlay.borrow_mut() = None;
        }
    });
    if let Some(restore) = restore {
        restore.request_focus();
    }
}
fn first_enabled(menu: &Menu) -> usize {
    menu.entries
        .iter()
        .position(MenuEntry::enabled)
        .unwrap_or(0)
}
fn move_selection(menu: &Menu, selected: usize, delta: isize) -> usize {
    let len = menu.entries.len();
    for step in 1..=len {
        let next = (selected as isize + delta * step as isize)
            .rem_euclid(len as isize) as usize;
        if menu.entries[next].enabled() {
            return next;
        }
    }
    selected
}
fn enter(
    levels: RwSignal<Vec<Menu>>,
    selections: RwSignal<Vec<usize>>,
    depth: usize,
    index: usize,
    id: ViewId,
    restore: Option<ViewId>,
    run: bool,
) {
    let entry = levels.with_untracked(|menus| {
        menus
            .get(depth)
            .and_then(|menu| menu.entries.get(index))
            .cloned()
    });
    let Some(entry) = entry else { return };
    if !entry.enabled() {
        return;
    }
    levels.update(|menus| menus.truncate(depth + 1));
    selections.update(|selected| {
        selected.truncate(depth + 1);
        if let Some(value) = selected.get_mut(depth) {
            *value = index;
        }
    });
    match entry {
        MenuEntry::SubMenu(menu) => {
            let first = first_enabled(&menu);
            levels.update(|menus| menus.push(menu));
            selections.update(|selected| selected.push(first));
        }
        MenuEntry::Item(item) if run => {
            dismiss(id, restore);
            if let Some(action) = item.action {
                action();
            }
        }
        _ => {}
    }
}
pub fn show_context_menu(menu: Menu, position: Option<Point>) {
    show(menu, position, None);
}
fn show(menu: Menu, position: Option<Point>, anchor: Option<ViewId>) {
    let Some(common) = HOST.with(|host| host.borrow().upgrade()) else {
        return;
    };
    OVERLAY.with(|current| {
        if let Some(id) = current.borrow_mut().take() {
            remove_overlay(id);
        }
    });
    let restore = common
        .keyboard_focus
        .get_untracked()
        .or(anchor)
        .or(Some(common.view_id.get_untracked()));
    let pos = position.unwrap_or_else(|| POINTER.with(|last| *last.borrow()));
    let config = common.config;
    let size = common.window_common.size;
    let levels = create_rw_signal(vec![menu.clone()]);
    let selections = create_rw_signal(vec![first_enabled(&menu)]);
    let id = add_overlay(Point::ZERO, move |overlay| {
        let columns=dyn_stack(move ||levels.get().into_iter().enumerate().collect::<Vec<_>>(),|(depth,menu)|(*depth,menu.title.clone()),move |(depth,menu)|{
            let width=(menu.entries.iter().map(|item|item.title().chars().count()).max().unwrap_or(12) as f64*7.0+56.0).clamp(220.0,440.0);
            let row_count=menu.entries.len();
            let rows=dyn_stack(move ||menu.entries.clone().into_iter().enumerate().collect::<Vec<_>>(),|(index,_)|*index,move |(index,entry)| {
                if matches!(entry, MenuEntry::Separator) {
                    return crate::divider::Divider::new(config)
                        .spacing(4.0)
                        .indent(8.0)
                        .into_view()
                        .into_any();
                }
                let enabled=entry.enabled();let title=entry.title().to_string();let child=matches!(entry,MenuEntry::SubMenu(_));
                stack((label(move ||title.clone()).style(|s|s.flex_grow(1.0_f32)),label(move ||if child {"›"}else{""}).style(|s|s.margin_left(24.0))))
                    .on_event_stop(EventListener::PointerEnter,move |_|{if enabled{enter(levels,selections,depth,index,overlay,restore,false);}})
                    .on_click_stop(move |_|enter(levels,selections,depth,index,overlay,restore,true))
                    .style(move |s|s.width_full().height(26.0).padding_horiz(10.0).items_center().border_radius(3.0)
                        .color(config.get().color(if enabled {SparrowColor::EDITOR_FOREGROUND}else{SparrowColor::EDITOR_DIM}))
                        .apply_if(enabled&&selections.with(|selected|selected.get(depth)==Some(&index)),|s|s.background(config.get().color(SparrowColor::SPARROW_TAB_ACTIVE_BACKGROUND))))
                    .into_any()
            }).style(|s|s.flex_col().width_full());
            let bounds=size.get_untracked();let height=(row_count as f64*26.0+8.0).min((bounds.height-24.0).max(80.0));
            let placement=move || {
                let menus=levels.get();let selected=selections.get();
                let mut x=pos.x;let mut y=pos.y;
                for (level, parent) in menus.iter().take(depth).enumerate() {
                    let parent_width=(parent.entries.iter().map(|entry| entry.title().chars().count()).max().unwrap_or(12) as f64*7.0+56.0).clamp(220.0,440.0);
                    x=x.min((bounds.width-parent_width).max(0.0)).max(0.0);
                    let next=x+parent_width-2.0;
                    x=if next+width <= bounds.width { next } else { (x-width+2.0).max(0.0) };
                    y+=parent.entries.iter().take(selected.get(level).copied().unwrap_or(0)).map(|entry| if matches!(entry, MenuEntry::Separator) {9.0}else{26.0}).sum::<f64>();
                }
                (x.min((bounds.width-width).max(0.0)).max(0.0), y.min((bounds.height-height).max(0.0)).max(0.0))
            };
            scroll(rows).ensure_visible(move ||{let selected=selections.with(|s|s.get(depth).copied().unwrap_or(0));floem::kurbo::Rect::new(0.0,selected as f64*26.0,1.0,(selected+1)as f64*26.0)})
                .on_event_stop(EventListener::PointerDown,|_|{})
                .style(move |s|s.absolute().inset_left(placement().0).inset_top(placement().1).width(width).max_height(height).padding(4.0).border(1.0).border_radius(5.0)
                    .border_color(config.get().color(SparrowColor::SPARROW_BORDER)).background(config.get().color(SparrowColor::PANEL_BACKGROUND)).box_shadow_blur(12.0)
                    .font_size(13.0).font_family(config.get().ui.font_family.clone()))
        }).style(|s|s.absolute().size_full());
        let view = stack((columns,))
            .keyboard_navigable()
            .on_event_stop(EventListener::PointerDown, move |_| {
                dismiss(overlay, restore)
            })
            .on_event(EventListener::KeyDown, move |event| {
                let Event::KeyDown(key) = event else {
                    return EventPropagation::Continue;
                };
                let depth =
                    levels.with_untracked(|menus| menus.len().saturating_sub(1));
                let selected = selections
                    .with_untracked(|s| s.get(depth).copied().unwrap_or(0));
                match &key.key.logical_key {
                    Key::Named(NamedKey::Escape) => {crate::modal_focus::note_popup_escape();dismiss(overlay, restore)},
                    Key::Named(NamedKey::Tab) => dismiss(overlay, restore),
                    Key::Named(NamedKey::ArrowLeft) if depth > 0 => {
                        levels.update(|menus| {
                            menus.pop();
                        });
                        selections.update(|s| {
                            s.pop();
                        });
                    }
                    Key::Named(NamedKey::ArrowDown | NamedKey::ArrowUp) => {
                        let delta = if key.key.logical_key
                            == Key::Named(NamedKey::ArrowDown)
                        {
                            1
                        } else {
                            -1
                        };
                        let next = levels.with_untracked(|menus| {
                            move_selection(&menus[depth], selected, delta)
                        });
                        selections.update(|s| s[depth] = next);
                    }
                    Key::Named(NamedKey::Home | NamedKey::End) => {
                        let next = levels.with_untracked(|menus| {
                            if key.key.logical_key == Key::Named(NamedKey::Home) {
                                first_enabled(&menus[depth])
                            } else {
                                menus[depth]
                                    .entries
                                    .iter()
                                    .rposition(MenuEntry::enabled)
                                    .unwrap_or(0)
                            }
                        });
                        selections.update(|s| s[depth] = next);
                    }
                    Key::Named(NamedKey::Enter | NamedKey::Space) => enter(
                        levels, selections, depth, selected, overlay, restore, true,
                    ),
                    Key::Named(NamedKey::ArrowRight) => enter(
                        levels, selections, depth, selected, overlay, restore, false,
                    ),
                    Key::Character(text) => {
                        let needle = text.to_lowercase();
                        let next = levels.with_untracked(|menus| {
                            menus[depth]
                                .entries
                                .iter()
                                .enumerate()
                                .cycle()
                                .skip(selected + 1)
                                .take(menus[depth].entries.len())
                                .find(|(_, entry)| {
                                    entry.enabled()
                                        && entry
                                            .title()
                                            .to_lowercase()
                                            .starts_with(&needle)
                                })
                                .map(|(index, _)| index)
                        });
                        if let Some(next) = next {
                            selections.update(|s| s[depth] = next);
                        }
                    }
                    _ => {}
                }
                EventPropagation::Stop
            })
            .on_event_stop(EventListener::WindowLostFocus, move |_| {
                // Do not steal focus back from the newly activated window.
                dismiss(overlay, None);
            })
            .style(move |s| {
                // Floem's overlay wrapper is auto-sized. Percentage dimensions
                // collapse its absolute child to zero, leaving only the menus
                // hit-testable. Give the backdrop real window dimensions so it
                // catches outside clicks, including after a window resize.
                let bounds = size.get();
                s.absolute().size(bounds.width, bounds.height).z_index(1000)
            });
        let focus = view.id();
        focus.request_focus();
        view
    });
    OVERLAY.with(|overlay| *overlay.borrow_mut() = Some(id));
}
pub trait PopupMenuExt: View + Sized + 'static {
    fn ide_popout_menu(self, menu: impl Fn() -> Menu + 'static) -> impl View {
        let id = self.id();
        let menu = Rc::new(menu);
        let keyboard = menu.clone();
        self.keyboard_navigable()
            .on_event(EventListener::PointerDown, move |event| {
                if let Event::PointerDown(pointer) = event {
                    if pointer.button.is_primary() {
                        let bounds = id.layout_rect();
                        show(
                            menu(),
                            Some(Point::new(bounds.x0, bounds.y1)),
                            Some(id),
                        );
                        return EventPropagation::Stop;
                    }
                }
                EventPropagation::Continue
            })
            .on_event(EventListener::KeyDown, move |event| {
                if let Event::KeyDown(key) = event {
                    if matches!(
                        key.key.logical_key,
                        Key::Named(
                            NamedKey::Enter | NamedKey::Space | NamedKey::ArrowDown
                        )
                    ) {
                        let bounds = id.layout_rect();
                        show(
                            keyboard(),
                            Some(Point::new(bounds.x0, bounds.y1)),
                            Some(id),
                        );
                        return EventPropagation::Stop;
                    }
                }
                EventPropagation::Continue
            })
    }
    fn ide_context_menu(self, menu: impl Fn() -> Menu + 'static) -> impl View {
        let id = self.id();
        self.on_event(EventListener::PointerDown, move |event| {
            if let Event::PointerDown(pointer) = event {
                if pointer.button.is_secondary() {
                    let bounds = id.layout_rect();
                    show(
                        menu(),
                        Some(pointer.pos + bounds.origin().to_vec2()),
                        Some(id),
                    );
                    return EventPropagation::Stop;
                }
            }
            EventPropagation::Continue
        })
    }
}
impl<V: View + Sized + 'static> PopupMenuExt for V {}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn navigation_wraps_and_skips_disabled_entries_and_separators() {
        let menu = Menu::new("")
            .entry(MenuItem::new("Disabled").enabled(false))
            .separator()
            .entry(MenuItem::new("One"))
            .entry(MenuItem::new("Two"));
        assert_eq!(first_enabled(&menu), 2);
        assert_eq!(move_selection(&menu, 3, 1), 2);
        assert_eq!(move_selection(&menu, 2, -1), 3);
    }
}
