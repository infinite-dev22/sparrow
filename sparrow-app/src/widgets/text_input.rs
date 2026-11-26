//! Material 3 text field for floem 0.2.0.
//!
//! ```ignore
//! let name = RwSignal::new(String::new());
//!
//! TextField::new(name)
//!     .colors(move || m3_from_config(config))   // see your adapter
//!     .label("Project name")
//!     .supporting_text("Used in the window title")
//!     .max_length(40)
//!     .error_text(move || name.with(|n| n.contains('/').then(|| "No slashes".to_string())))
//!     .on_submit(|text| println!("submitted {text}"))
//! ```
//!
//! Implemented from the M3 spec: outlined and filled variants, floating label,
//! hover / focus / error / disabled states, leading and trailing icons,
//! supporting text and character counter.

use crate::config::SparrowConfig;
use crate::config::color::SparrowColor;
use floem::reactive::ReadSignal;
use floem::{
    AnyView, IntoView, ViewId,
    event::{Event, EventListener, EventPropagation},
    peniko::Brush,
    prelude::*,
    reactive::{RwSignal, SignalGet, SignalUpdate, SignalWith, create_effect},
    style::{CursorStyle, InsetLeft, InsetTop, Transition},
    views::{
        Decorators, PlaceholderTextClass, Stack, container, empty, h_stack, label,
        text_input, v_stack,
    },
};
use std::sync::Arc;
use std::{rc::Rc, time::Duration};

pub use super::editor_input::{TextInput, TextInputBuilder};
// ---------------------------------------------------------------------------
// Theme
// ---------------------------------------------------------------------------

/// The subset of the M3 colour scheme a text field needs.
#[derive(Clone, Copy, Debug)]
pub struct M3Colors {
    /// Focus indicator, caret, focused label.
    pub primary: Color,
    /// Input text.
    pub on_surface: Color,
    /// Resting label, placeholder, supporting text, icons.
    pub on_surface_variant: Color,
    /// Resting outline / bottom indicator.
    pub outline: Color,
    /// Colour behind the field (used to cut the outline under a floating label).
    pub surface: Color,
    /// Container fill of the *filled* variant.
    pub surface_container_highest: Color,
    /// Error state.
    pub error: Color,
}

impl M3Colors {
    /// Derives a full scheme from the four colours an editor theme usually has.
    /// Everything else is computed the way M3 does it: tonal blends of
    /// on-surface over surface.
    pub fn derive(
        surface: Color,
        on_surface: Color,
        primary: Color,
        outline: Color,
    ) -> Self {
        let dark = luminance(surface) < 0.5;
        Self {
            primary,
            on_surface,
            on_surface_variant: blend(surface, on_surface, 0.72),
            outline,
            surface,
            surface_container_highest: blend(surface, on_surface, 0.12),
            // M3 baseline error roles
            error: if dark {
                Color::from_rgb8(0xF2, 0xB8, 0xB5)
            } else {
                Color::from_rgb8(0xB3, 0x26, 0x1E)
            },
        }
    }
}

fn luminance(c: Color) -> f64 {
    let rgba = c.to_rgba8();
    (0.2126 * rgba.r as f64 + 0.7152 * rgba.g as f64 + 0.0722 * rgba.b as f64)
        / 255.0
}

