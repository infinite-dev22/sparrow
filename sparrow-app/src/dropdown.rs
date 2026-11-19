#![deny(missing_docs)]
//! Adapted from Floem 31fa8f444c37f4c314f47d88c23ffdbc25f2ab53.
//! Copyright (c) 2023 Floem; MIT license in `licenses/floem-dropdown-MIT.txt`.
//! Sparrow maintains this implementation locally.
//!
//! A view that allows the user to select an item from a list of items.
//!
//! The [`Dropdown`] struct provides several constructors, each offering different levels of customization and ease of use.
//!
//! The [`DropdownCustomStyle`] struct allows for easy and advanced customization of the dropdown's appearance.
use std::{any::Any, rc::Rc};

use floem::keyboard::{Key, NamedKey};
use floem::peniko::kurbo::{Point, Rect, Size};
use floem::reactive::{
    Scope, SignalGet, SignalUpdate, as_child_of_current_scope, create_effect,
    create_updater,
};

use floem::{
    AnyView, IntoView, View, ViewId,
    action::{add_overlay, remove_overlay},
    event::{Event, EventListener, EventPropagation},
    prop, prop_extractor,
    style::{CustomStylable, CustomStyle, Style, StyleClass, StyleClassRef, Width},
    style_class,
    unit::PxPctAuto,
    views::{Decorators, empty, scroll, stack, svg, text},
};

use crate::config::icon::SparrowIcons;
use crate::config::{SparrowConfig, color::SparrowColor};
use floem::reactive::{ReadSignal, RwSignal};
use floem::views::{list, scroll::ScrollClass};
use std::sync::Arc;

// Floem's OverlayView moves overlays inside a five-pixel window margin at
// paint time. Keep our frame inside that margin so paint and hit-testing
// retain the same origin. The backdrop itself still covers the whole window.
fn overlay_frame_size(window: Size) -> Size {
    Size::new(
        (window.width - 5.0).max(0.0),
        (window.height - 5.0).max(0.0),
    )
}

type ChildFn<T> = dyn Fn(T) -> (AnyView, Scope);
type ListViewFn<T> = Rc<dyn Fn(&dyn Fn(T) -> AnyView) -> AnyView>;

style_class!(
    /// A Style class that is applied to all dropdowns.
    pub DropdownClass
);

style_class!(
    /// Styles the selected-value label in the default dropdown display.
    pub DropdownLabelClass
);

style_class!(
    /// Styles the default chevron, including its size and color.
    pub DropdownChevronClass
);

prop!(
    /// A property that determines whether the dropdown should close automatically when an item is selected.
    pub CloseOnAccept: bool {} = true
);
prop_extractor!(DropdownStyle {
    close_on_accept: CloseOnAccept,
});

