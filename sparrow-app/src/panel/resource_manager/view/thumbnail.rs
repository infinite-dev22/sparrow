//! Resource thumbnails, color swatches and fallback file icons.

use crate::config::{SparrowConfig, color::SparrowColor};
use crate::panel::resource_manager::{Variant, color, vector};
use floem::{
    IntoView, View,
    reactive::{ReadSignal, RwSignal, SignalGet},
    views::{Decorators, empty, img_from_path, stack, svg},
};
use std::sync::Arc;

pub(super) fn resource_thumbnail(
    preview: Option<Variant>,
    size: RwSignal<f32>,
    config: ReadSignal<Arc<SparrowConfig>>,
) -> impl View {
    let image = if let Some(variant) = preview {
        if let Some(color) = color(&variant.value) {
            empty()
                .style(move |s| {
                    s.size(size.get(), size.get())
                        .background(color)
                        .border(1.0)
                        .border_color(
                            config.get().color(SparrowColor::SPARROW_BORDER),
                        )
                })
                .into_any()
        } else if matches!(
            variant.path.extension().and_then(|s| s.to_str()),
            Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp")
        ) {
            img_from_path(move || variant.path.clone())
                .style(move |s| s.size(size.get(), size.get()))
                .into_any()
        } else if variant.path.extension().is_some_and(|s| s == "svg") {
            svg(move || std::fs::read_to_string(&variant.path).unwrap_or_default())
                .style(move |s| s.size(size.get(), size.get()))
                .into_any()
        } else if let Some(vector) = vector(&variant.path) {
            svg(move || vector.clone())
                .style(move |s| s.size(size.get(), size.get()))
                .into_any()
        } else {
            svg(move || config.get().file_svg(&variant.path).0)
                .style(move |s| s.size(size.get(), size.get()))
                .into_any()
        }
    } else {
        empty().into_any()
    };

    stack((
                            svg(|| "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"128\" height=\"128\"><defs><pattern id=\"checks\" width=\"16\" height=\"16\" patternUnits=\"userSpaceOnUse\"><rect width=\"16\" height=\"16\" fill=\"#303234\"/><path d=\"M0 0h8v8H0zM8 8h8v8H8z\" fill=\"#3b3d3f\"/></pattern></defs><rect width=\"128\" height=\"128\" fill=\"url(#checks)\"/></svg>".to_string()).style(move |s|s.absolute().size(size.get(),size.get())),
                            image,
                        )).style(move |s|s.size(size.get(),size.get()).items_center().justify_center())
}
