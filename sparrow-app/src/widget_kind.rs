//! Semantic classification of Flutter widgets.
//!
//! [`WidgetKind`] groups Flutter's ~300 built-in widgets (plus common user
//! widget naming patterns) into a manageable set of categories for icon
//! selection, tree-view grouping, and outline filtering.

use serde::{Deserialize, Serialize};
use strum_macros::{
    AsRefStr, Display, EnumIter, EnumMessage, EnumString, IntoStaticStr,
};

use crate::config::icon::SparrowIcons;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default,
    Serialize, Deserialize,
    Display, AsRefStr, IntoStaticStr, EnumString, EnumIter, EnumMessage,
)]
#[serde(rename_all = "kebab-case")]
#[strum(ascii_case_insensitive)]
pub enum WidgetKind {
    // ── Widget lifecycle ────────────────────────────────────────────
    #[strum(message = "Stateless Widget")]
    Stateless,
    #[strum(message = "Stateful Widget")]
    Stateful,
    #[strum(message = "Inherited Widget")]
    Inherited,
    #[strum(message = "Builder")]
    Builder,

    // ── Layout / composition ────────────────────────────────────────
    #[strum(message = "Layout")]
    Layout,
    #[strum(message = "Padding")]
    Padding,
    #[strum(message = "Expanded")]
    Expanded,
    #[strum(message = "Scrollable")]
    Scrollable,
    #[strum(message = "List")]
    List,
    #[strum(message = "Grid")]
    Grid,
    #[strum(message = "Sliver")]
    Sliver,
    #[strum(message = "Viewport")]
    Viewport,

    // ── Content ─────────────────────────────────────────────────────
    #[strum(message = "Text")]
    Text,
    #[strum(message = "Icon / Image")]
    Icon,
    #[strum(message = "Decoration")]
    Decoration,

    // ── Input / form controls ───────────────────────────────────────
    #[strum(message = "Input")]
    Input,
    #[strum(message = "Button")]
    Button,
    #[strum(message = "Toggle")]
    Toggle,
    #[strum(message = "Slider")]
    Slider,

    // ── Navigation ──────────────────────────────────────────────────
    #[strum(message = "Navigation")]
    Navigation,
    #[strum(message = "App Bar")]
    AppBar,
    #[strum(message = "Scaffold")]
    Scaffold,
    #[strum(message = "App")]
    App,

    // ── Design systems ──────────────────────────────────────────────
    #[strum(message = "Material")]
    Material,
    #[strum(message = "Cupertino")]
    Cupertino,

    // ── Animation ───────────────────────────────────────────────────
    #[strum(message = "Animated")]
    Animated,

    // ── Interaction ─────────────────────────────────────────────────
    #[strum(message = "Gesture")]
    Gesture,

    // ── Painting / graphics ─────────────────────────────────────────
    #[strum(message = "Painting")]
    Painting,

    // ── Async / reactive ────────────────────────────────────────────
    #[strum(message = "Async")]
    Async,

    // ── State management ────────────────────────────────────────────
    #[strum(message = "Provider")]
    Provider,
    #[strum(message = "Bloc")]
    Bloc,
    #[strum(message = "Riverpod")]
    Riverpod,
    #[strum(message = "GetX")]
    GetX,

    // ── Fallback ────────────────────────────────────────────────────
    #[default]
    #[strum(message = "Widget")]
    Other,
}

/// Analyzer information about a single widget usage — enough to classify it
/// and to jump to its declaration.
#[derive(Debug, Clone, Default)]
pub struct FlutterWidgetInfo {
    /// The Dart class name, e.g. `Container`, `MyCustomButton`.
    pub class_name: String,
    /// The simple name of the superclass if known, e.g. `StatelessWidget`.
    pub superclass: Option<String>,
    /// Absolute byte offsets into the source file of the *class declaration*.
    pub source_range: Option<(usize, usize)>,
    /// Absolute byte offsets of the *instantiation site*.
    pub instantiation_range: Option<(usize, usize)>,
    /// The source snippet of the instantiation.
    pub snippet: Option<String>,
    /// Nesting depth inside the widget tree (0 = root).
    /// Computed by the analyzer from the parenthesised structure.
    pub depth: usize,
}

