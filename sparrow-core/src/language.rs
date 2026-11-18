use std::{
    collections::{HashMap, HashSet, hash_map::Entry},
    fmt::Write,
    path::Path,
    str::FromStr,
};

use once_cell::sync::Lazy;
use regex::Regex;
use sparrow_rpc::style::{LineStyle, Style};
use strum_macros::{AsRefStr, Display, EnumMessage, EnumString, IntoStaticStr};
use tracing::{Level, event};
use tree_sitter::{Point, TreeCursor};

use crate::{
    directory::Directory,
    syntax::highlight::{HighlightConfiguration, HighlightIssue},
};

#[remain::sorted]
pub enum Indent {
    Space(u8),
    Tab,
}

impl Indent {
    const fn tab() -> &'static str {
        Indent::Tab.as_str()
    }

    const fn space(count: u8) -> &'static str {
        Indent::Space(count).as_str()
    }

    const fn as_str(&self) -> &'static str {
        match self {
            Indent::Tab => "\u{0009}",
            #[allow(clippy::wildcard_in_or_patterns)]
            Indent::Space(v) => match v {
                2 => "\u{0020}\u{0020}",
                4 => "\u{0020}\u{0020}\u{0020}\u{0020}",
                8 | _ => {
                    "\u{0020}\u{0020}\u{0020}\u{0020}\u{0020}\u{0020}\u{0020}\u{0020}"
                }
            },
        }
    }
}

const DEFAULT_CODE_GLANCE_LIST: &[&str] = &["source_file"];
const DEFAULT_CODE_GLANCE_IGNORE_LIST: &[&str] = &["source_file"];

#[macro_export]
macro_rules! comment_properties {
    () => {
        CommentProperties {
            single_line_start: None,
            single_line_end: None,

            multi_line_start: None,
            multi_line_end: None,
            multi_line_prefix: None,
        }
    };
    ($s:expr) => {
        CommentProperties {
            single_line_start: Some($s),
            single_line_end: None,

            multi_line_start: None,
            multi_line_end: None,
            multi_line_prefix: None,
        }
    };
    ($s:expr, $e:expr) => {
        CommentProperties {
            single_line_start: Some($s),
            single_line_end: Some($e),

            multi_line_start: None,
            multi_line_end: None,
            multi_line_prefix: None,
        }
    };
    ($sl_s:expr, $sl_e:expr, $ml_s:expr, $ml_e:expr) => {
        CommentProperties {
            single_line_start: Some($sl_s),
            single_line_end: Some($sl_e),

            multi_line_start: Some($sl_s),
            multi_line_end: None,
            multi_line_prefix: Some($sl_e),
        }
    };
}

#[derive(Eq, PartialEq, Hash, Clone, Copy, Debug, PartialOrd, Ord, Default)]
pub struct SyntaxProperties {
    /// An extra check to make sure that the array elements are in the correct order.  
    /// If this id does not match the enum value, a panic will happen with a debug assertion message.
    id: SparrowLanguage,

    /// All tokens that can be used for comments in language
    comment: CommentProperties,
    /// The indent unit.  
    /// Two spaces for Dart and YAML project files.
    indent: &'static str,
    /// Filenames that belong to this language  
    /// `["Dockerfile"]` for Dockerfile, `[".editorconfig"]` for EditorConfig
    files: &'static [&'static str],
    /// File name extensions to determine the language.  
    /// For example, `["dart"]` for Dart sources.
    extensions: &'static [&'static str],
    /// Tree-sitter properties
    tree_sitter: TreeSitterProperties,
}

#[derive(Eq, PartialEq, Hash, Clone, Copy, Debug, PartialOrd, Ord, Default)]
struct TreeSitterProperties {
    /// the grammar name that's in the grammars folder
    grammar: Option<&'static str>,
    /// the grammar fn name
    grammar_fn: Option<&'static str>,
    /// the query folder name
    query: Option<&'static str>,
    /// Preface: Originally this feature was called "Code Lens", which is not
    /// an LSP "Code Lens". It is renamed to "Code Glance", below doc text is
    /// left unchanged.  
    ///
    /// Lists of tree-sitter node types that control how code lenses are built.
    /// The first is a list of nodes that should be traversed and included in
    /// the lens, along with their children. The second is a list of nodes that
    /// should be excluded from the lens, though they will still be traversed.
    /// See `walk_tree` for more details.
    ///
    /// The tree-sitter playground may be useful when creating these lists:
    /// https://tree-sitter.github.io/tree-sitter/playground
    ///
    /// If unsure, use `DEFAULT_CODE_GLANCE_LIST` and
    /// `DEFAULT_CODE_GLANCE_IGNORE_LIST`.
    code_glance: (&'static [&'static str], &'static [&'static str]),
    /// the tree-sitter tag names that can be put in sticky headers
    sticky_headers: &'static [&'static str],
}

