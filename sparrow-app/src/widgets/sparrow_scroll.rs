use floem::{
    IntoView,
    views::{Scroll, scroll},
};

/// A Floem scroll view with Sparrow's scrollbar defaults.
///
/// Callers can use Floem's `scroll_style`, `on_scroll`, and
/// `ensure_visible` methods to customize individual views.
pub fn sparrow_scroll(child: impl IntoView + 'static) -> Scroll {
    scroll(child).scroll_style(|s| {
        s.handle_thickness(8)
            .handle_rounded(true)
            .track_thickness(8)
            .track_rounded(true)
    })
}

/// A scroll view that maps vertical wheel input to horizontal scrolling.
pub fn horizontal_scroll(child: impl IntoView + 'static) -> Scroll {
    sparrow_scroll(child).scroll_style(|s| s.vertical_scroll_as_horizontal(true))
}
