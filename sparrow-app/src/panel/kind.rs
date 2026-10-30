use serde::{Deserialize, Serialize};
use strum_macros::EnumIter;

use super::{data::PanelOrder, position::PanelPosition};
use crate::config::icon::SparrowIcons;

#[derive(
    Clone, Copy, PartialEq, Serialize, Deserialize, Hash, Eq, Debug, EnumIter,
)]
pub enum PanelKind {
    Terminal,
    FileExplorer,
    SourceControl,
    Plugin,
    Search,
    Problem,
    Debug,
    CallHierarchy,
    DocumentSymbol,
    References,
    Implementation,
    WidgetTree,
    DartOutline,
    FlutterInspector,
}

impl PanelKind {
    pub fn svg_name(&self) -> &'static str {
        match &self {
            PanelKind::Terminal => SparrowIcons::TERMINAL,
            PanelKind::FileExplorer => SparrowIcons::FILE_EXPLORER,
            PanelKind::SourceControl => SparrowIcons::SCM,
            PanelKind::Plugin => SparrowIcons::EXTENSIONS,
            PanelKind::Search => SparrowIcons::SEARCH,
            PanelKind::Problem => SparrowIcons::PROBLEM,
            PanelKind::Debug => SparrowIcons::DEBUG,
            PanelKind::CallHierarchy => SparrowIcons::TYPE_HIERARCHY,
            PanelKind::DocumentSymbol => SparrowIcons::DOCUMENT_SYMBOL,
            PanelKind::References => SparrowIcons::REFERENCES,
            PanelKind::Implementation => SparrowIcons::IMPLEMENTATION,
            PanelKind::WidgetTree => SparrowIcons::TYPE_HIERARCHY,
            PanelKind::DartOutline => SparrowIcons::DOCUMENT_SYMBOL,
            PanelKind::FlutterInspector => SparrowIcons::DEBUG,
        }
    }

    pub fn position(&self, order: &PanelOrder) -> Option<(usize, PanelPosition)> {
        for (pos, panels) in order.iter() {
            let index = panels.iter().position(|k| k == self);
            if let Some(index) = index {
                return Some((index, *pos));
            }
        }
        None
    }

    pub fn default_position(&self) -> PanelPosition {
        match self {
            PanelKind::Terminal => PanelPosition::BottomLeft,
            PanelKind::FileExplorer => PanelPosition::LeftTop,
            PanelKind::SourceControl => PanelPosition::LeftTop,
            PanelKind::Plugin => PanelPosition::LeftTop,
            PanelKind::Search => PanelPosition::BottomLeft,
            PanelKind::Problem => PanelPosition::BottomLeft,
            PanelKind::Debug => PanelPosition::LeftTop,
            PanelKind::CallHierarchy => PanelPosition::BottomLeft,
            PanelKind::DocumentSymbol => PanelPosition::RightTop,
            PanelKind::References => PanelPosition::BottomLeft,
            PanelKind::Implementation => PanelPosition::BottomLeft,
            PanelKind::WidgetTree => PanelPosition::RightTop,
            PanelKind::DartOutline => PanelPosition::RightTop,
            PanelKind::FlutterInspector => PanelPosition::RightBottom,
        }
    }
}
