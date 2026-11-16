use std::{rc::Rc, sync::Arc};

use floem::{
    kurbo::Size,
    reactive::{
        Memo, RwSignal, Scope, SignalGet, SignalUpdate, SignalWith, use_context,
    },
};
use serde::{Deserialize, Serialize};

use super::{
    kind::PanelKind,
    position::{PanelContainerPosition, PanelPosition},
    style::PanelStyle,
};
use crate::{
    db::SparrowDb,
    window_tab::{CommonData, Focus},
};

pub type PanelOrder = im::HashMap<PanelPosition, im::Vector<PanelKind>>;

pub fn default_panel_order() -> PanelOrder {
    let mut order = PanelOrder::new();
    order.insert(
        PanelPosition::LeftTop,
        im::vector![
            PanelKind::FileExplorer,
            PanelKind::Plugin,
            PanelKind::SourceControl,
            PanelKind::PullRequests,
        ],
    );
    order.insert(
        PanelPosition::BottomLeft,
        im::vector![
            PanelKind::FlutterRun,
            PanelKind::Debug,
            PanelKind::FlutterTest,
            PanelKind::Problem,
            PanelKind::Problems,
            PanelKind::Logcat,
            PanelKind::GitHistory,
            PanelKind::Terminal,
            PanelKind::Search,
            PanelKind::CallHierarchy,
            PanelKind::References,
            PanelKind::Implementation,
        ],
    );
    order.insert(
        PanelPosition::RightTop,
        im::vector![
            PanelKind::Notifications,
            PanelKind::RunningDevices,
            PanelKind::FlutterInspector,
            PanelKind::FlutterPerformance,
            PanelKind::DeviceManager,
            PanelKind::WidgetTree,
            PanelKind::DartOutline,
        ],
    );

    order
}

