//! Keyboard navigation confined to the controls of an open IDE dialog.
use floem::{
    IntoView, View, ViewId,
    event::{Event, EventListener, EventPropagation},
    keyboard::{Key, Modifiers, NamedKey},
    reactive::{RwSignal, Scope, SignalGet, SignalUpdate, SignalWith},
    views::{
        Decorators, dyn_container, dyn_stack, empty, label, scroll, stack,
        text_input,
    },
};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
};

type Enabled = Rc<dyn Fn() -> bool>;
type Submit = Rc<RefCell<Option<Rc<dyn Fn()>>>>;
thread_local! {
    static POPUP_ESCAPE: Cell<bool> = const { Cell::new(false) };
    static FILE_PICKERS: Cell<usize> = const { Cell::new(0) };
}
pub(crate) fn external_dialog_active() -> bool {
    FILE_PICKERS.with(|count| count.get() > 0)
}
/// A popup Escape must not also dismiss its containing dialog on key-up.
pub(crate) fn note_popup_escape(){POPUP_ESCAPE.with(|handled|handled.set(true));}
pub(crate) fn consume_popup_escape() -> bool {
    POPUP_ESCAPE.with(|handled| handled.replace(false))
}

#[derive(Clone)]
pub struct ModalChoice {
    label: String,
    enabled: bool,
    selected: bool,
    action: Rc<dyn Fn()>,
}
impl ModalChoice {
    pub fn new(label: impl Into<String>, action: impl Fn() + 'static) -> Self {
        Self {
            label: label.into(),
            enabled: true,
            selected: false,
            action: Rc::new(action),
        }
    }
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }
}
struct ModalPopup {
    anchor: ViewId,
    choices: Vec<ModalChoice>,
    selected: RwSignal<usize>,
}
#[derive(Clone)]
pub struct ModalFocus {
    root: Rc<Cell<Option<ViewId>>>,
    controls: Rc<RefCell<HashMap<ViewId, Enabled>>>,
    submit: Submit,
    popup: RwSignal<Option<Rc<ModalPopup>>>,
    picker_open: RwSignal<bool>,
    popup_selection: RwSignal<usize>,
}
impl Default for ModalFocus {
    fn default() -> Self {
        Self {
            root: Default::default(),
            controls: Default::default(),
            submit: Default::default(),
            popup: Scope::current().create_rw_signal(None),
            picker_open: Scope::current().create_rw_signal(false),
            popup_selection: Scope::current().create_rw_signal(0),
        }
    }
}
impl ModalFocus {
    /// Keep the owning dialog modal while the asynchronous OS chooser is open.
    pub fn open_file(
        &self,
        options: floem::file::FileDialogOptions,
        selected: impl Fn(Option<floem::file::FileInfo>) + 'static,
    ) {
        let pending = self.picker_open;
        if pending.get_untracked() {
            return;
        }
        pending.set(true);
        FILE_PICKERS.with(|count| count.set(count.get() + 1));
        floem::action::open_file(options, move |file| {
            FILE_PICKERS.with(|count| count.set(count.get().saturating_sub(1)));
            pending.set(false);
            selected(file);
        });
    }
    pub fn set_root(&self, root: ViewId) {
        self.root.set(Some(root));
    }
    pub fn set_submit(&self, submit: impl Fn() + 'static) {
        *self.submit.borrow_mut() = Some(Rc::new(submit));
    }
    pub fn focus_first(&self) {
        if let Some(root) = self.root.get() {
            self.advance(root, false);
        }
    }
    pub fn bind_root<V: View + 'static>(
        &self,
        view: V,
        visible: RwSignal<bool>,
        config: floem::reactive::ReadSignal<
            std::sync::Arc<crate::config::SparrowConfig>,
        >,
    ) -> impl View + use<V> {
        let popup = self.popup;
        Scope::current().create_effect(move |_| {
            if !visible.get() {
                popup.set(None);
            }
        });
        let picker_open = self.picker_open;
        let picker_guard = empty()
            .keyboard_navigable()
            .on_event_stop(EventListener::KeyDown, |_| {})
            .on_event_stop(EventListener::PointerDown, |_| {})
            .style(move |s| {
                s.absolute()
                    .size_full()
                    .z_index(301)
                    .apply_if(!picker_open.get(), |s| s.hide())
            });
        let picker_guard_id = picker_guard.id();
        Scope::current().create_effect(move |_| {
            if picker_open.get() && visible.get_untracked() {
                picker_guard_id.request_focus();
            }
        });
        let view = stack((view, self.popup_view(config), picker_guard))
            .style(|s| s.absolute().size_full());
        let id = view.id();
        self.set_root(id);
        let navigation = self.clone();
        view.style(move |s| s.apply_if(!visible.get(), |s| s.hide()))
            .on_event(EventListener::KeyDown, move |event| {
                if let Event::KeyDown(key) = event {
                    if key.key.logical_key == Key::Named(NamedKey::Tab) {
                        navigation
                            .advance(id, key.modifiers.contains(Modifiers::SHIFT));
                        return EventPropagation::Stop;
                    }
                }
                EventPropagation::Continue
            })
    }
    fn dismiss_popup(&self, focus_anchor: bool) {
        let popup = self.popup.get_untracked();
        self.popup.set(None);
        if focus_anchor {
            if let Some(popup) = popup {
                popup.anchor.request_focus();
            }
        }
    }
    fn popup_view(
        &self,
        config: floem::reactive::ReadSignal<
            std::sync::Arc<crate::config::SparrowConfig>,
        >,
    ) -> impl View + use<> {
        let navigation = self.clone();
        let popup_state = self.popup;
        dyn_container(move || navigation.popup.get(), {
            let navigation = self.clone();
            move |popup: Option<Rc<ModalPopup>>| {
                let Some(popup) = popup else { return empty().into_any(); };
                let anchor = popup.anchor.layout_rect();
                let bounds = navigation.root.get().map(|id| id.layout_rect()).unwrap_or(anchor);
                let width = popup.choices.iter().map(|choice| choice.label.chars().count() as f64 * 8.0 + 40.0).fold(anchor.width().max(220.0), f64::max).min(420.0);
                let height = (popup.choices.len() as f64 * 26.0 + 8.0).min(320.0);
                let x = (anchor.x0 - bounds.x0).clamp(0.0, (bounds.width() - width).max(0.0));
                let y = if anchor.y1 + height <= bounds.y1 { anchor.y1 - bounds.y0 } else { (anchor.y0 - bounds.y0 - height).max(0.0) };
                let selected = popup.selected;
                let rows = dyn_stack({ let popup = popup.clone(); move || popup.choices.iter().cloned().enumerate().collect::<Vec<_>>() }, |(index, _)| *index, {
                    let navigation = navigation.clone();
                    move |(index, choice)| {
                        let enabled = choice.enabled;
                        let action = choice.action.clone();
                        let navigation = navigation.clone();
                        stack((label(move || if choice.selected { "✓" } else { "" }).style(|s| s.width(18.0)), label(move || choice.label.clone())))
                            .on_event_stop(EventListener::PointerEnter, move |_| { if enabled { selected.set(index); } })
                            .on_click_stop(move |_| { if enabled { navigation.dismiss_popup(true); action(); } })
                            .style(move |s| s.width_full().height(26.0).padding_horiz(8.0).items_center()
                                .color(config.get().color(if enabled { crate::config::color::SparrowColor::EDITOR_FOREGROUND } else { crate::config::color::SparrowColor::EDITOR_DIM }))
                                .apply_if(enabled && selected.get() == index, |s| s.background(config.get().color(crate::config::color::SparrowColor::SPARROW_TAB_ACTIVE_BACKGROUND))))
                    }
                }).style(|s| s.flex_col().width_full());
                let menu = scroll(rows).ensure_visible(move || floem::kurbo::Rect::new(0.0, selected.get() as f64 * 26.0, 1.0, (selected.get() + 1) as f64 * 26.0))
                    .style(move |s| s.width(width).max_height(312.0));
                let menu = stack((menu,)).keyboard_navigable().on_event_stop(EventListener::PointerDown, |_| {})
                    .style(move |s| s.absolute().inset_left(x).inset_top(y).width(width).max_height(320.0).padding_vert(4.0).flex_col().border(1.0).border_radius(5.0)
                        .font_family(config.get().ui.font_family.clone()).font_size(13.0)
                        .border_color(config.get().color(crate::config::color::SparrowColor::SPARROW_BORDER))
                        .background(config.get().color(crate::config::color::SparrowColor::PANEL_BACKGROUND)));
                let id = menu.id();
                let key_navigation = navigation.clone();
                let key_popup = popup.clone();
                let menu = menu.on_event(EventListener::KeyDown, move |event| {
                    let Event::KeyDown(key) = event else { return EventPropagation::Continue; };
                    match &key.key.logical_key {
                        Key::Named(NamedKey::Escape) => { POPUP_ESCAPE.with(|handled| handled.set(true)); key_navigation.dismiss_popup(true); }
                        Key::Named(NamedKey::Tab) => { key_navigation.dismiss_popup(false); key_navigation.advance(key_popup.anchor, key.modifiers.contains(Modifiers::SHIFT)); }
                        Key::Named(NamedKey::Enter | NamedKey::Space) => {
                            if let Some(choice) = key_popup.choices.get(selected.get_untracked()).filter(|choice| choice.enabled) { key_navigation.dismiss_popup(true); (choice.action)(); }
                        }
                        key => {
                            let enabled = key_popup.choices.iter().map(|choice| choice.enabled).collect::<Vec<_>>();
                            if let Some(index) = menu_destination(key, selected.get_untracked(), &enabled) { selected.set(index); }
                            else if let Key::Character(text) = key {
                                let start = selected.get_untracked() + 1;
                                if let Some(index) = (start..start+enabled.len()).map(|i|i % enabled.len()).find(|i| enabled[*i] && key_popup.choices[*i].label.to_lowercase().starts_with(&text.to_lowercase())) { selected.set(index); }
                            }
                        }
                    }
                    EventPropagation::Stop
                });
                let popup_state = navigation.popup;
                floem::action::exec_after(std::time::Duration::from_millis(30), move |_| {
                    if popup_state.with_untracked(|current| current.as_ref().is_some_and(|current| Rc::ptr_eq(current, &popup))) { id.request_focus(); }
                });
                let outside_navigation = navigation.clone();
                stack((menu,)).on_event_stop(EventListener::PointerDown, move |_| outside_navigation.dismiss_popup(true))
                    .style(|s| s.absolute().size_full().z_index(300)).into_any()
            }
        }).style(move |s|s.absolute().size_full().apply_if(popup_state.with(|popup|popup.is_none()),|s|s.hide()))
    }
    /// Register the actual button inside its tooltip, so Enter reaches its click handler.
    pub fn icon<I, C, A, D, T, S>(
        &self,
        icon: I,
        on_click: C,
        active: A,
        disabled: D,
        tooltip: T,
        config: floem::reactive::ReadSignal<
            std::sync::Arc<crate::config::SparrowConfig>,
        >,
    ) -> impl View + use<I, C, A, D, T, S>
    where
        I: Fn() -> &'static str + 'static,
        C: Fn() + 'static,
        A: Fn() -> bool + 'static,
        D: Fn() -> bool + Copy + 'static,
        T: Fn() -> S + Clone + 'static,
        S: std::fmt::Display + 'static,
    {
        let button = crate::app::clickable_icon_base(
            icon,
            None::<fn()>,
            active,
            disabled,
            config,
        );
        let id = button.id();
        let navigation = self.clone();
        let button = button.on_click_stop(move |_| {
            on_click();
            if disabled() {
                // A removed task can hide its fields and disable this button.
                // Advance after layout, so focus lands on a visible control.
                let navigation = navigation.clone();
                floem::action::exec_after(
                    std::time::Duration::from_millis(30),
                    move |_| navigation.advance(id, false),
                );
            }
        });
        crate::app::tooltip_label(
            config,
            self.control_if(button, move || !disabled()),
            tooltip,
        )
    }

    pub fn menu<V: View + 'static, M: Fn() -> Vec<ModalChoice> + 'static>(
        &self,
        view: V,
        menu: M,
    ) -> impl View + use<V, M> {
        self.menu_if(view, || true, menu)
    }
    pub fn menu_if<
        V: View + 'static,
        E: Fn() -> bool + 'static,
        M: Fn() -> Vec<ModalChoice> + 'static,
    >(
        &self,
        view: V,
        enabled: E,
        menu: M,
    ) -> impl View + use<V, E, M> {
        let id = view.id();
        let popup = self.popup;
        let selection = self.popup_selection;
        let open = Rc::new(move || {
            let choices = menu();
            let selected = choices
                .iter()
                .position(|choice| choice.enabled && choice.selected)
                .or_else(|| choices.iter().position(|choice| choice.enabled));
            if let Some(selected) = selected {
                selection.set(selected);
                popup.set(Some(Rc::new(ModalPopup {
                    anchor: id,
                    choices,
                    selected: selection,
                })));
            }
        });
        let keyboard_open = open.clone();
        self.control_if(
            view.on_click_stop(move |_| open()).on_event(
                EventListener::KeyDown,
                move |event| {
                    if let Event::KeyDown(key) = event {
                        if key.key.logical_key == Key::Named(NamedKey::ArrowDown)
                            && !key
                                .modifiers
                                .intersects(Modifiers::CONTROL | Modifiers::META)
                        {
                            keyboard_open();
                            return EventPropagation::Stop;
                        }
                    }
                    EventPropagation::Continue
                },
            ),
            enabled,
        )
    }

    pub fn input(&self, value: RwSignal<String>) -> impl View + use<> {
        let submit = self.submit.clone();
        let picker_open = self.picker_open;
        let input = text_input(value).on_enter(move || {
            if picker_open.get_untracked() {
                return;
            }
            let action = submit.borrow().clone();
            if let Some(action) = action {
                action();
            }
        });
        let id = input.id();
        // Floem's Auto width gives its internal text node only a fixed
        // character count, even when the outer field grows with flex layout.
        self.control(
            input
                .on_click_stop(move |_| id.request_focus())
                .style(|s| s.width_full().min_width(0.0)),
        )
    }
    pub fn control<V: View + 'static>(&self, view: V) -> impl View + use<V> {
        self.control_if(view, || true)
    }
    pub fn control_if<V: View + 'static, E: Fn() -> bool + 'static>(
        &self,
        view: V,
        enabled: E,
    ) -> impl View + use<V, E> {
        let id = view.id();
        self.controls.borrow_mut().insert(id, Rc::new(enabled));
        let navigation = self.clone();
        let cleanup = self.controls.clone();
        let picker_open = self.picker_open;
        view.keyboard_navigable()
            .on_event(EventListener::KeyDown, move |event| {
                if picker_open.get_untracked() {
                    return EventPropagation::Stop;
                }
                if let Event::KeyDown(key) = event {
                    if key.key.logical_key == Key::Named(NamedKey::Tab)
                        && !key.modifiers.intersects(
                            Modifiers::CONTROL | Modifiers::ALT | Modifiers::META,
                        )
                    {
                        navigation
                            .advance(id, key.modifiers.contains(Modifiers::SHIFT));
                        return EventPropagation::Stop;
                    }
                }
                EventPropagation::Continue
            })
            .on_cleanup(move || {
                cleanup.borrow_mut().remove(&id);
            })
            .style(|s| {
                s.focus(|s| {
                    s.background(floem::peniko::Color::from_rgba8(75, 119, 200, 55))
                })
            })
    }
    fn advance(&self, current: ViewId, backwards: bool) {
        fn visit(
            id: ViewId,
            current: ViewId,
            controls: &HashMap<ViewId, Enabled>,
            order: &mut Vec<ViewId>,
        ) {
            if id.is_hidden_recursive() {
                return;
            }
            if controls
                .get(&id)
                .is_some_and(|enabled| id == current || enabled())
            {
                order.push(id);
            }
            for child in id.children() {
                visit(child, current, controls, order);
            }
        }
        let Some(root) = self.root.get() else {
            return;
        };
        let mut order = Vec::new();
        visit(root, current, &self.controls.borrow(), &mut order);
        if order.is_empty() {
            return;
        }
        let index = order.iter().position(|id| *id == current);
        let next = match (index, backwards) {
            (Some(index), true) => (index + order.len() - 1) % order.len(),
            (Some(index), false) => (index + 1) % order.len(),
            (None, true) => order.len() - 1,
            (None, false) => 0,
        };
        order[next].request_focus();
    }
}