/// `over` painted on `base` at `amount` (0..=1). Result is opaque.
fn blend(base: Color, over: Color, amount: f64) -> Color {
    let mix =
        |b: u8, o: u8| (b as f64 + (o as f64 - b as f64) * amount).round() as u8;

    let base_rgba = base.to_rgba8();
    let over_rgba = over.to_rgba8();

    Color::from_rgb8(
        mix(base_rgba.r, over_rgba.r),
        mix(base_rgba.g, over_rgba.g),
        mix(base_rgba.b, over_rgba.b),
    )
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TextFieldVariant {
    Filled,
    #[default]
    Outlined,
}

pub struct TextField {
    value: RwSignal<String>,
    colors: Rc<dyn Fn() -> M3Colors>,
    variant: TextFieldVariant,
    label: Option<String>,
    placeholder: Option<String>,
    supporting_text: Option<String>,
    error_text: Option<Rc<dyn Fn() -> Option<String>>>,
    error: Option<Rc<dyn Fn() -> bool>>,
    disabled: Option<Rc<dyn Fn() -> bool>>,
    max_length: Option<usize>,
    leading_icon: Option<AnyView>,
    trailing_icon: Option<AnyView>,
    on_submit: Option<Rc<dyn Fn(String)>>,
    on_change: Option<Rc<dyn Fn(String)>>,
    focus_when: Option<Rc<dyn Fn()>>,
    input_events: Vec<(
        EventListener,
        Box<dyn FnMut(&Event) -> EventPropagation>,
    )>,
}

impl TextField {
    pub fn new(value: RwSignal<String>) -> Self {
        // Neutral dark fallback so the widget is usable before a theme is set.
        let fallback = M3Colors::derive(
            Color::from_rgb8(0x14, 0x12, 0x18),
            Color::from_rgb8(0xE6, 0xE0, 0xE9),
            Color::from_rgb8(0xD0, 0xBC, 0xFF),
            Color::from_rgb8(0x93, 0x8F, 0x99),
        );
        Self {
            value,
            colors: Rc::new(move || fallback),
            variant: TextFieldVariant::Outlined,
            label: None,
            placeholder: None,
            supporting_text: None,
            error_text: None,
            error: None,
            disabled: None,
            max_length: None,
            leading_icon: None,
            trailing_icon: None,
            on_submit: None,
            on_change: None,
            focus_when: None,
            input_events: Vec::new(),
        }
    }

    /// Reactive colour source. Read signals inside it and the field restyles itself.
    pub fn colors(mut self, colors: impl Fn() -> M3Colors + 'static) -> Self {
        self.colors = Rc::new(colors);
        self
    }

    pub fn filled(mut self) -> Self {
        self.variant = TextFieldVariant::Filled;
        self
    }

    pub fn outlined(mut self) -> Self {
        self.variant = TextFieldVariant::Outlined;
        self
    }

    pub fn variant(mut self, variant: TextFieldVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Floating label. Rests inside the field, moves to the top on focus or when there is text.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Hint text. Like M3, it only shows while the label is floated (or there is no label).
    pub fn placeholder(mut self, text: impl Into<String>) -> Self {
        self.placeholder = Some(text.into());
        self
    }

    pub fn supporting_text(mut self, text: impl Into<String>) -> Self {
        self.supporting_text = Some(text.into());
        self
    }

    /// `Some(message)` puts the field in the error state and replaces the supporting text.
    pub fn error_text(mut self, f: impl Fn() -> Option<String> + 'static) -> Self {
        self.error_text = Some(Rc::new(f));
        self
    }

    /// Error state without a message.
    pub fn error(mut self, f: impl Fn() -> bool + 'static) -> Self {
        self.error = Some(Rc::new(f));
        self
    }

    pub fn disabled(mut self, f: impl Fn() -> bool + 'static) -> Self {
        self.disabled = Some(Rc::new(f));
        self
    }

    /// Shows a `len/max` counter and truncates input beyond `max` characters.
    pub fn max_length(mut self, max: usize) -> Self {
        self.max_length = Some(max);
        self
    }

    /// Any 24x24 view (an `svg(..)`, a glyph label...).
    pub fn leading_icon(mut self, icon: impl IntoView + 'static) -> Self {
        self.leading_icon = Some(icon.into_any());
        self
    }

    pub fn trailing_icon(mut self, icon: impl IntoView + 'static) -> Self {
        self.trailing_icon = Some(icon.into_any());
        self
    }

    /// Focus the field once when it is built.
    pub fn autofocus(self) -> Self {
        self.focus_when(|| {})
    }

    /// Focus the field now, and again whenever a signal read inside `when` changes.
    ///
    /// ```ignore
    /// let focus = RwSignal::new(0);
    /// TextField::new(q).focus_when(move || { focus.track(); });
    /// // later, from a shortcut or button:
    /// focus.update(|n| *n += 1);
    /// ```
    pub fn focus_when(mut self, when: impl Fn() + 'static) -> Self {
        self.focus_when = Some(Rc::new(when));
        self
    }

    /// Called with the current text when Enter is pressed.
    pub fn on_submit(mut self, f: impl Fn(String) + 'static) -> Self {
        self.on_submit = Some(Rc::new(f));
        self
    }

    /// Called whenever the text changes (not for the initial value).
    pub fn on_change(mut self, f: impl Fn(String) + 'static) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }

    /// Attach an event listener directly to the inner text input view.
    pub(crate) fn on_input_event(
        mut self,
        listener: EventListener,
        action: impl FnMut(&Event) -> EventPropagation + 'static,
    ) -> Self {
        self.input_events.push((listener, Box::new(action)));
        self
    }
}

// ---------------------------------------------------------------------------
// Spec constants (dp)
// ---------------------------------------------------------------------------


const HEIGHT: f32 = 28.0;
const RADIUS: f64 = 4.0;
const SIDE: f32 = 8.0;
const ICON_SIDE: f32 = 6.0;
const ICON: f32 = 28.0;
const ICON_GAP: f32 = 6.0;
const BODY: f32 = 13.0;
const LABEL_SMALL: f32 = 12.0;
const LABEL_NOTCH_PAD: f32 = 4.0;

const DISABLED_CONTENT: f32 = 0.38;
const DISABLED_OUTLINE: f32 = 0.12;
const HOVER_LAYER: f64 = 0.08;

fn anim() -> Transition {
    Transition::ease_in_out(Duration::from_millis(150))
}

// ---------------------------------------------------------------------------
// Shared state, cloned into each style closure
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct Ctx {
    value: RwSignal<String>,
    focused: RwSignal<bool>,
    colors: Rc<dyn Fn() -> M3Colors>,
    error_text: Option<Rc<dyn Fn() -> Option<String>>>,
    error: Option<Rc<dyn Fn() -> bool>>,
    disabled: Option<Rc<dyn Fn() -> bool>>,
    variant: TextFieldVariant,
    has_label: bool,
    leading: bool,
}

impl Ctx {
    fn c(&self) -> M3Colors {
        (self.colors)()
    }

    fn is_disabled(&self) -> bool {
        self.disabled.as_ref().is_some_and(|f| f())
    }

    fn is_error(&self) -> bool {
        self.error.as_ref().is_some_and(|f| f())
            || self.error_text.as_ref().is_some_and(|f| f().is_some())
    }

    fn is_focused(&self) -> bool {
        self.focused.get() && !self.is_disabled()
    }

    /// Hover feedback applies only while the field is idle.
    fn hoverable(&self) -> bool {
        !self.is_disabled() && !self.is_focused() && !self.is_error()
    }

    /// Label is in the "small, on the top edge" position.
    fn floated(&self) -> bool {
        self.focused.get() || self.value.with(|v| !v.is_empty())
    }

    /// Thickness of the outline (or filled indicator) in the current state.
    fn stroke(&self) -> f32 {
        if self.is_focused() { 2.0 } else { 1.0 }
    }

    fn text_x(&self) -> f32 {
        if self.leading {
            ICON_SIDE + ICON + ICON_GAP
        } else {
            SIDE
        }
    }

    fn input_color(&self) -> Color {
        let c = self.c();
        if self.is_disabled() {
            c.on_surface.multiply_alpha(DISABLED_CONTENT)
        } else {
            c.on_surface
        }
    }

    fn outline_color(&self) -> Color {
        let c = self.c();
        if self.is_disabled() {
            c.on_surface.multiply_alpha(DISABLED_OUTLINE)
        } else if self.is_error() {
            c.error
        } else if self.is_focused() {
            c.primary
        } else {
            c.outline
        }
    }

    fn label_color(&self) -> Color {
        let c = self.c();
        if self.is_disabled() {
            c.on_surface.multiply_alpha(DISABLED_CONTENT)
        } else if self.is_error() {
            c.error
        } else if self.is_focused() {
            c.primary
        } else {
            c.on_surface_variant
        }
    }

    fn support_color(&self) -> Color {
        let c = self.c();
        if self.is_disabled() {
            c.on_surface.multiply_alpha(DISABLED_CONTENT)
        } else if self.is_error() {
            c.error
        } else {
            c.on_surface_variant
        }
    }

    fn icon_color(&self) -> Color {
        let c = self.c();
        if self.is_disabled() {
            c.on_surface.multiply_alpha(DISABLED_CONTENT)
        } else if self.is_error() {
            c.error
        } else {
            c.on_surface_variant
        }
    }

    fn fill(&self, hovered_layer: bool) -> Color {
        let c = self.c();
        if self.is_disabled() {
            blend(c.surface, c.on_surface, 0.04)
        } else if hovered_layer {
            blend(c.surface_container_highest, c.on_surface, HOVER_LAYER)
        } else {
            c.surface_container_highest
        }
    }
}

// ---------------------------------------------------------------------------
// View construction
// ---------------------------------------------------------------------------

impl IntoView for TextField {
    type V = Stack;

    fn into_view(self) -> Stack {
        self.build_with_input_id().0
    }
}

impl TextField {
    pub(crate) fn build_with_input_id(self) -> (Stack, ViewId) {
        let TextField {
            value,
            colors,
            variant,
            label: label_text,
            placeholder,
            supporting_text,
            error_text,
            error,
            disabled,
            max_length,
            leading_icon,
            trailing_icon,
            on_submit,
            on_change,
            focus_when,
            input_events,
        } = self;

        let focused = RwSignal::new(false);
        let hovered = RwSignal::new(false);
        let ctx = Ctx {
            value,
            focused,
            colors,
            error_text,
            error,
            disabled,
            variant,
            has_label: label_text.is_some(),
            leading: leading_icon.is_some(),
        };

        // --- behaviour -----------------------------------------------------

        if let Some(max) = max_length {
            create_effect(move |_| {
                let over = value.with(|v| v.chars().count() > max);
                if over {
                    value.update(|v| *v = v.chars().take(max).collect());
                }
            });
        }

        if let Some(f) = on_change {
            create_effect(move |prev: Option<()>| {
                let current = value.get();
                if prev.is_some() {
                    f(current);
                }
            });
        }

        // --- the native input ----------------------------------------------

        let mut input = text_input(value);
        if let Some(p) = placeholder {
            input = input.placeholder(p);
        }
        let input_id = input.id();

        let ictx = ctx.clone();
        let mut input = input
            .on_event_cont(EventListener::FocusGained, move |_| focused.set(true))
            .on_event_cont(EventListener::FocusLost, move |_| focused.set(false))
            .style(move |s| {
                let c = ictx.c();
                // With a label the placeholder only appears once the label has floated.
                let placeholder_visible = !ictx.has_label || ictx.floated();
                let placeholder_color = if placeholder_visible {
                    if ictx.is_disabled() {
                        c.on_surface.multiply_alpha(DISABLED_CONTENT)
                    } else {
                        c.on_surface_variant
                    }
                } else {
                    Color::TRANSPARENT
                };
                let filled_with_label =
                    ictx.variant == TextFieldVariant::Filled && ictx.has_label;
                s.flex_grow(1.0_f32)
                    .min_width(0.0)
                    .height(ICON)
                    .margin_top(if filled_with_label { 16.0 } else { 0.0 })
                    .items_center()
                    .padding_horiz(6.0)
                    .padding_vert(0.0)
                    .border(0.0)
                    .border_radius(0.0)
                    .background(Color::TRANSPARENT)
                    .font_size(BODY)
                    .color(ictx.input_color())
                    .cursor_color(Brush::Solid(c.primary))
                    .cursor(if ictx.is_disabled() {
                        CursorStyle::Default
                    } else {
                        CursorStyle::Text
                    })
                    // Some themes add a focus ring to inputs; the field draws its own.
                    .focus(|s| s.border(0.0).padding(0.0))
                    .class(PlaceholderTextClass, move |s| {
                        s.color(placeholder_color).font_size(BODY)
                    })
            });

        if let Some(when) = focus_when {
            input = input.request_focus(move || when());
        }

        if let Some(f) = on_submit {
            input = input.on_enter(move || f(value.get_untracked()));
        }

        for (listener, action) in input_events {
            input_id.add_event_listener(listener, action);
        }

        // --- floating label ------------------------------------------------

        let label_view: AnyView = match label_text {
            None => empty().into_any(),
            Some(text) => {
                let lctx = ctx.clone();
                let ltx = ctx.clone();
                container(label(move || text.clone()).style(move |s| {
                    s.font_size(if ltx.floated() { LABEL_SMALL } else { BODY })
                        .color(ltx.label_color())
                        .padding_horiz(LABEL_NOTCH_PAD)
                        .background(Color::TRANSPARENT)
                }))
                .style(move |s| {
                    let b = lctx.stroke();
                    let floated = lctx.floated();
                    let outlined = lctx.variant == TextFieldVariant::Outlined;
                    let c = lctx.c();

                    let height = if floated { 16.0 } else { ICON };
                    // Offsets are measured from the padding box, i.e. inside the border.
                    let (top, left) = match (outlined, floated) {
                        // resting, vertically centred
                        (_, false) => {
                            ((HEIGHT - 2.0 * b - ICON) / 2.0, lctx.text_x() - b)
                        }
                        // outlined: straddle the top border, start at the 16dp edge
                        (true, true) => (-(8.0 + b / 2.0), SIDE - b),
                        // filled: top of the container, aligned with the text
                        (false, true) => (8.0, lctx.text_x() - b),
                    };
                    let notch = outlined && floated;
                    s.absolute()
                        .inset_top(top)
                        .inset_left(left - LABEL_NOTCH_PAD)
                        .height(height)
                        .items_center()
                        .background(if notch {
                            c.surface
                        } else {
                            Color::TRANSPARENT
                        })
                        .transition(InsetTop, anim())
                        .transition(InsetLeft, anim())
                        .pointer_events_none()
                })
                .into_any()
            }
        };

        // --- icons ---------------------------------------------------------

        let icon_slot = |icon: Option<AnyView>, ctx: Ctx| -> AnyView {
            match icon {
                None => empty().into_any(),
                Some(icon) => container(icon)
                    .style(move |s| {
                        s.size(ICON, ICON)
                            .flex_shrink(0.0_f32)
                            .items_center()
                            .justify_center()
                            .color(ctx.icon_color())
                    })
                    .into_any(),
            }
        };
        let leading = icon_slot(leading_icon, ctx.clone());
        let trailing = icon_slot(trailing_icon, ctx.clone());

        // --- filled variant: container shape -------------------------------
        // floem 0.2.0 has a single border radius, so the "rounded top, square
        // bottom" container is two stacked fills: a rounded one and a square
        // one covering the lower half.

        let fill_layers: AnyView = if variant == TextFieldVariant::Filled {
            let f1 = ctx.clone();
            let f2 = ctx.clone();
            let hovered1 = hovered;
            let hovered2 = hovered;
            let rounded = empty().style(move |s| {
                s.absolute()
                    .inset(0.0)
                    .size_full()
                    .border_radius(RADIUS)
                    .background(f1.fill(hovered1.get() && f1.hoverable()))
            });
            let square = empty().style(move |s| {
                s.absolute()
                    .inset_left(0.0)
                    .inset_right(0.0)
                    .inset_bottom(0.0)
                    .height(HEIGHT / 2.0)
                    .background(f2.fill(hovered2.get() && f2.hoverable()))
            });
            stack((rounded, square))
                .style(|s| s.absolute().inset(0.0).size_full())
                .into_any()
        } else {
            empty().into_any()
        };

        // --- the field box -------------------------------------------------

        let leading_present = ctx.leading;
        let bctx = ctx.clone();
        let field_box = h_stack((fill_layers, leading, input, trailing, label_view))
            .on_click_stop(move |_| input_id.request_focus())
            .on_event_cont(EventListener::PointerEnter, move |_| hovered.set(true))
            .on_event_cont(EventListener::PointerLeave, move |_| hovered.set(false))
            .style(move |s| {
                let b = bctx.stroke();
                let c = bctx.c();
                let filled = bctx.variant == TextFieldVariant::Filled;
                let hot = hovered.get() && bctx.hoverable();
                let outline = if hot {
                    c.on_surface
                } else {
                    bctx.outline_color()
                };

                let pad_left = if leading_present { ICON_SIDE } else { SIDE };
                let s = s
                    .width_full()
                    .min_width(0.0)
                    .height(HEIGHT)
                    .items_center()
                    .gap(if leading_present { ICON_GAP } else { 0.0 })
                    .background(Color::TRANSPARENT)
                    .border_color(outline)
                    .cursor(if bctx.is_disabled() {
                        CursorStyle::Default
                    } else {
                        CursorStyle::Text
                    });

                if filled {
                    // Only a bottom indicator; padding is constant.
                    s.border_bottom(b)
                        .padding_left(pad_left)
                        .padding_right(SIDE)
                } else {
                    // Border + padding stays constant so text never shifts on focus.
                    s.border(b)
                        .border_radius(RADIUS)
                        .padding_left(pad_left - b)
                        .padding_right(SIDE - b)
                }
            });

        // --- supporting row ------------------------------------------------

        let counter = max_length;
        let show_row = supporting_text.is_some()
            || ctx.error_text.is_some()
            || counter.is_some();

        let row: AnyView = if !show_row {
            empty().into_any()
        } else {
            let sctx = ctx.clone();
            let supporting = label(move || {
                sctx.error_text
                    .as_ref()
                    .and_then(|f| f())
                    .or_else(|| supporting_text.clone())
                    .unwrap_or_default()
            })
            .style({
                let sctx = ctx.clone();
                move |s| {
                    s.flex_grow(1.0_f32)
                        .min_width(0.0)
                        .font_size(LABEL_SMALL)
                        .color(sctx.support_color())
                }
            });

            let counter_view: AnyView = match counter {
                None => empty().into_any(),
                Some(max) => {
                    let cctx = ctx.clone();
                    label(move || {
                        format!("{}/{}", value.with(|v| v.chars().count()), max)
                    })
                    .style(move |s| {
                        s.flex_shrink(0.0_f32)
                            .margin_left(16.0)
                            .font_size(LABEL_SMALL)
                            .color(cctx.support_color())
                    })
                    .into_any()
                }
            };

            h_stack((supporting, counter_view))
                .style(|s| {
                    s.width_full()
                        .min_width(0.0)
                        .padding_horiz(SIDE)
                        .padding_top(4.0)
                })
                .into_any()
        };

        let fctx = ctx.clone();
        let octx = ctx;
        let view = v_stack((field_box, row))
            .on_event_cont(EventListener::FocusGained, move |_| {
                if !fctx.is_disabled() {
                    input_id.request_focus();
                }
            })
            .style(|s| s.width_full().min_width(0.0))
            .disabled(move || octx.is_disabled());
        (view, input_id)
    }
}

pub fn m3_from_config(config: ReadSignal<Arc<SparrowConfig>>) -> M3Colors {
    let c = config.get();
    M3Colors::derive(
        c.color(SparrowColor::EDITOR_BACKGROUND), // surface — verify this name exists
        c.color(SparrowColor::EDITOR_FOREGROUND),
        c.color(SparrowColor::EDITOR_CARET),
        c.color(SparrowColor::SPARROW_BORDER),
    )
}
