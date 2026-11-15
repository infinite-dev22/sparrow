use serde::{Deserialize, Serialize};
use strum_macros::EnumIter;

use super::{data::PanelOrder, position::PanelPosition};
use crate::config::icon::SparrowIcons;

#[derive(
    Clone, Copy, PartialEq, Serialize, Deserialize, Hash, Eq, Debug, EnumIter,
)]
pub enum PanelKind {
    Terminal,
    Notifications,
    PullRequests,
    RunningDevices,
    Problems,
    Logcat,
    GitHistory,
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
    FlutterRun,
    FlutterTest,
    DeviceManager,
    FlutterPerformance,
}

impl PanelKind {
    /// Saved layouts and old commands may still refer to the pre-Dart outline.
    pub fn canonical(self) -> Self {
        match self {
            Self::DocumentSymbol => Self::DartOutline,
            other => other,
        }
    }

    pub fn title(self) -> &'static str {
        match self.canonical() {
            Self::FileExplorer => "Project",
            Self::SourceControl => "Commit",
            Self::Plugin => "Themes",
            Self::Search => "Find",
            Self::Problem => "Dart Analysis",
            Self::Problems => "Problems",
            Self::Logcat => "Logcat",
            Self::GitHistory => "Git",
            Self::Terminal => "Terminal",
            Self::Debug => "Debug",
            Self::Notifications => "Notifications",
            Self::PullRequests => "Pull Requests",
            Self::RunningDevices => "Running Devices",
            Self::CallHierarchy => "Call Hierarchy",
            Self::References => "Find Usages",
            Self::Implementation => "Implementations",
            Self::WidgetTree => "Flutter Outline",
            Self::DartOutline | Self::DocumentSymbol => "Structure",
            Self::FlutterInspector => "Flutter Inspector",
            Self::FlutterRun => "Run",
            Self::FlutterTest => "Flutter Tests",
            Self::DeviceManager => "Device Manager",
            Self::FlutterPerformance => "Flutter Performance",
        }
    }

    /// Auxiliary tools stay in More Tool Windows until opened.
    pub fn pinned(self) -> bool {
        !matches!(
            self,
            Self::Plugin
                | Self::Search
                | Self::CallHierarchy
                | Self::References
                | Self::Implementation
                | Self::FlutterTest
                | Self::DocumentSymbol
        )
    }

    pub fn svg_name(&self) -> &'static str {
        match &self {
            PanelKind::Notifications => "notifications",
            PanelKind::PullRequests => "pull_requests",
            PanelKind::RunningDevices => "running_devices",
            PanelKind::Problems => SparrowIcons::ERROR,
            PanelKind::Logcat => "logcat",
            PanelKind::GitHistory => SparrowIcons::SCM,
            PanelKind::Terminal => SparrowIcons::TERMINAL,
            PanelKind::FileExplorer => SparrowIcons::FILE_EXPLORER,
            PanelKind::SourceControl => "commit",
            PanelKind::Plugin => SparrowIcons::SETTINGS,
            PanelKind::Search => SparrowIcons::SEARCH,
            PanelKind::Problem => "dart_analysis",
            PanelKind::Debug => SparrowIcons::DEBUG,
            PanelKind::CallHierarchy => SparrowIcons::TYPE_HIERARCHY,
            PanelKind::DocumentSymbol => SparrowIcons::DOCUMENT_SYMBOL,
            PanelKind::References => SparrowIcons::REFERENCES,
            PanelKind::Implementation => SparrowIcons::IMPLEMENTATION,
            PanelKind::WidgetTree => SparrowIcons::FLUTTER_OUTLINE,
            PanelKind::DartOutline => "structure",
            PanelKind::FlutterInspector => SparrowIcons::FLUTTER_INSPECTOR,
            PanelKind::FlutterRun => SparrowIcons::FLUTTER_RUN,
            PanelKind::FlutterTest => SparrowIcons::START,
            PanelKind::DeviceManager => SparrowIcons::DEVICE_MANAGER,
            PanelKind::FlutterPerformance => SparrowIcons::FLUTTER_PERFORMANCE,
        }
    }

    pub fn position(&self, order: &PanelOrder) -> Option<(usize, PanelPosition)> {
        for (pos, panels) in order.iter() {
            let index = panels.iter().position(|k| *k == self.canonical());
            if let Some(index) = index {
                return Some((index, *pos));
            }
        }
        None
    }

    pub fn default_position(&self) -> PanelPosition {
        match self {
            PanelKind::Problems | PanelKind::Logcat | PanelKind::GitHistory => {
                PanelPosition::BottomLeft
            }
            PanelKind::Notifications | PanelKind::RunningDevices => {
                PanelPosition::RightTop
            }
            PanelKind::PullRequests => PanelPosition::LeftTop,
            PanelKind::Terminal => PanelPosition::BottomLeft,
            PanelKind::FileExplorer => PanelPosition::LeftTop,
            PanelKind::SourceControl => PanelPosition::LeftTop,
            PanelKind::Plugin => PanelPosition::LeftTop,
            PanelKind::Search => PanelPosition::BottomLeft,
            PanelKind::Problem => PanelPosition::BottomLeft,
            PanelKind::Debug => PanelPosition::BottomLeft,
            PanelKind::CallHierarchy => PanelPosition::BottomLeft,
            PanelKind::DocumentSymbol => PanelPosition::RightTop,
            PanelKind::References => PanelPosition::BottomLeft,
            PanelKind::Implementation => PanelPosition::BottomLeft,
            PanelKind::WidgetTree => PanelPosition::RightTop,
            PanelKind::DartOutline => PanelPosition::RightTop,
            PanelKind::FlutterInspector => PanelPosition::RightTop,
            PanelKind::FlutterRun => PanelPosition::BottomLeft,
            PanelKind::FlutterTest => PanelPosition::BottomLeft,
            PanelKind::DeviceManager => PanelPosition::RightTop,
            PanelKind::FlutterPerformance => PanelPosition::RightTop,
        }
    }
}
