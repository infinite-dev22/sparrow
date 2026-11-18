use serde::{Deserialize, Serialize};
use structdesc::FieldNames;

#[derive(FieldNames, Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "kebab-case")]
pub struct CoreConfig {
    #[field_names(desc = "Enable modal editing (Vim like)")]
    pub modal: bool,
    #[serde(default)]
    #[field_names(
        desc = "Flutter SDK root directory. Leave empty for automatic detection. Reopen the project to restart analysis with the selected SDK."
    )]
    pub flutter_sdk_path: String,
    #[serde(default)]
    #[field_names(
        desc = "Standalone Dart SDK root directory. Flutter projects use Flutter's bundled Dart SDK. Leave empty for automatic detection. Reopen the project after changing this path."
    )]
    pub dart_sdk_path: String,
    #[field_names(desc = "Set the color theme of Sparrow")]
    pub color_theme: String,
    #[field_names(desc = "Set the icon theme of Sparrow")]
    pub icon_theme: String,
    #[field_names(
        desc = "Enable customised titlebar and disable OS native one (Linux, BSD, Windows)"
    )]
    pub custom_titlebar: bool,
    #[field_names(
        desc = "Only allow double-click to open files in the file explorer"
    )]
    pub file_explorer_double_click: bool,
}