impl TreeSitterProperties {
    const DEFAULT: Self = Self {
        grammar: None,
        grammar_fn: None,
        query: None,
        code_glance: (DEFAULT_CODE_GLANCE_LIST, DEFAULT_CODE_GLANCE_IGNORE_LIST),
        sticky_headers: &[],
    };
}

#[derive(Eq, PartialEq, Hash, Clone, Copy, Debug, PartialOrd, Ord, Default)]
struct CommentProperties {
    /// Single line comment token used when commenting out one line.
    /// For example, "//" for Dart or "#" for YAML.
    single_line_start: Option<&'static str>,
    single_line_end: Option<&'static str>,

    /// Multi line comment token used when commenting a selection of lines.
    /// For example, "//" for Dart or "#" for YAML.
    multi_line_start: Option<&'static str>,
    multi_line_end: Option<&'static str>,
    multi_line_prefix: Option<&'static str>,
}

/// NOTE: Keep the enum variants "fieldless" so they can cast to usize as array
/// indices into the LANGUAGES array.  See method `SparrowLanguage::properties`.
///
/// Do not assign values to the variants because the number of variants and
/// number of elements in the LANGUAGES array change as different features
/// selected by the cargo build command.
#[derive(
    Eq,
    PartialEq,
    Ord,
    PartialOrd,
    Hash,
    Clone,
    Copy,
    Debug,
    Display,
    AsRefStr,
    IntoStaticStr,
    EnumString,
    EnumMessage,
    Default,
)]
#[strum(ascii_case_insensitive)]
#[remain::sorted]
pub enum SparrowLanguage {
    // Do not move
    #[remain::unsorted]
    #[default]
    #[strum(message = "Plain Text")]
    PlainText,
    #[strum(message = "Bash")]
    Bash,
    #[strum(message = "C")]
    C,
    #[strum(message = "CMake")]
    Cmake,
    #[strum(message = "C++")]
    Cpp,
    #[strum(message = "CSS")]
    Css,
    #[strum(message = "Dart")]
    Dart,
    #[strum(message = "Diff")]
    Diff,
    #[strum(message = "Gitattributes")]
    Gitattributes,
    #[strum(message = "Git (commit)")]
    GitCommit,
    #[strum(message = "Git (config)")]
    GitConfig,
    #[strum(message = "Git (rebase)")]
    GitRebase,
    #[strum(message = "GLSL")]
    Glsl,
    #[strum(message = "Groovy")]
    Groovy,
    #[strum(message = "HTML")]
    Html,
    #[strum(message = "INI")]
    Ini,
    #[strum(message = "Java")]
    Java,
    #[strum(message = "JavaScript")]
    JavaScript,
    #[strum(message = "JSON")]
    Json,
    #[strum(message = "JSON5")]
    Json5,
    #[strum(message = "Kotlin")]
    Kotlin,
    #[strum(message = "Kotlin Build Script")]
    KotlinBuildScript,
    #[strum(message = "Log")]
    Log,
    #[strum(message = "Makefile")]
    Make,
    #[strum(message = "Markdown")]
    Markdown,
    #[strum(serialize = "markdown.inline")]
    MarkdownInline,
    #[strum(message = "Objective-C")]
    ObjectiveC,
    #[strum(message = "Shell Script (POSIX)")]
    ShellScript,
    #[strum(message = "Swift")]
    Swift,
    #[strum(message = "TOML")]
    Toml,
    #[strum(message = "TypeScript")]
    TypeScript,
    #[strum(message = "XML")]
    Xml,
    #[strum(message = "YAML")]
    Yaml,
}