impl FlutterWidgetInfo {
    /// Classify this widget, preferring the superclass when known.
    pub fn kind(&self) -> WidgetKind {
        WidgetKind::from_ast_info(&self.class_name, self.superclass.as_deref())
    }
}

impl WidgetKind {
    // ════════════════════════════════════════════════════════════════
    //  Construction
    // ════════════════════════════════════════════════════════════════

    /// Resolve a widget kind from a Dart class name. Handles Flutter's
    /// built-ins and common user-widget naming conventions.
    pub fn from_class_name(name: &str) -> Self {
        let name = normalize_class_name(name);
        if name.is_empty() {
            return Self::Other;
        }

        // 1. Exact match against the built-in catalogue.
        if let Some(kind) = Self::exact_match(name) {
            return kind;
        }

        // 2. Prefix heuristics (must precede suffix checks).
        let lower = name.to_ascii_lowercase();
        if lower.starts_with("sliver") {
            return Self::Sliver;
        }
        if lower.starts_with("animated") {
            return Self::Animated;
        }
        if lower.starts_with("cupertino") {
            return Self::Cupertino;
        }
        if lower.starts_with("bloc") {
            return Self::Bloc;
        }
        if lower.starts_with("riverpod") || lower.starts_with("hookconsumer") {
            return Self::Riverpod;
        }

        // 3. Suffix heuristics (user code naming conventions).
        if lower.ends_with("builder") {
            return Self::Builder;
        }
        if lower.ends_with("provider") {
            return Self::Provider;
        }
        if lower.ends_with("button") {
            return Self::Button;
        }
        if lower.ends_with("bloc") || lower.ends_with("cubit") {
            return Self::Bloc;
        }
        if lower.ends_with("dialog") || lower.ends_with("sheet") {
            return Self::Material;
        }

        // 4. Fallback.
        Self::Other
    }

    /// Combine name-based classification with analyzer information when the
    /// user widget's superclass is known. Always prefers the AST answer.
    pub fn from_ast_info(class_name: &str, superclass: Option<&str>) -> Self {
        if let Some(super_name) = superclass {
            match super_name {
                "StatelessWidget" => return Self::Stateless,
                "StatefulWidget" => return Self::Stateful,
                "InheritedWidget" | "InheritedModel" | "InheritedNotifier"
                | "InheritedTheme" => return Self::Inherited,
                "ChangeNotifier" | "Notifier" => return Self::Provider,
                _ => {}
            }
        }
        Self::from_class_name(class_name)
    }

    // ════════════════════════════════════════════════════════════════
    //  Presentation helpers
    // ════════════════════════════════════════════════════════════════