/// # A customizable dropdown view for selecting an item from a list.
///
/// The `Dropdown` struct provides several constructors, each offering different levels of
/// customization and ease of use:
///
/// - [`Dropdown::new_rw`]: The simplest constructor, ideal for quick setup with minimal customization.
///   It uses default views and assumes direct access to a signal that can be both read from and written to for driving the selection of an item.
///
/// - [`Dropdown::new`]: Similar to `new_rw`, but uses a read-only function for the active item, and requires that you manually provide an `on_accept` callback.
///
/// - [`Dropdown::custom`]: Offers full customization, letting you define custom view functions for
///   both the main display and list items. Uses a read-only function for the active item and requires that you manually provide an `on_accept` callback.
///
/// - The dropdown also has methods [`Dropdown::main_view`] and [`Dropdown::list_item_view`] that let you override the main view function and list item view function respectively.
///
/// Choose the constructor that best fits your needs based on the level of customization required.
///
/// ## Usage with Enums
///
/// A common scenario is populating a dropdown menu from an enum. The `widget-gallery` example does this.
///
/// The below example creates a dropdown with three items, one for each character in our `Character` enum.
///
/// The `strum` crate is handy for this use case. This example uses the `strum` crate to create an iterator for our `Character` enum.
///
/// First, define the enum and implement `Clone`, `strum::EnumIter`, and `Display` on it:
/// ```ignore
/// use strum::IntoEnumIterator;
///
/// #[derive(Clone, strum::EnumIter)]
/// enum Character {
///     Ori,
///     Naru,
///     Gumo,
/// }
///
/// impl std::fmt::Display for Character {
///     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
///         match self {
///             Self::Ori => write!(f, "Ori"),
///             Self::Naru => write!(f, "Naru"),
///             Self::Gumo => write!(f, "Gumo"),
///         }
///     }
/// }
/// ```
///
/// Then, create a signal:
/// ```ignore
/// # use strum::IntoEnumIterator;
/// #
/// # #[derive(Clone, strum::EnumIter)]
/// # enum Character {
/// #     Ori,
/// #     Naru,
/// #     Gumo,
/// # }
/// #
/// # impl std::fmt::Display for Character {
/// #     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
/// #         match self {
/// #             Self::Ori => write!(f, "Ori"),
/// #             Self::Naru => write!(f, "Naru"),
/// #             Self::Gumo => write!(f, "Gumo"),
/// #         }
/// #     }
/// # }
/// #
/// # use floem::reactive::RwSignal;
/// let selected = RwSignal::new(Character::Ori);
/// ```
///
/// Finally, create the dropdown using one of the available constructors, like [`Dropdown::new_rw`]:
///
/// ```ignore
/// # use strum::IntoEnumIterator;
/// #
/// # #[derive(Clone, strum::EnumIter)]
/// # enum Character {
/// #     Ori,
/// #     Naru,
/// #     Gumo,
/// # }
/// #
/// # impl std::fmt::Display for Character {
/// #     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
/// #         match self {
/// #             Self::Ori => write!(f, "Ori"),
/// #             Self::Naru => write!(f, "Naru"),
/// #             Self::Gumo => write!(f, "Gumo"),
/// #         }
/// #     }
/// # }
/// #
/// # fn character_select() -> impl floem::IntoView {
/// #     use floem::{prelude::*, views::dropdown::Dropdown};
/// # let selected = RwSignal::new(Character::Ori);
/// Dropdown::new_rw(selected, Character::iter())
/// # }
/// ```
///
/// ## Styling
///
/// You can modify the behavior of the dropdown through the `CloseOnAccept` property.
/// If the property is set to `true`, the dropdown will automatically close when an item is selected.
/// If the property is set to `false`, the dropdown will not automatically close when an item is selected.
/// The default is `true`.
/// Styling Example:
/// ```ignore
/// # use floem::views::dropdown;
/// # use floem::views::empty;
/// # use floem::views::Decorators;
/// // root view
/// empty().style(|s| {
///     s.class(dropdown::DropdownClass, |s| {
///         s.set(dropdown::CloseOnAccept, false)
///     })
/// });
/// ```
pub struct Dropdown<T: 'static> {
    id: ViewId,
    current_value: T,
    pointer_pressed: bool,
    main_view: ViewId,
    main_view_scope: Scope,
    main_fn: Box<ChildFn<T>>,
    list_view: ListViewFn<T>,
    list_item_fn: Rc<dyn Fn(T) -> AnyView>,
    list_style: Style,
    overlay_id: Option<ViewId>,
    window_origin: Option<Point>,
    on_accept: Option<Box<dyn Fn(T)>>,
    on_open: Option<Box<dyn Fn(bool)>>,
    style: DropdownStyle,
}

enum Message {
    OpenState(bool),
    ActiveElement(Box<dyn Any>),
    ListFocusLost,
    DismissWithFocus,
    ListSelect(Box<dyn Any>),
}