/// NOTE: Elements in the array must be in the same order as the enum variants of
/// `SparrowLanguage` as they will be accessed using the enum variants as indices.
const LANGUAGES: &[SyntaxProperties] = &[
    SyntaxProperties {
        id: SparrowLanguage::PlainText,
        indent: Indent::tab(),
        files: &[],
        extensions: &["txt"],
        comment: comment_properties!(),
        tree_sitter: TreeSitterProperties::DEFAULT,
    },
    SyntaxProperties {
        id: SparrowLanguage::Bash,
        indent: Indent::space(2),
        files: &[],
        extensions: &["bash", "sh"],
        comment: comment_properties!("#"),
        tree_sitter: TreeSitterProperties::DEFAULT,
    },
    SyntaxProperties {
        id: SparrowLanguage::C,
        indent: Indent::space(4),
        files: &[],
        extensions: &["c", "h"],
        comment: comment_properties!("//"),
        tree_sitter: TreeSitterProperties {
            grammar: None,
            grammar_fn: None,
            query: None,
            code_glance: (DEFAULT_CODE_GLANCE_LIST, DEFAULT_CODE_GLANCE_IGNORE_LIST),
            sticky_headers: &["function_definition", "struct_specifier"],
        },
    },
    SyntaxProperties {
        id: SparrowLanguage::Cmake,
        indent: Indent::space(2),
        files: &["CMakeLists.txt", "cmakelists"],
        extensions: &["cmake"],
        comment: comment_properties!("#"),
        tree_sitter: TreeSitterProperties {
            grammar: None,
            grammar_fn: None,
            query: None,
            code_glance: (DEFAULT_CODE_GLANCE_LIST, DEFAULT_CODE_GLANCE_IGNORE_LIST),
            sticky_headers: &["function_definition"],
        },
    },
    SyntaxProperties {
        id: SparrowLanguage::Cpp,
        indent: Indent::space(4),
        files: &[],
        extensions: &["cpp", "cxx", "cc", "c++", "hpp", "hxx", "hh", "h++"],
        comment: comment_properties!("//"),
        tree_sitter: TreeSitterProperties {
            grammar: None,
            grammar_fn: None,
            query: None,
            code_glance: (DEFAULT_CODE_GLANCE_LIST, DEFAULT_CODE_GLANCE_IGNORE_LIST),
            sticky_headers: &[
                "function_definition",
                "class_specifier",
                "struct_specifier",
            ],
        },
    },
    SyntaxProperties {
        id: SparrowLanguage::Css,
        indent: Indent::space(2),
        files: &[],
        extensions: &["css"],
        comment: comment_properties!("/*", "*/"),
        tree_sitter: TreeSitterProperties::DEFAULT,
    },
    SyntaxProperties {
        id: SparrowLanguage::Dart,
        indent: Indent::space(2),
        files: &[],
        extensions: &["dart"],
        comment: CommentProperties {
            single_line_start: Some("//"),
            single_line_end: None,

            multi_line_start: Some("/*"),
            multi_line_prefix: None,
            multi_line_end: Some("*/"),
        },
        tree_sitter: TreeSitterProperties {
            grammar: None,
            grammar_fn: None,
            query: None,
            code_glance: (
                &["program", "class_definition"],
                &[
                    "program",
                    "import_or_export",
                    "comment",
                    "documentation_comment",
                ],
            ),
            sticky_headers: &["class_definition"],
        },
    },
    SyntaxProperties {
        id: SparrowLanguage::Diff,
        indent: Indent::tab(),
        files: &[],
        extensions: &["diff", "patch"],
        comment: comment_properties!(),
        tree_sitter: TreeSitterProperties::DEFAULT,
    },
    SyntaxProperties {
        id: SparrowLanguage::Gitattributes,
        indent: Indent::tab(),
        files: &[".gitattributes", ".gitignore", ".dockerignore"],
        extensions: &[],
        comment: comment_properties!(),
        tree_sitter: TreeSitterProperties::DEFAULT,
    },
    SyntaxProperties {
        id: SparrowLanguage::GitCommit,
        indent: Indent::tab(),
        files: &[],
        extensions: &[],
        comment: comment_properties!(),
        tree_sitter: TreeSitterProperties::DEFAULT,
    },
    SyntaxProperties {
        id: SparrowLanguage::GitConfig,
        indent: Indent::tab(),
        files: &[".gitconfig", ".git/config"],
        extensions: &[],
        comment: comment_properties!(),
        tree_sitter: TreeSitterProperties::DEFAULT,
    },
    SyntaxProperties {
        id: SparrowLanguage::GitRebase,
        indent: Indent::tab(),
        files: &[],
        extensions: &[],
        comment: comment_properties!(),
        tree_sitter: TreeSitterProperties::DEFAULT,
    },
    SyntaxProperties {
        id: SparrowLanguage::Glsl,
        indent: Indent::space(2),
        files: &[],
        // spellchecker:off
        extensions: &[
            "glsl", "cs", "vs", "gs", "fs", "csh", "vsh", "gsh", "fsh", "cshader",
            "vshader", "gshader", "fshader", "comp", "vert", "geom", "frag", "tesc",
            "tese", "mesh", "task", "rgen", "rint", "rahit", "rchit", "rmiss",
            "rcall",
        ],
        // spellchecker:on
        comment: comment_properties!("//"),
        tree_sitter: TreeSitterProperties::DEFAULT,
    },
    SyntaxProperties {
        id: SparrowLanguage::Groovy,
        indent: Indent::tab(),
        files: &[],
        extensions: &["gradle"],
        comment: comment_properties!(),
        tree_sitter: TreeSitterProperties::DEFAULT,
    },
    SyntaxProperties {
        id: SparrowLanguage::Html,
        indent: Indent::space(4),
        files: &[],
        extensions: &["html", "htm"],
        comment: comment_properties!("<!--", "-->"),
        tree_sitter: TreeSitterProperties::DEFAULT,
    },
    SyntaxProperties {
        id: SparrowLanguage::Ini,
        indent: Indent::tab(),
        files: &[".env"],
        extensions: &["ini", "properties", "xcconfig", "desktop", "env"],
        comment: comment_properties!("#"),
        tree_sitter: TreeSitterProperties::DEFAULT,
    },
    SyntaxProperties {
        id: SparrowLanguage::Java,
        indent: Indent::space(4),
        files: &[],
        extensions: &["java"],
        comment: comment_properties!("//"),
        tree_sitter: TreeSitterProperties::DEFAULT,
    },
    SyntaxProperties {
        id: SparrowLanguage::JavaScript,
        indent: Indent::space(2),
        files: &[],
        extensions: &["js", "mjs", "cjs", "jsx"],
        comment: comment_properties!("//"),
        tree_sitter: TreeSitterProperties::DEFAULT,
    },
    SyntaxProperties {
        id: SparrowLanguage::Json,
        indent: Indent::space(4),
        files: &[],
        extensions: &["json", "arb", "har", "gltf"],
        comment: comment_properties!(),
        tree_sitter: TreeSitterProperties::DEFAULT,
    },
    SyntaxProperties {
        id: SparrowLanguage::Json5,
        indent: Indent::tab(),
        files: &[],
        extensions: &[],
        comment: comment_properties!(),
        tree_sitter: TreeSitterProperties::DEFAULT,
    },
    SyntaxProperties {
        id: SparrowLanguage::Kotlin,
        indent: Indent::space(2),
        files: &[],
        extensions: &["kt"],
        comment: CommentProperties {
            single_line_start: Some("//"),
            single_line_end: None,

            multi_line_start: Some("/*"),
            multi_line_prefix: None,
            multi_line_end: Some("*/"),
        },
        tree_sitter: TreeSitterProperties::DEFAULT,
    },
    SyntaxProperties {
        id: SparrowLanguage::KotlinBuildScript,
        indent: Indent::space(2),
        files: &[],
        extensions: &["kts"],
        comment: CommentProperties {
            single_line_start: Some("//"),
            single_line_end: None,

            multi_line_start: Some("/*"),
            multi_line_prefix: None,
            multi_line_end: Some("*/"),
        },
        tree_sitter: TreeSitterProperties::DEFAULT,
    },
    SyntaxProperties {
        id: SparrowLanguage::Log,
        indent: Indent::tab(),
        files: &["log.txt"],
        extensions: &["log"],
        comment: comment_properties!(),
        tree_sitter: TreeSitterProperties::DEFAULT,
    },
    SyntaxProperties {
        id: SparrowLanguage::Make,
        indent: Indent::tab(),
        files: &["Makefile", "makefile", "GNUmakefile"],
        extensions: &["mk", "mak"],
        comment: comment_properties!("#"),
        tree_sitter: TreeSitterProperties::DEFAULT,
    },
    SyntaxProperties {
        id: SparrowLanguage::Markdown,
        indent: Indent::space(4),
        files: &[],
        extensions: &["md", "markdown", "mdown"],
        comment: comment_properties!(),
        tree_sitter: TreeSitterProperties::DEFAULT,
    },
    SyntaxProperties {
        id: SparrowLanguage::MarkdownInline,
        indent: Indent::space(4),
        // markdown inline is only used as an injection by the Markdown language
        files: &[],
        extensions: &[],
        comment: comment_properties!(),
        tree_sitter: TreeSitterProperties {
            grammar: Some("markdown_inline"),
            grammar_fn: Some("markdown_inline"),
            query: Some("markdown.inline"),
            code_glance: (DEFAULT_CODE_GLANCE_LIST, DEFAULT_CODE_GLANCE_IGNORE_LIST),
            sticky_headers: &[],
        },
    },
    SyntaxProperties {
        id: SparrowLanguage::ObjectiveC,
        indent: Indent::space(2),
        files: &[],
        extensions: &["m", "mm"],
        comment: comment_properties!("//"),
        tree_sitter: TreeSitterProperties::DEFAULT,
    },
    SyntaxProperties {
        id: SparrowLanguage::ShellScript,
        indent: Indent::space(2),
        files: &[],
        extensions: &["sh"],
        comment: comment_properties!("#"),
        tree_sitter: TreeSitterProperties::DEFAULT,
    },
    SyntaxProperties {
        id: SparrowLanguage::Swift,
        indent: Indent::space(2),
        files: &[],
        extensions: &["swift"],
        comment: comment_properties!("//"),
        tree_sitter: TreeSitterProperties::DEFAULT,
    },
    SyntaxProperties {
        id: SparrowLanguage::Toml,
        indent: Indent::space(2),
        files: &["Cargo.lock"],
        extensions: &["toml"],
        comment: comment_properties!("#"),
        tree_sitter: TreeSitterProperties::DEFAULT,
    },
    SyntaxProperties {
        id: SparrowLanguage::TypeScript,
        indent: Indent::space(2),
        files: &[],
        extensions: &["ts", "tsx"],
        comment: comment_properties!("//"),
        tree_sitter: TreeSitterProperties::DEFAULT,
    },
    SyntaxProperties {
        id: SparrowLanguage::Xml,
        indent: Indent::space(4),
        files: &[],
        extensions: &["xml", "svg", "plist", "entitlements", "storyboard", "xib", "vcxproj", "iml"],
        comment: comment_properties!("<!--", "-->"),
        tree_sitter: TreeSitterProperties::DEFAULT,
    },
    SyntaxProperties {
        id: SparrowLanguage::Yaml,
        indent: Indent::space(2),
        files: &[".metadata", "pubspec.lock"],
        extensions: &["yml", "yaml"],
        comment: comment_properties!("#"),
        tree_sitter: TreeSitterProperties::DEFAULT,
    },
];