#[derive(Clone, Copy, Serialize, Deserialize, Hash, PartialEq, Eq)]
pub enum PanelSection {
    OpenEditor,
    FileExplorer,
    Error,
    Warn,
    Lint,
    Changes,
    Installed,
    Available,
    Process,
    Variable,
    StackFrame,
    Breakpoint,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct PanelSize {
    pub left: f64,
    pub left_split: f64,
    pub bottom: f64,
    pub bottom_split: f64,
    pub right: f64,
    pub right_split: f64,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct PanelInfo {
    pub panels: PanelOrder,
    pub styles: im::HashMap<PanelPosition, PanelStyle>,
    pub size: PanelSize,
    pub sections: im::HashMap<PanelSection, bool>,
}

#[derive(Clone)]
pub struct PanelData {
    pub panels: RwSignal<PanelOrder>,
    pub styles: RwSignal<im::HashMap<PanelPosition, PanelStyle>>,
    pub size: RwSignal<PanelSize>,
    pub(crate) hidden_styles:
        RwSignal<Option<im::HashMap<PanelPosition, PanelStyle>>>,
    pub last_opened: RwSignal<Option<PanelKind>>,
    pub available_size: Memo<Size>,
    pub sections: RwSignal<im::HashMap<PanelSection, RwSignal<bool>>>,
    pub common: Rc<CommonData>,
}

impl PanelData {
    pub fn new(
        cx: Scope,
        panels: im::HashMap<PanelPosition, im::Vector<PanelKind>>,
        available_size: Memo<Size>,
        sections: im::HashMap<PanelSection, bool>,
        common: Rc<CommonData>,
    ) -> Self {
        let mut styles = im::HashMap::new();
        styles.insert(
            PanelPosition::LeftTop,
            PanelStyle {
                active: 0,
                shown: true,
                maximized: false,
            },
        );
        styles.insert(
            PanelPosition::LeftBottom,
            PanelStyle {
                active: 0,
                shown: false,
                maximized: false,
            },
        );
        styles.insert(
            PanelPosition::BottomLeft,
            PanelStyle {
                active: 0,
                shown: false,
                maximized: false,
            },
        );
        styles.insert(
            PanelPosition::BottomRight,
            PanelStyle {
                active: 0,
                shown: false,
                maximized: false,
            },
        );
        styles.insert(
            PanelPosition::RightTop,
            PanelStyle {
                active: 0,
                shown: false,
                maximized: false,
            },
        );
        styles.insert(
            PanelPosition::RightBottom,
            PanelStyle {
                active: 0,
                shown: false,
                maximized: false,
            },
        );
        let (panels, styles) = normalize_panel_layout(panels, styles);
        let panels = cx.create_rw_signal(panels);
        let styles = cx.create_rw_signal(styles);
        let size = cx.create_rw_signal(PanelSize {
            left: 420.0,
            left_split: 0.5,
            bottom: 342.0,
            bottom_split: 0.5,
            right: 466.0,
            right_split: 0.5,
        });
        let sections = cx.create_rw_signal(
            sections
                .into_iter()
                .map(|(key, value)| (key, cx.create_rw_signal(value)))
                .collect(),
        );

        Self {
            panels,
            styles,
            size,
            hidden_styles: cx.create_rw_signal(None),
            last_opened: cx.create_rw_signal(Some(PanelKind::FileExplorer)),
            available_size,
            sections,
            common,
        }
    }

    pub fn toggle_all_tool_windows(&self) {
        let current = self.styles.get_untracked();
        let (next, saved) =
            toggle_tool_windows(current, self.hidden_styles.get_untracked());
        self.hidden_styles.set(saved);
        self.styles.set(next);
    }

    pub fn panel_info(&self) -> PanelInfo {
        PanelInfo {
            panels: self.panels.get_untracked(),
            styles: self.styles.get_untracked(),
            size: self.size.get_untracked(),
            sections: self
                .sections
                .get_untracked()
                .into_iter()
                .map(|(key, value)| (key, value.get_untracked()))
                .collect(),
        }
    }

    pub fn is_container_shown(
        &self,
        position: &PanelContainerPosition,
        tracked: bool,
    ) -> bool {
        self.is_position_shown(&position.first(), tracked)
            || self.is_position_shown(&position.second(), tracked)
    }

    pub fn is_position_empty(
        &self,
        position: &PanelPosition,
        tracked: bool,
    ) -> bool {
        if tracked {
            self.panels
                .with(|panels| panels.get(position).map(|p| p.is_empty()))
                .unwrap_or(true)
        } else {
            self.panels
                .with_untracked(|panels| panels.get(position).map(|p| p.is_empty()))
                .unwrap_or(true)
        }
    }

    pub fn is_position_shown(
        &self,
        position: &PanelPosition,
        tracked: bool,
    ) -> bool {
        let styles = if tracked {
            self.styles.get()
        } else {
            self.styles.get_untracked()
        };
        styles.get(position).map(|s| s.shown).unwrap_or(false)
    }

    pub fn panel_position(
        &self,
        kind: &PanelKind,
    ) -> Option<(usize, PanelPosition)> {
        self.panels
            .with_untracked(|panels| panel_position(panels, &kind.canonical()))
    }

    pub fn is_panel_visible(&self, kind: &PanelKind) -> bool {
        if let Some((index, position)) = self.panel_position(kind) {
            if let Some(style) = self
                .styles
                .with_untracked(|styles| styles.get(&position).cloned())
            {
                return style.active == index && style.shown;
            }
        }
        false
    }

    pub fn show_panel(&self, kind: &PanelKind) {
        self.last_opened.set(Some(kind.canonical()));
        if let Some((index, position)) = self.panel_position(kind) {
            self.styles.update(|styles| {
                if let Some(style) = styles.get_mut(&position) {
                    style.shown = true;
                    style.active = index;
                }
            });
        }
    }

    pub fn hide_panel(&self, kind: &PanelKind) {
        if let Some((_, position)) = self.panel_position(kind) {
            if let Some((active_panel, _)) =
                self.active_panel_at_position(&position, false)
            {
                if active_panel == kind.canonical() {
                    self.set_shown(&position, false);
                    if self.common.focus.with_untracked(|focus| {
                        matches!(focus, Focus::Panel(active) if active.canonical() == kind.canonical())
                    }) {
                        self.common.focus.set(Focus::Workbench);
                    }
                    let peer_position = position.peer();
                    if let Some(order) = self
                        .panels
                        .with_untracked(|panels| panels.get(&peer_position).cloned())
                    {
                        if order.is_empty() {
                            self.set_shown(&peer_position, false);
                        }
                    }
                }
            }
        }
    }

    /// Get the active panel kind at that position, if any.  
    /// `tracked` decides whether it should track the signal or not.
    pub fn active_panel_at_position(
        &self,
        position: &PanelPosition,
        tracked: bool,
    ) -> Option<(PanelKind, bool)> {
        let style = if tracked {
            self.styles.with(|styles| styles.get(position).cloned())?
        } else {
            self.styles
                .with_untracked(|styles| styles.get(position).cloned())?
        };
        let order = if tracked {
            self.panels.with(|panels| panels.get(position).cloned())?
        } else {
            self.panels
                .with_untracked(|panels| panels.get(position).cloned())?
        };
        order
            .get(style.active)
            .cloned()
            .or_else(|| order.last().cloned())
            .map(|p| (p, style.shown))
    }

    pub fn set_shown(&self, position: &PanelPosition, shown: bool) {
        self.styles.update(|styles| {
            if let Some(style) = styles.get_mut(position) {
                style.shown = shown;
            }
        });
    }

    pub fn toggle_active_maximize(&self) {
        let focus = self.common.focus.get_untracked();
        if let Focus::Panel(kind) = focus {
            if let Some((_, pos)) = self.panel_position(&kind) {
                if pos.is_bottom() {
                    self.toggle_bottom_maximize();
                }
            }
        }
    }

    pub fn toggle_maximize(&self, kind: &PanelKind) {
        if let Some((_, p)) = self.panel_position(kind) {
            if p.is_bottom() {
                self.toggle_bottom_maximize();
            }
        }
    }

    pub fn toggle_bottom_maximize(&self) {
        let maximized = !self.panel_bottom_maximized(false);
        self.styles.update(|styles| {
            if let Some(style) = styles.get_mut(&PanelPosition::BottomLeft) {
                style.maximized = maximized;
            }
            if let Some(style) = styles.get_mut(&PanelPosition::BottomRight) {
                style.maximized = maximized;
            }
        });
    }

    pub fn panel_bottom_maximized(&self, tracked: bool) -> bool {
        let styles = if tracked {
            self.styles.get()
        } else {
            self.styles.get_untracked()
        };
        styles
            .get(&PanelPosition::BottomLeft)
            .map(|p| p.maximized)
            .unwrap_or(false)
            || styles
                .get(&PanelPosition::BottomRight)
                .map(|p| p.maximized)
                .unwrap_or(false)
    }

    pub fn toggle_container_visual(&self, position: &PanelContainerPosition) {
        let is_hidden = !self.is_container_shown(position, false);
        if is_hidden {
            self.styles.update(|styles| {
                let style = styles.entry(position.first()).or_default();
                style.shown = true;
                let style = styles.entry(position.second()).or_default();
                style.shown = true;
            });
        } else {
            if let Some((kind, _)) =
                self.active_panel_at_position(&position.second(), false)
            {
                self.hide_panel(&kind);
            }
            if let Some((kind, _)) =
                self.active_panel_at_position(&position.first(), false)
            {
                self.hide_panel(&kind);
            }
            self.styles.update(|styles| {
                let style = styles.entry(position.first()).or_default();
                style.shown = false;
                let style = styles.entry(position.second()).or_default();
                style.shown = false;
            });
        }
    }

    pub fn move_panel_to_position(&self, kind: PanelKind, position: &PanelPosition) {
        let (panels, styles) = relocate_panel(
            self.panels.get_untracked(),
            self.styles.get_untracked(),
            kind,
            *position,
        );
        self.panels.set(panels);
        self.styles.set(styles);

        let db: Arc<SparrowDb> = use_context().unwrap();
        db.save_panel_orders(self.panels.get_untracked());
    }

    pub fn section_open(&self, section: PanelSection) -> RwSignal<bool> {
        let open = self
            .sections
            .with_untracked(|sections| sections.get(&section).cloned());
        if let Some(open) = open {
            return open;
        }

        let open = self.common.scope.create_rw_signal(true);
        self.sections.update(|sections| {
            sections.insert(section, open);
        });
        open
    }
}

pub fn panel_position(
    order: &PanelOrder,
    kind: &PanelKind,
) -> Option<(usize, PanelPosition)> {
    for (pos, panels) in order.iter() {
        let index = panels.iter().position(|k| *k == kind.canonical());
        if let Some(index) = index {
            return Some((index, *pos));
        }
    }
    None
}

/// Stable iteration order matters: im::HashMap iteration must never decide
/// which of two saved copies owns a tool window.
const POSITIONS: [PanelPosition; 6] = [
    PanelPosition::LeftTop,
    PanelPosition::LeftBottom,
    PanelPosition::BottomLeft,
    PanelPosition::BottomRight,
    PanelPosition::RightTop,
    PanelPosition::RightBottom,
];

pub fn normalize_panel_layout(
    order: PanelOrder,
    mut styles: im::HashMap<PanelPosition, PanelStyle>,
) -> (PanelOrder, im::HashMap<PanelPosition, PanelStyle>) {
    use std::collections::HashMap;
    use strum::IntoEnumIterator;
    let mut owners = HashMap::new();
    for position in POSITIONS {
        if let Some(items) = order.get(&position) {
            for (index, kind) in items.iter().enumerate() {
                let kind = kind.canonical();
                let visible = styles
                    .get(&position)
                    .is_some_and(|style| style.shown && style.active == index);
                let priority = if visible {
                    2
                } else if position == kind.default_position() {
                    1
                } else {
                    0
                };
                if owners.get(&kind).is_none_or(|(_, old)| priority > *old) {
                    owners.insert(kind, (position, priority));
                }
            }
        }
    }
    let mut result = PanelOrder::new();
    for position in POSITIONS {
        let mut items = im::Vector::new();
        if let Some(saved) = order.get(&position) {
            for kind in saved.iter().map(|kind| kind.canonical()) {
                if owners
                    .get(&kind)
                    .is_some_and(|(owner, _)| *owner == position)
                    && !items.contains(&kind)
                {
                    items.push_back(kind);
                }
            }
        }
        result.insert(position, items);
    }
    for kind in PanelKind::iter().filter(|kind| kind.canonical() == *kind) {
        if !owners.contains_key(&kind) {
            result
                .entry(kind.default_position())
                .or_default()
                .push_back(kind);
        }
    }
    for position in POSITIONS {
        let style = styles.entry(position).or_default();
        let previous = order
            .get(&position)
            .and_then(|items| items.get(style.active))
            .map(|kind| kind.canonical());
        let items = &result[&position];
        if let Some(index) =
            previous.and_then(|kind| items.iter().position(|item| *item == kind))
        {
            style.active = index;
        } else {
            style.active = style.active.min(items.len().saturating_sub(1));
            // If a duplicate lost ownership, don't open a different tool in its place.
            if previous.is_some() || items.is_empty() {
                style.shown = false;
                style.maximized = false;
            }
        }
    }
    (result, styles)
}

fn relocate_panel(
    order: PanelOrder,
    styles: im::HashMap<PanelPosition, PanelStyle>,
    kind: PanelKind,
    destination: PanelPosition,
) -> (PanelOrder, im::HashMap<PanelPosition, PanelStyle>) {
    let kind = kind.canonical();
    let (mut order, mut styles) = normalize_panel_layout(order, styles);
    if panel_position(&order, &kind)
        .is_some_and(|(_, position)| position == destination)
    {
        return (order, styles);
    }
    for position in POSITIONS {
        let items = order.entry(position).or_default();
        if let Some(index) = items.iter().position(|item| *item == kind) {
            let style = styles.entry(position).or_default();
            if style.active == index {
                style.shown = false;
                style.maximized = false;
            }
            items.remove(index);
            if style.active > index {
                style.active -= 1;
            }
            style.active = style.active.min(items.len().saturating_sub(1));
            if items.is_empty() {
                style.shown = false;
                style.maximized = false;
            }
        }
    }
    let items = order.entry(destination).or_default();
    items.push_back(kind);
    let style = styles.entry(destination).or_default();
    style.active = items.len() - 1;
    style.shown = true;
    (order, styles)
}

#[cfg(test)]
mod layout_tests {
    use super::*;
    use std::collections::HashSet;
    fn assert_unique(order: &PanelOrder) {
        let kinds: Vec<_> = order
            .values()
            .flat_map(|items| items.iter().copied())
            .collect();
        assert_eq!(kinds.len(), kinds.iter().collect::<HashSet<_>>().len());
        assert!(!kinds.contains(&PanelKind::DocumentSymbol));
    }
    #[test]
    fn defaults_have_one_home_per_tool() {
        assert_unique(&default_panel_order());
    }
    #[test]
    fn migration_keeps_visible_duplicate_and_active_identity() {
        let mut order = default_panel_order();
        order.insert(
            PanelPosition::RightBottom,
            im::vector![PanelKind::DeviceManager],
        );
        order
            .get_mut(&PanelPosition::LeftTop)
            .unwrap()
            .push_front(PanelKind::DocumentSymbol);
        let mut styles = im::HashMap::new();
        styles.insert(
            PanelPosition::RightBottom,
            PanelStyle {
                shown: true,
                ..Default::default()
            },
        );
        styles.insert(
            PanelPosition::LeftTop,
            PanelStyle {
                active: 1,
                shown: true,
                ..Default::default()
            },
        );
        let (order, styles) = normalize_panel_layout(order, styles);
        assert_unique(&order);
        assert_eq!(
            panel_position(&order, &PanelKind::DeviceManager),
            Some((0, PanelPosition::RightBottom))
        );
        assert_eq!(
            order[&PanelPosition::LeftTop][styles[&PanelPosition::LeftTop].active],
            PanelKind::FileExplorer
        );
        assert!(styles[&PanelPosition::RightBottom].shown);
        let (again, _) = normalize_panel_layout(order.clone(), styles);
        assert_eq!(order, again);
    }
    #[test]
    fn moving_repairs_duplicates_and_preserves_other_active_tool() {
        let mut order = default_panel_order();
        order.insert(
            PanelPosition::RightBottom,
            im::vector![PanelKind::DeviceManager],
        );
        let mut styles = im::HashMap::new();
        styles.insert(
            PanelPosition::RightTop,
            PanelStyle {
                active: 3,
                shown: true,
                ..Default::default()
            },
        );
        let (order, styles) = relocate_panel(
            order,
            styles,
            PanelKind::DeviceManager,
            PanelPosition::LeftTop,
        );
        assert_unique(&order);
        assert_eq!(
            order[&PanelPosition::RightTop][styles[&PanelPosition::RightTop].active],
            PanelKind::FlutterPerformance
        );
        assert!(!styles[&PanelPosition::RightBottom].shown);
        let destination = &styles[&PanelPosition::LeftTop];
        assert!(destination.shown);
        assert_eq!(
            order[&PanelPosition::LeftTop][destination.active],
            PanelKind::DeviceManager
        );
    }
    #[test]
    fn moving_active_tool_closes_source_without_opening_another() {
        let order = default_panel_order();
        let index = panel_position(&order, &PanelKind::DeviceManager).unwrap().0;
        let mut styles = im::HashMap::new();
        styles.insert(
            PanelPosition::RightTop,
            PanelStyle {
                active: index,
                shown: true,
                ..Default::default()
            },
        );
        let (order, styles) = relocate_panel(
            order,
            styles,
            PanelKind::DeviceManager,
            PanelPosition::BottomLeft,
        );
        assert_unique(&order);
        assert!(!styles[&PanelPosition::RightTop].shown);
        assert!(styles[&PanelPosition::BottomLeft].shown);
    }
    #[test]
    fn moving_to_same_dock_keeps_order_and_selection() {
        let order = default_panel_order();
        let (again, _) = relocate_panel(
            order.clone(),
            im::HashMap::new(),
            PanelKind::DeviceManager,
            PanelPosition::RightTop,
        );
        for position in POSITIONS {
            assert_eq!(
                order.get(&position).cloned().unwrap_or_default(),
                again[&position]
            );
        }
    }
    #[test]
    fn old_outline_alias_has_one_home() {
        let (order, _) = relocate_panel(
            default_panel_order(),
            im::HashMap::new(),
            PanelKind::DocumentSymbol,
            PanelPosition::LeftTop,
        );
        assert_unique(&order);
        assert_eq!(
            panel_position(&order, &PanelKind::DocumentSymbol),
            panel_position(&order, &PanelKind::DartOutline)
        );
    }
}

fn toggle_tool_windows(
    mut current: im::HashMap<PanelPosition, PanelStyle>,
    saved: Option<im::HashMap<PanelPosition, PanelStyle>>,
) -> (
    im::HashMap<PanelPosition, PanelStyle>,
    Option<im::HashMap<PanelPosition, PanelStyle>>,
) {
    if current.values().any(|style| style.shown) {
        let saved = current.clone();
        for (_, style) in current.iter_mut() {
            style.shown = false;
        }
        (current, Some(saved))
    } else {
        (saved.unwrap_or(current), None)
    }
}
#[cfg(test)]
mod tool_window_tests {
    use super::*;
    #[test]
    fn hiding_and_restoring_keeps_active_tabs_and_maximized_state() {
        let layout = im::hashmap! {
            PanelPosition::LeftTop => PanelStyle { active:2,shown:true,maximized:false },
            PanelPosition::BottomLeft => PanelStyle { active:3,shown:true,maximized:true },
            PanelPosition::RightTop => PanelStyle { active:0,shown:false,maximized:false },
        };
        let (hidden, saved) = toggle_tool_windows(layout.clone(), None);
        assert!(hidden.values().all(|style| !style.shown));
        let (restored, _) = toggle_tool_windows(hidden, saved);
        for (position, expected) in layout.iter() {
            let actual = &restored[position];
            assert_eq!(
                (actual.active, actual.shown, actual.maximized),
                (expected.active, expected.shown, expected.maximized)
            );
        }
    }
}
