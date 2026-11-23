//! Navigation-first search across files, symbols, commands and text.

mod view;
pub(crate) use view::view;

use crate::{
    command::SparrowCommand,
    config::icon::SparrowIcons,
    project_search::{Hit, scan, walk},
};
use floem::reactive::{RwSignal, Scope, SignalUpdate, batch};
use nucleo::{
    Config, Matcher, Utf32Str,
    pattern::{CaseMatching, Normalization, Pattern},
};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    All,
    Classes,
    Files,
    Symbols,
    Actions,
    Text,
}

impl Tab {
    const ALL: [(Self, &'static str); 6] = [
        (Self::All, "All"),
        (Self::Classes, "Classes"),
        (Self::Files, "Files"),
        (Self::Symbols, "Symbols"),
        (Self::Actions, "Actions"),
        (Self::Text, "Text"),
    ];
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Places {
    Project,
    Open,
    All,
}

impl Places {
    fn label(self) -> &'static str {
        match self {
            Self::Project => "Project Files",
            Self::Open => "Open Files",
            Self::All => "All Places",
        }
    }
}

#[derive(Clone, Copy)]
pub struct State {
    pub(crate) active: RwSignal<bool>,
    tab: RwSignal<Tab>,
    query: RwSignal<String>,
    places: RwSignal<Places>,
    preview: RwSignal<bool>,
    case: RwSignal<bool>,
    words: RwSignal<bool>,
    regex: RwSignal<bool>,
    mask: RwSignal<String>,
}

impl State {
    pub(crate) fn new(cx: Scope) -> Self {
        Self {
            active: cx.create_rw_signal(false),
            tab: cx.create_rw_signal(Tab::All),
            query: cx.create_rw_signal(String::new()),
            places: cx.create_rw_signal(Places::Project),
            preview: cx.create_rw_signal(false),
            case: cx.create_rw_signal(false),
            words: cx.create_rw_signal(false),
            regex: cx.create_rw_signal(false),
            mask: cx.create_rw_signal(String::new()),
        }
    }