impl SparrowLanguage {
    const HIGHLIGHTS_INJECTIONS_FILE_NAME: &'static str = "injections.scm";
    const HIGHLIGHTS_QUERIES_FILE_NAME: &'static str = "highlights.scm";

    pub fn from_path(path: &Path) -> SparrowLanguage {
        Self::from_path_raw(path).unwrap_or(SparrowLanguage::PlainText)
    }

    pub fn from_path_raw(path: &Path) -> Option<SparrowLanguage> {
        let filename = path.file_name().and_then(|s| s.to_str());
        if filename.is_some_and(|name| name.starts_with(".env.")) { return Some(SparrowLanguage::Ini); }
        let extension = path
            .extension()
            .and_then(|s| s.to_str().map(|s| s.to_lowercase()));
        // NOTE: This is a linear search.  It is assumed that this function
        // isn't called in any tight loop.
        // Named configuration files (CMakeLists.txt, log.txt, dotfiles)
        // take priority over a generic extension such as txt.
        for properties in LANGUAGES {
            if properties.files.iter().any(|f| Some(*f) == filename) {
                return Some(properties.id);
            }
        }
        for properties in LANGUAGES {
            if properties
                .extensions
                .iter()
                .any(|e| Some(*e) == extension.as_deref())
            {
                return Some(properties.id);
            }
        }

        None
    }