impl<T: 'static + Clone> View for Dropdown<T> {
    fn id(&self) -> ViewId {
        self.id
    }

    fn debug_name(&self) -> std::borrow::Cow<'static, str> {
        "DropDown".into()
    }

    fn style_pass(&mut self, cx: &mut floem::context::StyleCx<'_>) {
        if self.style.read(cx) {
            cx.app_state_mut().request_paint(self.id);
        }
        self.list_style = Style::new().apply_classes_from_context(
            &[StyleClassRef {
                key: ScrollClass::key(),
            }],
            &cx.style(),
        );

        for child in self.id.children() {
            cx.style_view(child);
        }
    }

    fn compute_layout(
        &mut self,
        cx: &mut floem::context::ComputeLayoutCx,
    ) -> Option<Rect> {
        self.window_origin = Some(cx.window_origin());

        self.id
            .children()
            .into_iter()
            .filter(|child| !child.style_has_hidden())
            .filter_map(|child| cx.compute_view_layout(child))
            .reduce(|rect, child| rect.union(child))
    }

    fn update(
        &mut self,
        cx: &mut floem::context::UpdateCx,
        state: Box<dyn std::any::Any>,
    ) {
        if let Ok(state) = state.downcast::<Message>() {
            match *state {
                Message::OpenState(true) => self.open_dropdown(cx),
                Message::OpenState(false) => self.close_dropdown(),
                Message::ListFocusLost => self.close_dropdown(),
                Message::DismissWithFocus => {
                    self.close_dropdown();
                    self.id.request_focus();
                }
                Message::ListSelect(val) => {
                    if let Ok(val) = val.downcast::<T>() {
                        if self.style.close_on_accept() {
                            self.close_dropdown();
                            self.id.request_focus();
                        }
                        if let Some(on_select) = &self.on_accept {
                            on_select(*val);
                        }
                    }
                }
                Message::ActiveElement(val) => {
                    if let Ok(val) = val.downcast::<T>() {
                        let old_child_scope = self.main_view_scope;
                        let old_main_view = self.main_view;
                        let (main_view, main_view_scope) =
                            (self.main_fn)((*val).clone());
                        let main_view_id = main_view.id();
                        self.id.set_children([main_view]);
                        self.current_value = *val;
                        self.main_view = main_view_id;
                        self.main_view_scope = main_view_scope;

                        cx.app_state_mut().remove_view(old_main_view);
                        old_child_scope.dispose();
                        self.id.request_all();
                    }
                }
            }
        }
    }

    fn event_before_children(
        &mut self,
        _cx: &mut floem::context::EventCx,
        event: &Event,
    ) -> EventPropagation {
        match event {
            Event::PointerDown(pointer) if pointer.button.is_primary() => {
                self.pointer_pressed = true;
                self.id.request_active();
                self.id.request_focus();
                return EventPropagation::Stop;
            }
            Event::PointerUp(pointer) if pointer.button.is_primary() => {
                let pressed = std::mem::take(&mut self.pointer_pressed);
                self.id.clear_active();
                if pressed
                    && self
                        .id
                        .get_size()
                        .is_some_and(|size| size.to_rect().contains(pointer.pos))
                {
                    self.swap_state();
                }
                return EventPropagation::Stop;
            }
            Event::KeyDown(key_event)
                if matches!(
                    key_event.key.logical_key,
                    Key::Named(NamedKey::Enter) | Key::Named(NamedKey::Space)
                ) =>
            {
                self.swap_state();
                return EventPropagation::Stop;
            }
            _ => {}
        }

        EventPropagation::Continue
    }
}

impl<T: Clone> Dropdown<T> {
    /// Creates a default main view for the dropdown.
    ///
    /// This function generates a view that displays the given item as text,
    /// along with a chevron-down icon to indicate that it's a dropdown.
    /// Style the parts through [`DropdownLabelClass`] and
    /// [`DropdownChevronClass`] on the dropdown. The chevron inherits the
    /// foreground color unless its class overrides it. Use [`Self::main_view`]
    /// to replace the layout or icon entirely.
    pub fn default_main_view(item: T) -> AnyView
    where
        T: std::fmt::Display,
    {
        Self::main_display(item, || {
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 12 12"><path d="M2.5 4.5 6 8l3.5-3.5" fill="none" stroke="currentColor" stroke-width="1.5"/></svg>"#.into()
        })
    }