/// Menus wrap and skip disabled entries, unlike category/list selection.
fn menu_destination(key: &Key, current: usize, enabled: &[bool]) -> Option<usize> {
    let len = enabled.len();
    if len == 0 {
        return None;
    }
    match key {
        Key::Named(NamedKey::Home) => enabled.iter().position(|enabled| *enabled),
        Key::Named(NamedKey::End) => enabled.iter().rposition(|enabled| *enabled),
        Key::Named(NamedKey::ArrowDown) => (1..=len)
            .map(|step| (current + step) % len)
            .find(|index| enabled[*index]),
        Key::Named(NamedKey::ArrowUp) => (1..=len)
            .map(|step| (current + len - step) % len)
            .find(|index| enabled[*index]),
        _ => None,
    }
}
/// Selection keys used by dialog lists. Boundaries stay on the first/last row.
pub(crate) fn list_destination(
    key: &Key,
    current: usize,
    len: usize,
) -> Option<usize> {
    if len == 0 {
        return None;
    }
    let current = current.min(len - 1);
    match key {
        Key::Named(NamedKey::ArrowUp) => Some(current.saturating_sub(1)),
        Key::Named(NamedKey::ArrowDown) => Some((current + 1).min(len - 1)),
        Key::Named(NamedKey::Home) => Some(0),
        Key::Named(NamedKey::End) => Some(len - 1),
        _ => None,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn menu_keys_wrap_and_skip_disabled_choices() {
        let enabled = [true, false, true];
        for (key, current, expected) in [
            (NamedKey::ArrowDown, 0, 2),
            (NamedKey::ArrowDown, 2, 0),
            (NamedKey::ArrowUp, 0, 2),
            (NamedKey::ArrowUp, 2, 0),
            (NamedKey::Home, 2, 0),
            (NamedKey::End, 0, 2),
        ] {
            assert_eq!(
                menu_destination(&Key::Named(key), current, &enabled),
                Some(expected)
            );
        }
        assert_eq!(
            menu_destination(&Key::Named(NamedKey::ArrowDown), 0, &[]),
            None
        );
        assert_eq!(
            menu_destination(&Key::Named(NamedKey::ArrowDown), 0, &[false, false]),
            None
        );
        assert_eq!(
            menu_destination(&Key::Named(NamedKey::Enter), 0, &enabled),
            None
        );
    }
    #[test]
    fn list_selection_handles_empty_lists_boundaries_and_shortcuts() {
        assert_eq!(
            list_destination(&Key::Named(NamedKey::ArrowDown), 0, 0),
            None
        );
        for (key, current, expected) in [
            (NamedKey::ArrowUp, 0, 0),
            (NamedKey::ArrowDown, 2, 2),
            (NamedKey::ArrowUp, 2, 1),
            (NamedKey::ArrowDown, 0, 1),
            (NamedKey::Home, 2, 0),
            (NamedKey::End, 0, 2),
        ] {
            assert_eq!(
                list_destination(&Key::Named(key), current, 3),
                Some(expected)
            );
        }
        assert_eq!(list_destination(&Key::Named(NamedKey::Enter), 0, 3), None);
    }
}

/// Return keyboard input to the workbench when a native dialog closes.
pub(crate) fn restore_workbench_on_close(active:RwSignal<bool>,common:Rc<crate::window_tab::CommonData>){
    floem::reactive::create_effect(move |previous:Option<bool>|{let open=active.get();if previous==Some(true)&&!open{common.keyboard_focus.set(None);common.view_id.get_untracked().request_focus();}open});
}