    pub fn from_name(name: &str) -> Option<SparrowLanguage> {
        match SparrowLanguage::from_str(name.to_lowercase().as_str()) {
            Ok(v) => Some(v),
            Err(e) => {
                event!(Level::DEBUG, "failed parsing `{name}` SparrowLanguage: {e}");
                None
            }
        }
    }

    /// Names used by the upstream grammar/query archive for supported
    /// Dart and Flutter project-file syntax only.
    pub fn syntax_asset_names() -> HashSet<String> {
        LANGUAGES
            .iter()
            .flat_map(|p| [p.id.grammar_name(), p.id.query_name()])
            .collect()
    }

    pub fn languages() -> Vec<&'static str> {
        let mut langs = vec![];
        for l in LANGUAGES {
            // Get only languages with display name to hide inline grammars
            if let Some(lang) = strum::EnumMessage::get_message(&l.id) {
                langs.push(lang)
            }
        }
        langs
    }

    // NOTE: Instead of using `&LANGUAGES[*self as usize]` directly, the
    // `debug_assertion` gives better feedback should something has gone wrong
    // badly.
    fn properties(&self) -> &SyntaxProperties {
        let i = *self as usize;
        let l = &LANGUAGES[i];
        debug_assert!(
            l.id == *self,
            "LANGUAGES[{i}]: Setting::id mismatch: {:?} != {:?}",
            l.id,
            self
        );
        l
    }

    pub fn name(&self) -> &'static str {
        strum::EnumMessage::get_message(self).unwrap_or(self.into())
    }

    pub fn sticky_header_tags(&self) -> &[&'static str] {
        self.properties().tree_sitter.sticky_headers
    }

    pub fn comment_token(&self) -> &'static str {
        self.properties()
            .comment
            .single_line_start
            .unwrap_or_default()
    }

    pub fn indent_unit(&self) -> &str {
        self.properties().indent
    }

    fn get_grammar(&self) -> Option<tree_sitter::Language> {
        let grammar_name = self.grammar_name();
        let grammar_fn_name = self.grammar_fn_name();

        if let Some(grammars_dir) = Directory::grammars_directory() {
            match self::load_grammar(&grammar_name, &grammar_fn_name, &grammars_dir)
            {
                Ok(grammar) => {
                    return Some(grammar);
                }
                Err(err) => {
                    if self != &SparrowLanguage::PlainText {
                        tracing::debug!("Using bundled highlighting for {:?}: {:?}", self, err);
                    }
                }
            }
        };

        None
    }

    fn query_name(&self) -> String {
        self.properties()
            .tree_sitter
            .query
            .unwrap_or(self.properties().id.as_ref())
            .to_lowercase()
    }

    fn grammar_name(&self) -> String {
        self.properties()
            .tree_sitter
            .grammar
            .unwrap_or(self.properties().id.as_ref())
            .to_lowercase()
    }

    fn grammar_fn_name(&self) -> String {
        self.properties()
            .tree_sitter
            .grammar_fn
            .unwrap_or(self.properties().id.as_ref())
            .to_lowercase()
    }

    fn get_grammar_query(&self) -> (String, String) {
        let query_name = self.query_name();

        // Try reading highlights from user config dir
        if let Some(queries_dir) = Directory::queries_directory() {
            return (
                read_grammar_query(
                    &queries_dir,
                    &query_name,
                    Self::HIGHLIGHTS_QUERIES_FILE_NAME,
                ),
                read_grammar_query(
                    &queries_dir,
                    &query_name,
                    Self::HIGHLIGHTS_INJECTIONS_FILE_NAME,
                ),
            );
        }

        ("".to_string(), "".to_string())
    }

    pub(crate) fn new_highlight_config(
        &self,
    ) -> Result<HighlightConfiguration, HighlightIssue> {
        let grammar = self.get_grammar().ok_or(HighlightIssue::NotAvailable)?;
        let (query, injection) = self.get_grammar_query();

        match HighlightConfiguration::new(grammar, &query, &injection, "") {
            Ok(x) => Ok(x),
            Err(x) => {
                let str = format!(
                    "Encountered {x:?} while trying to construct HighlightConfiguration for {}",
                    self.name()
                );
                event!(Level::ERROR, "{str}");
                Err(HighlightIssue::Error(str))
            }
        }
    }

    pub(crate) fn walk_tree(
        &self,
        cursor: &mut TreeCursor,
        normal_lines: &mut HashSet<usize>,
    ) {
        let (list, ignore_list) = self.properties().tree_sitter.code_glance;
        walk_tree(cursor, normal_lines, list, ignore_list);
    }
}

