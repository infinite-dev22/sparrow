use std::{path::Path, sync::Arc};

use floem::{
    IntoView, Renderer, View, ViewId,
    context::PaintCx,
    kurbo::{BezPath, Stroke},
    reactive::{ReadSignal, SignalGet, SignalWith},
    views::Decorators,
};
use lsp_types::DiagnosticSeverity;
use sparrow_core::buffer::diff::DiffLines;
use sparrow_rpc::source_control::FileDiffKind;

use crate::{
    config::{SparrowConfig, color::SparrowColor},
    main_split::MainSplitData,
    source_control::SourceControlData,
};

pub(crate) fn git_color(
    path: &Path,
    source_control: &SourceControlData,
    docs: &MainSplitData,
) -> Option<&'static str> {
    if source_control.untracked.with(|paths| paths.contains(path)) {
        return Some(SparrowColor::SOURCE_CONTROL_UNTRACKED);
    }
    let kind = source_control
        .file_diffs
        .with(|diffs| diffs.get(path).map(|(diff, _)| diff.kind()));
    match kind {
        Some(FileDiffKind::Added) => Some(SparrowColor::SOURCE_CONTROL_ADDED),
        Some(FileDiffKind::Modified | FileDiffKind::Renamed) => {
            Some(SparrowColor::SOURCE_CONTROL_MODIFIED)
        }
        Some(FileDiffKind::Deleted) => Some(SparrowColor::SOURCE_CONTROL_REMOVED),
        None => docs
            .docs
            .with(|docs| docs.get(path).cloned())
            .filter(|doc| {
                doc.head_changes.with(|changes| {
                    changes
                        .iter()
                        .any(|change| !matches!(change, DiffLines::Both(_)))
                })
            })
            .map(|_| SparrowColor::SOURCE_CONTROL_MODIFIED),
    }
}

pub(crate) fn has_error(
    path: &Path,
    data: &MainSplitData,
    descendants: bool,
) -> bool {
    data.diagnostics.with(|diagnostics| {
        let has_errors = |data: &crate::doc::DiagnosticData| {
            data.diagnostics.with(|items| {
                items
                    .iter()
                    .any(|item| item.severity == Some(DiagnosticSeverity::ERROR))
            })
        };
        if descendants {
            diagnostics
                .iter()
                .any(|(file, data)| file.starts_with(path) && has_errors(data))
        } else {
            diagnostics.get(path).is_some_and(has_errors)
        }
    })
}

struct ErrorUnderline {
    id: ViewId,
    config: ReadSignal<Arc<SparrowConfig>>,
    visible: Box<dyn Fn() -> bool>,
}

impl View for ErrorUnderline {
    fn id(&self) -> ViewId {
        self.id
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        cx.paint_children(self.id);
        let Some(size) = self.id.get_size() else {
            return;
        };
        if !(self.visible)() {
            return;
        }
        let y = size.height - 2.0;
        let mut path = BezPath::new();
        path.move_to((0.0, y));
        for x in 1..=(size.width / 2.0) as usize {
            path.line_to((x as f64 * 2.0, y - (x % 2) as f64 * 1.5));
        }
        cx.stroke(
            &path,
            self.config
                .get_untracked()
                .color(SparrowColor::SPARROW_ERROR),
            &Stroke::new(1.0),
        );
    }
}

pub(crate) fn error_underline(
    child: impl IntoView + 'static,
    config: ReadSignal<Arc<SparrowConfig>>,
    visible: impl Fn() -> bool + 'static,
) -> impl View {
    let id = ViewId::new();
    id.set_children([child]);
    let visible = floem::reactive::create_memo(move |_| visible());
    floem::reactive::create_effect(move |_| {
        visible.get();
        config.get();
        id.request_paint();
    });
    ErrorUnderline {
        id,
        config,
        visible: Box::new(move || visible.get_untracked()),
    }
    .style(|s| s.min_width(0))
}