    pub(crate) fn open(&self) {
        batch(|| {
            self.tab.set(Tab::All);
            self.query.set(String::new());
            self.active.set(true);
        });
    }
}

#[derive(Clone, PartialEq)]
enum ResultTarget {
    Location(Hit),
    Action(SparrowCommand),
}
#[derive(Clone, PartialEq)]
struct Row {
    target: ResultTarget,
    title: String,
    hint: String,
    icon: &'static str,
}
impl Row {
    fn hit(&self) -> Option<&Hit> {
        match &self.target {
            ResultTarget::Location(hit) => Some(hit),
            ResultTarget::Action(_) => None,
        }
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

fn score(pattern: &Pattern, text: &str, matcher: &mut Matcher) -> Option<u32> {
    pattern.score(Utf32Str::new(text, &mut Vec::new()), matcher)
}

fn file_row(path: PathBuf, root: Option<&Path>) -> Row {
    let title = file_name(&path);
    let hint = path
        .parent()
        .map(|parent| {
            root.and_then(|root| parent.strip_prefix(root).ok())
                .unwrap_or(parent)
                .display()
                .to_string()
        })
        .unwrap_or_default();
    Row {
        target: ResultTarget::Location(Hit {
            path,
            offset: 0,
            position: None,
            line: 0,
            span: 0..0,
            text: title.clone().into(),
        }),
        title,
        hint,
        icon: SparrowIcons::FILE,
    }
}

struct Request {
    tab: Tab,
    query: String,
    places: Places,
    open: HashSet<PathBuf>,
    root: Option<PathBuf>,
    case: bool,
    words: bool,
    regex: bool,
    mask: String,
}

fn search(request: &Request) -> (Vec<Row>, String) {
    let root = request.root.as_deref();
    let query = request.query.trim();
    let actions_only = request.tab == Tab::All && query.starts_with('/');
    let query = if actions_only {
        query[1..].trim_start()
    } else {
        query
    };
    let mut result = Vec::new();
    let mut message = String::new();
    let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
    let mut matcher = Matcher::new(Config::DEFAULT.match_paths());
    if matches!(request.tab, Tab::All | Tab::Files) && !actions_only {
        let mut files = Vec::new();
        if request.places != Places::Open
            && let Some(root) = root
            && let Err(error) = walk(root, &mut files)
        {
            message = error;
        }
        if request.places != Places::Project || query.is_empty() {
            files.extend(
                request
                    .open
                    .iter()
                    .filter(|path| {
                        request.places != Places::Project
                            || root.is_some_and(|root| path.starts_with(root))
                    })
                    .cloned(),
            );
        }
        files.sort();
        files.dedup();
        let mut ranked = files
            .into_iter()
            .filter_map(|path| {
                let relative = root
                    .and_then(|root| path.strip_prefix(root).ok())
                    .unwrap_or(&path);
                let rank = if query.is_empty() {
                    u32::from(request.open.contains(&path)) * 100
                } else {
                    let full =
                        score(&pattern, &relative.to_string_lossy(), &mut matcher)?;
                    full + score(&pattern, &file_name(&path), &mut matcher)
                        .unwrap_or(0)
                };
                Some((rank, path))
            })
            .collect::<Vec<_>>();
        ranked
            .sort_by(|(a, left), (b, right)| b.cmp(a).then_with(|| left.cmp(right)));
        let limit = if request.tab == Tab::All && !query.is_empty() {
            35
        } else {
            160
        };
        result.extend(
            ranked
                .into_iter()
                .take(limit)
                .map(|(_, path)| file_row(path, root)),
        );
    }
    if (matches!(request.tab, Tab::All | Tab::Actions) && !query.is_empty())
        || actions_only
    {
        let mut ranked = crate::command::sparrow_internal_commands()
            .into_values()
            .filter_map(|command| {
                let title = command.kind.desc()?.to_owned();
                let rank = if query.is_empty() {
                    0
                } else {
                    score(&pattern, &title, &mut matcher)?
                };
                Some((
                    rank,
                    Row {
                        target: ResultTarget::Action(command),
                        title,
                        hint: "Action".into(),
                        icon: SparrowIcons::SEARCH,
                    },
                ))
            })
            .collect::<Vec<_>>();
        ranked
            .sort_by(|(a, l), (b, r)| b.cmp(a).then_with(|| l.title.cmp(&r.title)));
        result.extend(
            ranked
                .into_iter()
                .take(if request.tab == Tab::All { 35 } else { 160 })
                .map(|(_, row)| row),
        );
    }
    if matches!(request.tab, Tab::All | Tab::Text)
        && !actions_only
        && !query.is_empty()
        && let Some(root) = root
    {
        let text = scan(
            root,
            "",
            &request.mask,
            query,
            (request.places == Places::Open).then_some(&request.open),
            request.case,
            request.words,
            request.regex,
        );
        if !text.error.is_empty() {
            message = text.error;
        }
        result.extend(
            text.hits
                .into_iter()
                .take(if request.tab == Tab::All { 35 } else { 160 })
                .map(|hit| Row {
                    title: hit.snippet().0,
                    hint: format!("{}:{}", file_name(&hit.path), hit.line),
                    target: ResultTarget::Location(hit),
                    icon: SparrowIcons::SEARCH,
                }),
        );
    }
    (result, message)
}

fn symbol_rows(
    response: &serde_json::Value,
    tab: Tab,
    places: Places,
    root: Option<&Path>,
    open: &HashSet<PathBuf>,
) -> Vec<Row> {
    response["result"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let kind = item["kind"].as_u64()?;
            let class = matches!(kind, 5 | 10 | 11 | 23);
            if tab == Tab::Classes && !class {
                return None;
            }
            let path = lsp_types::Url::parse(item["location"]["uri"].as_str()?)
                .ok()?
                .to_file_path()
                .ok()?;
            let internal = root.is_some_and(|root| path.starts_with(root));
            if (places == Places::Project && !internal)
                || (places == Places::Open && !open.contains(&path))
            {
                return None;
            }
            let position = serde_json::from_value::<lsp_types::Position>(
                item["location"]["range"]["start"].clone(),
            )
            .ok();
            let title = item["name"].as_str()?.to_owned();
            let hint = item["containerName"]
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| file_name(&path));
            Some(Row {
                title: title.clone(),
                hint,
                icon: if class {
                    SparrowIcons::SYMBOL_KIND_CLASS
                } else {
                    SparrowIcons::SYMBOL_KIND_METHOD
                },
                target: ResultTarget::Location(Hit {
                    path,
                    position,
                    offset: 0,
                    span: 0..0,
                    line: position.map_or(0, |p| p.line as usize + 1),
                    text: title.into(),
                }),
            })
        })
        .take(120)
        .collect()
}