fn load_grammar(
    grammar_name: &str,
    grammar_fn_name: &str,
    path: &Path,
) -> Result<tree_sitter::Language, HighlightIssue> {
    let mut library_path = path.join(format!("libtree-sitter-{grammar_name}"));
    library_path.set_extension(std::env::consts::DLL_EXTENSION);

    if !library_path.exists() {
        event!(Level::WARN, "Grammar not found at: {library_path:?}");

        // Load backward compat libraries
        library_path = path.join(format!("tree-sitter-{grammar_name}"));
        library_path.set_extension(std::env::consts::DLL_EXTENSION);

        if !library_path.exists() {
            event!(Level::WARN, "Grammar not found at: {library_path:?}");
            return Err(HighlightIssue::Error("grammar not found".to_string()));
        }
    }

    event!(Level::DEBUG, "Loading grammar from user grammar dir");
    let library = match unsafe { libloading::Library::new(&library_path) } {
        Ok(v) => v,
        Err(e) => {
            let err = format!("Failed to load '{}': '{e}'", library_path.display());
            event!(Level::ERROR, err);
            return Err(HighlightIssue::Error(err));
        }
    };

    let language_fn_name =
        format!("tree_sitter_{}", grammar_fn_name.replace('-', "_"));
    event!(
        Level::DEBUG,
        "Loading grammar with address: '{language_fn_name}'"
    );
    let language = unsafe {
        let language_fn: libloading::Symbol<
            unsafe extern "C" fn() -> tree_sitter::Language,
        > = match library.get(language_fn_name.as_bytes()) {
            Ok(v) => v,
            Err(e) => {
                let err = format!("Failed to load '{language_fn_name}': '{e}'");
                event!(Level::ERROR, err);
                if let Some(e) = library.close().err() {
                    event!(Level::ERROR, "Failed to drop loaded library: {e}");
                };
                return Err(HighlightIssue::Error(err));
            }
        };
        language_fn()
    };
    std::mem::forget(library);

    Ok(language)
}