    /// The [`SparrowIcons`] constant for this widget kind.
    pub fn icon(self) -> &'static str {
        match self {
            Self::Stateless => SparrowIcons::WIDGET_STATELESS,
            Self::Stateful => SparrowIcons::WIDGET_STATEFUL,
            Self::Inherited => SparrowIcons::WIDGET_INHERITED,
            Self::Builder => SparrowIcons::WIDGET_BUILDER,

            Self::Layout
            | Self::Padding
            | Self::Expanded
            | Self::Viewport
            | Self::AppBar
            | Self::Scaffold => SparrowIcons::WIDGET_LAYOUT,

            Self::Scrollable | Self::List | Self::Sliver => {
                SparrowIcons::WIDGET_LIST
            }
            Self::Grid => SparrowIcons::WIDGET_GRID,

            Self::Text
            | Self::Icon
            | Self::Decoration
            | Self::Input
            | Self::Button
            | Self::Toggle
            | Self::Slider
            | Self::Navigation
            | Self::Animated
            | Self::Gesture
            | Self::Painting => SparrowIcons::WIDGET,

            Self::App => SparrowIcons::FLUTTER,
            Self::Material => SparrowIcons::MATERIAL,
            Self::Cupertino => SparrowIcons::CUPERTINO,
            Self::Async => SparrowIcons::DART_FUTURE,

            Self::Provider | Self::Bloc | Self::Riverpod | Self::GetX => {
                SparrowIcons::WIDGET
            }

            Self::Other => SparrowIcons::WIDGET,
        }
    }

    /// Short human-readable label.
    pub fn label(self) -> &'static str {
        strum::EnumMessage::get_message(&self).unwrap_or("Widget")
    }

    /// Coarse grouping used by the widget-tree outline.
    pub fn category(self) -> &'static str {
        match self {
            Self::Stateless
            | Self::Stateful
            | Self::Inherited
            | Self::Builder => "lifecycle",
            Self::Layout
            | Self::Padding
            | Self::Expanded
            | Self::Scrollable
            | Self::List
            | Self::Grid
            | Self::Sliver
            | Self::Viewport => "layout",
            Self::Text | Self::Icon | Self::Decoration => "content",
            Self::Input | Self::Button | Self::Toggle | Self::Slider => "input",
            Self::Navigation | Self::AppBar | Self::Scaffold | Self::App => {
                "navigation"
            }
            Self::Material | Self::Cupertino => "design-system",
            Self::Animated => "animation",
            Self::Gesture => "interaction",
            Self::Painting => "painting",
            Self::Async => "async",
            Self::Provider | Self::Bloc | Self::Riverpod | Self::GetX => "state",
            Self::Other => "other",
        }
    }

    // ════════════════════════════════════════════════════════════════
    //  Built-in catalogue
    // ════════════════════════════════════════════════════════════════

    fn exact_match(name: &str) -> Option<Self> {
        use WidgetKind::*;
        Some(match name {
            // ── Lifecycle ──
            "StatelessWidget" => Stateless,
            "StatefulWidget" => Stateful,
            "InheritedWidget" | "InheritedModel" | "InheritedNotifier"
            | "InheritedTheme" => Inherited,

            // ── Layout ──
            "Row" | "Column" | "Stack" | "Wrap" | "Flex" | "Container"
            | "Center" | "Align" | "Baseline" | "IntrinsicHeight"
            | "IntrinsicWidth" | "AspectRatio" | "FractionallySizedBox"
            | "Offstage" | "Opacity" | "Transform" | "RotatedBox"
            | "ClipRect" | "ClipRRect" | "ClipOval" | "ClipPath"
            | "DecoratedBox" | "ColoredBox" | "SizedOverflowBox"
            | "ConstrainedBox" | "OverflowBox" | "UnconstrainedBox"
            | "IndexedStack" | "Positioned" => Layout,

            // ── Padding / sizing ──
            "Padding" | "SizedBox" | "Gap" | "LimitedBox" | "FittedBox"
            | "EdgeInsets" => Padding,

            // ── Flex children ──
            "Expanded" | "Flexible" | "Spacer" => Expanded,

            // ── Lists ──
            "ListView" | "ReorderableListView" | "ListBody" => List,

            // ── Grids ──
            "GridView" => Grid,

            // ── Scrollable ──
            "SingleChildScrollView" | "CustomScrollView" | "PageView"
            | "Scrollable" | "Scrollbar" | "NestedScrollView" | "TabBarView"
            | "ScrollView" => Scrollable,

            // ── Viewport / environment ──
            "SafeArea" | "MediaQuery" | "Directionality" | "Viewport"
            | "Overlay" | "FocusScope" | "DefaultFocusTraversal" => Viewport,

            // ── Content ──
            "Text" | "RichText" | "SelectableText" | "DefaultTextStyle"
            | "TextSpan" | "WidgetSpan" => Text,

            "Icon" | "Image" | "FlutterLogo" | "Placeholder" | "CircleAvatar"
            | "IconTheme" => Icon,

            "Decoration" | "BoxDecoration" | "ShapeDecoration"
            | "InputDecoration" | "BoxShadow" | "Border" => Decoration,

            // ── Input ──
            "TextField" | "TextFormField" | "EditableText" | "TextInput"
            | "CupertinoTextField" | "CupertinoTextFormFieldRow" => Input,

            "ElevatedButton" | "TextButton" | "OutlinedButton"
            | "FilledButton" | "IconButton" | "FloatingActionButton"
            | "CupertinoButton" | "SegmentedButton" | "DropdownButton"
            | "PopupMenuButton" | "BackButton" | "CloseButton"
            | "MenuAnchor" => Button,

            "Checkbox" | "Radio" | "Switch" | "SwitchListTile"
            | "CheckboxListTile" | "RadioListTile" | "ToggleButtons"
            | "CupertinoSwitch" | "CupertinoCheckbox" => Toggle,

            "Slider" | "RangeSlider" | "CupertinoSlider" | "SliderTheme" => {
                Slider
            }

            // ── Navigation ──
            "Navigator" | "MaterialPageRoute" | "CupertinoPageRoute"
            | "PageRouteBuilder" | "Router" | "RouterDelegate" | "Route"
            | "RouteInformationParser" | "HeroControllerScope" => Navigation,

            "AppBar" | "NavigationBar" | "NavigationRail"
            | "BottomNavigationBar" | "BottomAppBar" | "TabBar"
            | "TabPageSelector" | "CupertinoNavigationBar" => AppBar,

            "Scaffold" | "Drawer" | "EndDrawer" | "BottomSheet"
            | "CupertinoPageScaffold" | "CupertinoTabScaffold" => Scaffold,

            "MaterialApp" | "WidgetsApp" | "CupertinoApp" => App,

            // ── Material ──
            "Material" | "Card" | "ListTile" | "ListTileTheme" | "Divider"
            | "VerticalDivider" | "ExpansionTile" | "Ink" | "Stepper"
            | "SnackBar" | "SnackBarAction" | "Dialog" | "AlertDialog"
            | "SimpleDialog" | "DrawerHeader" | "UserAccountsDrawerHeader"
            | "Chip" | "InputChip" | "ActionChip" | "ChoiceChip"
            | "FilterChip" => Material,

            // ── Animation ──
            "Hero" | "HeroController" | "AnimationController" | "Tween"
            | "CurveTween" | "Curves" => Animated,

            // ── Gesture / interaction ──
            "GestureDetector" | "InkWell" | "InkResponse" | "Dismissible"
            | "Draggable" | "DragTarget" | "LongPressDraggable" | "Listener"
            | "RawGestureDetector" | "MouseRegion" | "AbsorbPointer"
            | "IgnorePointer" => Gesture,

            // ── Painting ──
            "CustomPaint" | "CustomPainter" | "CustomMultiChildLayout"
            | "RepaintBoundary" | "Flow" | "ShaderMask" | "BackdropFilter"
            | "ImageFiltered" | "ColorFiltered" | "PhysicalModel"
            | "PhysicalShape" => Painting,

            // ── Async ──
            "FutureBuilder" | "StreamBuilder" | "ValueListenableBuilder"
            | "StreamBuilderBase" => Async,

            // ── State management ──
            "ChangeNotifier" | "ChangeNotifierProvider" | "Provider"
            | "MultiProvider" | "ProxyProvider" | "Consumer" | "Selector" => {
                Provider
            }
            "BlocProvider" | "BlocBuilder" | "BlocListener" | "BlocConsumer"
            | "BlocSelector" | "RepositoryProvider" | "MultiBlocProvider" => {
                Bloc
            }
            "ProviderScope" | "ConsumerWidget" | "HookConsumerWidget" => {
                Riverpod
            }
            "GetMaterialApp" | "Obx" | "GetBuilder" => GetX,

            _ => return None,
        })
    }
}