    fn main_display(item: T, icon: impl Fn() -> String + 'static) -> AnyView
    where
        T: std::fmt::Display,
    {
        stack((
            text(item).class(DropdownLabelClass),
            svg(icon).class(DropdownChevronClass),
        ))
        .style(|s| s.items_center().gap(6.0).size_full().min_width(0.0))
        .into_any()
    }

    /// Creates a new customizable dropdown.
    ///
    /// You might want to use some of the simpler constructors like [`Dropdown::new`] or [`Dropdown::new_rw`].
    ///
    /// # Example
    /// ```ignore
    /// # use floem::{*, views::*, reactive::*};
    /// # use floem::views::dropdown::*;
    /// let active_item = RwSignal::new(3);
    ///
    /// Dropdown::custom(
    ///     move || active_item.get(),
    ///     |main_item| text(main_item).into_any(),
    ///     1..=5,
    ///     |list_item| text(list_item).into_any(),
    /// )
    /// .on_accept(move |item| active_item.set(item));
    /// ```
    ///
    /// This function provides full control over the dropdown's appearance and behavior
    /// by allowing custom view functions for both the main display and list items.
    ///
    /// # Arguments
    ///
    /// * `active_item` - A function that returns the currently selected item.
    ///
    /// * `main_view` - A function that takes a value of type `T` and returns an `AnyView`
    ///   to be used as the main dropdown display.
    ///
    /// * `iterator` - An iterator that provides the items to be displayed in the dropdown list.
    ///
    /// * `list_item_fn` - A function that takes a value of type `T` and returns an `AnyView`
    ///   to be used for each item in the dropdown list.
    pub fn custom<MF, I, LF, AIF>(
        active_item: AIF,
        main_view: MF,
        iterator: I,
        list_item_fn: LF,
    ) -> Dropdown<T>
    where
        MF: Fn(T) -> AnyView + 'static,
        I: IntoIterator<Item = T> + Clone + 'static,
        LF: Fn(T) -> AnyView + Clone + 'static,
        T: Clone + 'static,
        AIF: Fn() -> T + 'static,
    {
        let dropdown_id = ViewId::new();

        let list_item_fn = Rc::new(list_item_fn);

        let list_view = Rc::new(move |list_item_fn: &dyn Fn(T) -> AnyView| {
            let iterator = iterator.clone();
            let iter_clone = iterator.clone();
            let inner_list = list(iterator.into_iter().map(|value| {
                let selected = value.clone();
                list_item_fn(value).on_click_stop(move |_| {
                    dropdown_id.update_state(Message::ActiveElement(Box::new(selected.clone())));
                    dropdown_id.update_state(Message::ListSelect(Box::new(selected.clone())));
                })
            }))
                .on_accept(move |opt_idx| {
                    if let Some(idx) = opt_idx {
                        if let Some(val) = iter_clone.clone().into_iter().nth(idx) {
                            dropdown_id.update_state(Message::ActiveElement(
                                Box::new(val.clone()),
                            ));
                            dropdown_id
                                .update_state(Message::ListSelect(Box::new(val)));
                        }
                    }
                })
                .style(|s| s.size_full())
                .keyboard_navigable()
                .on_event(EventListener::KeyDown, move |event| {
                    if matches!(event, Event::KeyDown(key) if key.key.logical_key == Key::Named(NamedKey::Escape)) {
                        crate::modal_focus::note_popup_escape();
                        dropdown_id.update_state(Message::DismissWithFocus);
                        EventPropagation::Stop
                    } else {
                        EventPropagation::Continue
                    }
                })
                .on_event_stop(EventListener::FocusLost, move |_| {
                    dropdown_id.update_state(Message::ListFocusLost);
                })
                .on_event_stop(EventListener::PointerMove, |_| {});
            let inner_list_id = inner_list.id();
            scroll(inner_list)
                .on_event_stop(EventListener::FocusGained, move |_| {
                    inner_list_id.request_focus();
                })
                .into_any()
        });

        let initial = create_updater(active_item, move |new_state| {
            dropdown_id.update_state(Message::ActiveElement(Box::new(new_state)));
        });

        let main_fn = Box::new(as_child_of_current_scope(main_view));

        let (child, main_view_scope) = main_fn(initial.clone());
        let main_view = child.id();

        dropdown_id.set_children([child]);

        Self {
            id: dropdown_id,
            current_value: initial,
            pointer_pressed: false,
            main_view,
            main_view_scope,
            main_fn,
            list_view,
            list_item_fn,
            list_style: Style::new(),
            overlay_id: None,
            window_origin: None,
            on_accept: None,
            on_open: None,
            style: Default::default(),
        }
        .class(DropdownClass)
        .style(|s| {
            s.class(DropdownLabelClass, |s| {
                s.flex_grow(1.0_f32).min_width(0.0).text_ellipsis()
            })
            .class(DropdownChevronClass, |s| {
                s.size(12.0, 12.0).flex_shrink(0.0_f32)
            })
        })
    }

    /// Creates a new dropdown with a read-only function for the active item.
    ///
    /// # Example
    /// ```ignore
    /// # use floem::{*, views::*, reactive::*};
    /// # use floem::views::dropdown::*;
    /// let active_item = RwSignal::new(3);
    ///
    /// Dropdown::new(move || active_item.get(), 1..=5).on_accept(move |val| active_item.set(val));
    /// ```
    ///
    /// This function is a convenience wrapper around `Dropdown::new` that uses default views
    /// for the main and list items.
    ///
    /// See also [`Dropdown::new_rw`].
    ///
    /// # Arguments
    ///
    /// * `active_item` - A function that returns the currently selected item.
    ///
    /// * `iterator` - An iterator that provides the items to be displayed in the dropdown list.
    pub fn new<AIF, I>(active_item: AIF, iterator: I) -> Dropdown<T>
    where
        AIF: Fn() -> T + 'static,
        I: IntoIterator<Item = T> + Clone + 'static,
        T: Clone + std::fmt::Display + 'static,
    {
        Self::custom(active_item, Self::default_main_view, iterator, |v| {
            floem::views::text(v).into_any()
        })
    }

    /// Creates a new dropdown with a read-write signal for the active item.
    ///
    /// # Example:
    /// ```ignore
    /// # use floem::{*, views::*, reactive::*};
    /// # use floem::{views::dropdown::*};
    /// let dropdown_active_item = RwSignal::new(3);
    ///
    /// Dropdown::new_rw(dropdown_active_item, 1..=5);
    /// ```
    ///
    /// This function is a convenience wrapper around `Dropdown::custom` that uses default views
    /// for the main and list items.
    ///
    /// # Arguments
    ///
    /// * `active_item` - A read-write signal representing the currently selected item.
    ///   It must implement `SignalGet<T>` and `SignalUpdate<T>`.
    ///
    /// * `iterator` - An iterator that provides the items to be displayed in the dropdown list.
    pub fn new_rw<AI, I>(active_item: AI, iterator: I) -> Dropdown<T>
    where
        AI: SignalGet<T> + SignalUpdate<T> + Copy + 'static,
        I: IntoIterator<Item = T> + Clone + 'static,
        T: Clone + std::fmt::Display + 'static,
    {
        Self::custom(
            move || active_item.get(),
            Self::default_main_view,
            iterator,
            |t| text(t).into_any(),
        )
        .on_accept(move |nv| active_item.set(nv))
    }

    /// Overrides the main view for the dropdown.
    pub fn main_view(
        mut self,
        main_view: impl Fn(T) -> Box<dyn View> + 'static,
    ) -> Self {
        self.main_fn = Box::new(as_child_of_current_scope(main_view));
        let (child, main_view_scope) = (self.main_fn)(self.current_value.clone());
        let main_view = child.id();
        self.main_view_scope = main_view_scope;
        self.main_view = main_view;
        self.id.set_children([child]);
        self
    }

    /// Overrides the list view for each item in the dropdown list.
    pub fn list_item_view(
        mut self,
        list_item_fn: impl Fn(T) -> Box<dyn View> + 'static,
    ) -> Self {
        self.list_item_fn = Rc::new(list_item_fn);
        self
    }

    /// Sets a reactive condition for showing or hiding the dropdown list.
    ///
    /// # Reactivity
    /// The `show` function will be re-run whenever any signal it depends on changes.
    pub fn show_list(self, show: impl Fn() -> bool + 'static) -> Self {
        let id = self.id();
        create_effect(move |_| {
            let state = show();
            id.update_state(Message::OpenState(state));
        });
        self
    }

    /// Sets a callback function to be called when an item is selected from the dropdown.
    ///
    /// Only one `on_accept` callback can be set at a time.
    pub fn on_accept(mut self, on_accept: impl Fn(T) + 'static) -> Self {
        self.on_accept = Some(Box::new(on_accept));
        self
    }

    /// Sets a callback function to be called when the dropdown is opened.
    ///
    /// Only one `on_open` callback can be set at a time.
    pub fn on_open(mut self, on_open: impl Fn(bool) + 'static) -> Self {
        self.on_open = Some(Box::new(on_open));
        self
    }

    fn swap_state(&self) {
        if self.overlay_id.is_some() {
            self.id.update_state(Message::OpenState(false));
        } else {
            self.id.request_layout();
            self.id.update_state(Message::OpenState(true));
        }
    }

    fn open_dropdown(&mut self, cx: &mut floem::context::UpdateCx) {
        if self.overlay_id.is_none() {
            self.id.request_layout();
            cx.app_state_mut().compute_layout();
            if let Some(layout) = self.id.get_layout() {
                self.update_list_style(layout.size.width as f64);
                let point = self.window_origin.unwrap_or_default()
                    + (0., layout.size.height as f64);
                self.create_overlay(point);

                if let Some(on_open) = &self.on_open {
                    on_open(true);
                }
            }
        }
    }

    fn close_dropdown(&mut self) {
        if let Some(id) = self.overlay_id.take() {
            remove_overlay(id);
            if let Some(on_open) = &self.on_open {
                on_open(false);
            }
        }
    }

    fn update_list_style(&mut self, width: f64) {
        if let PxPctAuto::Pct(pct) = self.list_style.get(Width) {
            let new_width = width * pct / 100.0;
            self.list_style = self.list_style.clone().width(new_width);
        }
    }

    fn create_overlay(&mut self, point: Point) {
        let list = self.list_view.clone();
        let list_style = self.list_style.clone();
        let list_item_fn = self.list_item_fn.clone();
        let dropdown_id = self.id;
        let mut root = self.id;
        while let Some(parent) = root.parent() {
            root = parent;
        }
        let size = root.get_size().unwrap_or_default();
        let frame_size = overlay_frame_size(size);
        self.overlay_id = Some(add_overlay(Point::ZERO, move |_| {
            let menu = list(&*list_item_fn.clone())
                .style(move |s| {
                    s.apply(list_style.clone())
                        .absolute()
                        .inset_left(point.x)
                        .inset_top(point.y)
                        .z_index(401)
                })
                .on_event_stop(EventListener::PointerDown, |_| {});
            let list_id = menu.id();
            list_id.request_focus();
            stack((
                empty()
                    .style(move |s| s.absolute().size(size.width, size.height))
                    .on_event_stop(EventListener::PointerDown, move |_| {
                        dropdown_id.update_state(Message::DismissWithFocus);
                    }),
                menu,
            ))
            .style(move |s| s.size(frame_size.width, frame_size.height).z_index(400))
        }));
    }

    /// Sets the custom style properties of the `Dropdown`.
    pub fn dropdown_style(
        self,
        style: impl Fn(DropdownCustomStyle) -> DropdownCustomStyle + 'static,
    ) -> Self {
        self.custom_style(style)
    }
}

#[derive(Debug, Clone, Default)]
/// A struct that allows for easy custom styling of the `Dropdown` using the [`Dropdown::dropdown_style`] method or the [`Style::custom_style`](crate::style::CustomStylable::custom_style) method.
pub struct DropdownCustomStyle(Style);
impl From<DropdownCustomStyle> for Style {
    fn from(val: DropdownCustomStyle) -> Self {
        val.0
    }
}
impl From<Style> for DropdownCustomStyle {
    fn from(val: Style) -> Self {
        Self(val)
    }
}
impl CustomStyle for DropdownCustomStyle {
    type StyleClass = DropdownClass;
}

impl<T: Clone> CustomStylable<DropdownCustomStyle> for Dropdown<T> {
    type DV = Self;
}

impl DropdownCustomStyle {
    /// Creates a new `DropDownCustomStyle` with default values.
    pub fn new() -> Self {
        Self::default()
    }
    /// Sets the `CloseOnAccept` property for the dropdown, which determines whether the dropdown
    /// should automatically close when an item is selected. The default value is `true`.
    ///
    /// # Arguments
    /// * `close`: If set to `true`, the dropdown will close upon item selection. If `false`, it
    ///   will remain open after an item is selected.
    pub fn close_on_accept(mut self, close: bool) -> Self {
        self = Self(self.0.set(CloseOnAccept, close));
        self
    }
}

impl<T> Drop for Dropdown<T> {
    fn drop(&mut self) {
        if let Some(id) = self.overlay_id {
            remove_overlay(id)
        }
    }
}

/// Shared appearance for Sparrow dropdowns; callers supply values and layout.
pub fn dropdown<T>(
    value: RwSignal<T>,
    items: impl IntoIterator<Item = T> + Clone + 'static,
    config: ReadSignal<Arc<SparrowConfig>>,
) -> Dropdown<T>
where
    T: Clone + std::fmt::Display + 'static,
{
    let items: Vec<T> = items.into_iter().collect();
    let list_height = (items.len() as f64 * 28.0 + 2.0).clamp(2.0, 240.0);
    Dropdown::custom(
        move || value.get(),
        move |item| {
            Dropdown::main_display(item, move || {
                config.get().ui_svg(SparrowIcons::PALETTE_MENU)
            })
        },
        items,
        |item| {
            text(item)
                .style(|s| s.height(28.0).padding_horiz(6.0).items_center())
                .into_any()
        },
    )
    .on_accept(move |selected| value.set(selected))
    .style(move |s| {
        let config = config.get();
        let border = config.color(SparrowColor::SPARROW_BORDER);
        let foreground = config.color(SparrowColor::EDITOR_FOREGROUND);
        let background = config.color(SparrowColor::PANEL_BACKGROUND);
        s.height(28.0)
            .min_width(0.0)
            .padding_horiz(6.0)
            .border(1.0)
            .border_radius(4.0)
            .border_color(border)
            .color(foreground)
            .background(background)
            .focus(|s| s.border_color(config.color(SparrowColor::EDITOR_CARET)))
            .class(DropdownChevronClass, |s| {
                s.size(14.0, 14.0)
                    .color(config.color(SparrowColor::SPARROW_ICON_ACTIVE))
            })
            .class(ScrollClass, move |s| {
                s.width_pct(100.0)
                    .height(list_height)
                    .font_size(config.ui.font_size() as f32)
                    .font_family(config.ui.font_family.clone())
                    .max_height(240.0)
                    .color(foreground)
                    .background(background)
                    .border(1.0)
                    .border_color(border)
                    .border_radius(4.0)
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_frame_does_not_trigger_floem_paint_translation() {
        for window in [Size::new(1920.0, 1080.0), Size::new(960.0, 540.0)] {
            let frame = overlay_frame_size(window);
            // These are the boundary checks in Floem OverlayView::paint.
            assert!(frame.width <= window.width - 5.0);
            assert!(frame.height <= window.height - 5.0);
        }
    }

    #[test]
    fn overlay_frame_handles_tiny_windows() {
        assert_eq!(overlay_frame_size(Size::new(3.0, 4.0)), Size::ZERO);
    }
}