/// Walk an AST and determine which lines to include in the code glance.
///
/// Node types listed in `list` will be walked, along with their children. All
/// nodes encountered will be included, unless they are listed in `ignore_list`.
fn walk_tree(
    cursor: &mut TreeCursor,
    normal_lines: &mut HashSet<usize>,
    list: &[&str],
    ignore_list: &[&str],
) {
    let node = cursor.node();
    let start_pos = node.start_position();
    let end_pos = node.end_position();
    let kind = node.kind().trim();
    if !ignore_list.contains(&kind) && !kind.is_empty() {
        normal_lines.insert(start_pos.row);
        normal_lines.insert(end_pos.row);
    }

    if list.contains(&kind) && cursor.goto_first_child() {
        loop {
            walk_tree(cursor, normal_lines, list, ignore_list);
            if !cursor.goto_next_sibling() {
                break;
            }
        }
        cursor.goto_parent();
    }
}

fn add_bracket_pos(
    bracket_pos: &mut HashMap<usize, Vec<LineStyle>>,
    start_pos: Point,
    color: String,
) {
    let line_style = LineStyle {
        start: start_pos.column,
        end: start_pos.column + 1,
        style: Style {
            fg_color: Some(color),
        },
    };
    match bracket_pos.entry(start_pos.row) {
        Entry::Vacant(v) => _ = v.insert(vec![line_style]),
        Entry::Occupied(mut o) => o.get_mut().push(line_style),
    }
}

