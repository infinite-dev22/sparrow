use floem::{
    View,
    style::FlexWrap,
    view_tuple::ViewTuple,
    views::{Decorators, h_stack},
};

/// Keeps related controls together and lets them wrap in narrow dialogs.
pub(crate) fn action_group(controls: impl ViewTuple + 'static) -> impl View {
    h_stack(controls).style(|s| s.items_center().gap(8.0).flex_wrap(FlexWrap::Wrap))
}