/// Strip generic parameters (`Widget<T>` → `Widget`) and namespace prefixes
/// (`package:flutter/widgets.dart::Row` → `Row`, `foo.Bar` → `Bar`).
fn normalize_class_name(raw: &str) -> &str {
    let s = raw.split('<').next().unwrap_or(raw);
    let s = s.rsplit("::").next().unwrap_or(s);
    let s = s.rsplit('.').next().unwrap_or(s);
    s.trim()
}

// ════════════════════════════════════════════════════════════════════
//  Tests
// ════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::WidgetKind::*;
    use super::*;

    #[test]
    fn exact_matches_builtin_widgets() {
        assert_eq!(WidgetKind::from_class_name("Container"), Layout);
        assert_eq!(WidgetKind::from_class_name("Row"), Layout);
        assert_eq!(WidgetKind::from_class_name("Column"), Layout);
        assert_eq!(WidgetKind::from_class_name("Text"), Text);
        assert_eq!(WidgetKind::from_class_name("ElevatedButton"), Button);
        assert_eq!(WidgetKind::from_class_name("ListView"), List);
        assert_eq!(WidgetKind::from_class_name("GridView"), Grid);
        assert_eq!(WidgetKind::from_class_name("FutureBuilder"), Async);
        assert_eq!(WidgetKind::from_class_name("Scaffold"), Scaffold);
        assert_eq!(WidgetKind::from_class_name("MaterialApp"), App);
        assert_eq!(WidgetKind::from_class_name("GestureDetector"), Gesture);
    }

    #[test]
    fn prefix_heuristics() {
        // Animated* — none of these are in exact_match, so prefix wins.
        assert_eq!(WidgetKind::from_class_name("AnimatedContainer"), Animated);
        assert_eq!(WidgetKind::from_class_name("AnimatedOpacity"), Animated);
        assert_eq!(WidgetKind::from_class_name("AnimatedList"), Animated);

        // Sliver* — not catalogued individually, so prefix wins.
        assert_eq!(WidgetKind::from_class_name("SliverList"), Sliver);
        assert_eq!(WidgetKind::from_class_name("SliverAppBar"), Sliver);

        // Cupertino* — exact match wins for catalogued ones…
        assert_eq!(WidgetKind::from_class_name("CupertinoButton"), Button);
        assert_eq!(WidgetKind::from_class_name("CupertinoTextField"), Input);
        assert_eq!(WidgetKind::from_class_name("CupertinoSwitch"), Toggle);

        // …and the prefix catches ones we haven't catalogued yet.
        assert_eq!(WidgetKind::from_class_name("CupertinoAlertDialog"), Cupertino);
        assert_eq!(
            WidgetKind::from_class_name("CupertinoSegmentedControl"),
            Cupertino
        );
        assert_eq!(WidgetKind::from_class_name("CupertinoActivityIndicator"), Cupertino);

        // Bloc* — exact match wins for BlocBuilder, prefix is a safety net.
        assert_eq!(WidgetKind::from_class_name("BlocBuilder"), Bloc);
        assert_eq!(WidgetKind::from_class_name("BlocListenerTree"), Bloc);
    }

    #[test]
    fn suffix_heuristics_user_widgets() {
        assert_eq!(WidgetKind::from_class_name("MyCustomButton"), Button);
        assert_eq!(WidgetKind::from_class_name("FooBloc"), Bloc);
        assert_eq!(WidgetKind::from_class_name("CartCubit"), Bloc);
        assert_eq!(WidgetKind::from_class_name("UserProvider"), Provider);
        assert_eq!(WidgetKind::from_class_name("ConfirmDialog"), Material);
    }

    #[test]
    fn namespace_and_generics_are_stripped() {
        assert_eq!(
            WidgetKind::from_class_name("package:flutter/widgets.dart::Row"),
            Layout
        );
        assert_eq!(WidgetKind::from_class_name("widgets.Row"), Layout);
        assert_eq!(WidgetKind::from_class_name("StreamBuilder<int>"), Async);
    }

    #[test]
    fn ast_info_prefers_superclass() {
        assert_eq!(
            WidgetKind::from_ast_info("WeirdlyNamedButton", Some("StatefulWidget")),
            Stateful
        );
        assert_eq!(
            WidgetKind::from_ast_info("WeirdlyNamedButton", None),
            Button
        );
    }

    #[test]
    fn unknown_falls_back_to_other() {
        assert_eq!(WidgetKind::from_class_name("FooWidget"), Other);
        assert_eq!(WidgetKind::from_class_name(""), Other);
    }

    #[test]
    fn strum_round_trip() {
        let variants = [
            Stateless, Stateful, Layout, Button, Async, Material, Cupertino,
            Other, List, Grid,
        ];
        for kind in variants {
            let s: &'static str = kind.into();
            let parsed: WidgetKind = s.parse().unwrap();
            assert_eq!(kind, parsed, "round trip failed for {s}");
        }
    }

    #[test]
    fn labels_and_categories_are_populated() {
        let all = [
            Stateless, Stateful, Inherited, Builder,
            Layout, Padding, Expanded, Scrollable, List, Grid, Sliver, Viewport,
            Text, Icon, Decoration,
            Input, Button, Toggle, Slider,
            Navigation, AppBar, Scaffold, App,
            Material, Cupertino,
            Animated,
            Gesture,
            Painting,
            Async,
            Provider, Bloc, Riverpod, GetX,
            Other,
        ];
        for kind in all {
            assert!(!kind.label().is_empty(), "{kind:?} has empty label");
            assert!(!kind.category().is_empty(), "{kind:?} has empty category");
            assert!(!kind.icon().is_empty(), "{kind:?} has empty icon");
        }
    }
}