pub(crate) fn walk_tree_bracket_ast(
    cursor: &mut TreeCursor,
    level: &mut usize,
    counter: &mut usize,
    bracket_pos: &mut HashMap<usize, Vec<LineStyle>>,
    palette: &Vec<String>,
) {
    if cursor.node().kind().ends_with('(')
        || cursor.node().kind().ends_with('{')
        || cursor.node().kind().ends_with('[')
    {
        let row = cursor.node().end_position().row;
        let col = cursor.node().end_position().column - 1;
        let start_pos = Point::new(row, col);
        add_bracket_pos(
            bracket_pos,
            start_pos,
            palette.get(*level % palette.len()).unwrap().clone(),
        );
        *level += 1;
    } else if cursor.node().kind().ends_with(')')
        || cursor.node().kind().ends_with('}')
        || cursor.node().kind().ends_with(']')
    {
        let (new_level, overflow) = (*level).overflowing_sub(1);
        let row = cursor.node().end_position().row;
        let col = cursor.node().end_position().column - 1;
        let start_pos = Point::new(row, col);
        if overflow {
            add_bracket_pos(bracket_pos, start_pos, "bracket.unpaired".to_string());
        } else {
            *level = new_level;
            add_bracket_pos(
                bracket_pos,
                start_pos,
                palette.get(*level % palette.len()).unwrap().clone(),
            );
        }
    }
    *counter += 1;
    if cursor.goto_first_child() {
        loop {
            walk_tree_bracket_ast(cursor, level, counter, bracket_pos, palette);
            if !cursor.goto_next_sibling() {
                break;
            }
        }
        cursor.goto_parent();
    }
}

fn read_grammar_query(queries_dir: &Path, name: &str, kind: &str) -> String {
    static INHERITS_REGEX: Lazy<Regex> =
        Lazy::new(|| Regex::new(r";+\s*inherits\s*:?\s*([a-z_,()-]+)\s*").unwrap());

    let file = queries_dir.join(name).join(kind);
    let query = std::fs::read_to_string(&file).unwrap_or_else(|err| {
        tracing::event!(
            tracing::Level::WARN,
            "Failed to read queries at: {file:?}, {err}"
        );
        String::new()
    });

    INHERITS_REGEX
        .replace_all(&query, |captures: &regex::Captures| {
            captures[1]
                .split(',')
                .fold(String::new(), |mut output, name| {
                    write!(
                        output,
                        "\n{}\n",
                        read_grammar_query(queries_dir, name, kind)
                    )
                    .unwrap();
                    output
                })
        })
        .to_string()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::SparrowLanguage;

    #[test]
    fn catalog_is_ordered_and_excludes_unrelated_languages() {
        for (i, p) in super::LANGUAGES.iter().enumerate() {
            assert_eq!(p.id as usize, i);
            let _ = p.id.properties();
        }
        for file in ["main.rs", "app.py", "main.go"] {
            assert_eq!(
                SparrowLanguage::from_path(std::path::Path::new(file)),
                SparrowLanguage::PlainText
            );
        }
        assert_eq!(
            SparrowLanguage::from_path(std::path::Path::new("pubspec.yaml")),
            SparrowLanguage::Yaml
        );
        assert_eq!(
            SparrowLanguage::from_path(std::path::Path::new("app_en.arb")),
            SparrowLanguage::Json
        );
        assert!(SparrowLanguage::from_name("Python").is_none());
    }
    #[test]
    fn flutter_configuration_and_native_resource_syntax() {
        use std::path::Path;
        for (file, language) in [
            ("model.freezed.dart", SparrowLanguage::Dart),
            (".metadata", SparrowLanguage::Yaml),
            ("pubspec.lock", SparrowLanguage::Yaml),
            (".env", SparrowLanguage::Ini),
            ("local.properties", SparrowLanguage::Ini),
            ("CMakeLists.txt", SparrowLanguage::Cmake),
            ("build.gradle", SparrowLanguage::Groovy),
            ("build.gradle.kts", SparrowLanguage::KotlinBuildScript),
            ("Runner.entitlements", SparrowLanguage::Xml),
            ("flutter.iml", SparrowLanguage::Xml),
            ("model.gltf", SparrowLanguage::Json),
        ] { assert_eq!(SparrowLanguage::from_path(Path::new(file)), language, "{file}"); }
    }
    #[test]
    fn test_language_from_path() {
        let l = SparrowLanguage::from_path(&PathBuf::new().join("test.dart"));
        assert_eq!(l, SparrowLanguage::Dart);
    }
}
