//! Rendered, in-memory Dart overlays in a separate server. Project buffers are untouched.
pub(super) mod actions;
use super::{Draft, engine, language};
use crate::dart::lsp::{DartLspService, Notification};
use floem::reactive::{RwSignal, Scope, SignalGet, SignalUpdate};
use lsp_types::{
    CompletionItem, CompletionTextEdit, DiagnosticSeverity, Position, TextEdit,
};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    ops::Range,
    path::{Path, PathBuf},
    rc::Rc,
    str::FromStr,
};

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Choice {
    version: i64,
    source: PathBuf,
    output: PathBuf,
    offset: usize,
    expected: String,
    item: CompletionItem,
}
#[derive(Clone, Debug)]
struct Segment {
    raw: Range<usize>,
    generated: Range<usize>,
    variable: bool,
}
#[derive(Clone, Debug)]
pub(super) struct Projection {
    source: Option<PathBuf>,
    original: PathBuf,
    raw: String,
    generated: String,
    segments: Vec<Segment>,
}
impl Projection {
    fn new(
        source: Option<PathBuf>,
        raw: String,
        vars: &BTreeMap<&str, String>,
    ) -> Result<Self, String> {
        let mut generated = String::new();
        let mut segments = vec![];
        let mut last = 0;
        for (range, key) in language::placeholders(&raw) {
            let value = vars.get(key).ok_or("Unknown projection variable")?;
            let begin = generated.len();
            generated.push_str(&raw[last..range.start]);
            segments.push(Segment {
                raw: last..range.start,
                generated: begin..generated.len(),
                variable: false,
            });
            let begin = generated.len();
            generated.push_str(value);
            segments.push(Segment {
                raw: range.clone(),
                generated: begin..generated.len(),
                variable: true,
            });
            last = range.end;
        }
        let begin = generated.len();
        generated.push_str(&raw[last..]);
        segments.push(Segment {
            raw: last..raw.len(),
            generated: begin..generated.len(),
            variable: false,
        });
        Ok(Self {
            source,
            original: PathBuf::new(),
            raw,
            generated,
            segments,
        })
    }
    fn rewrite_uris(
        &mut self,
        package: &str,
        root: &Path,
        aliases: &BTreeMap<PathBuf, PathBuf>,
    ) -> Result<(), String> {
        // Read only URI literals in import/export/part directives, including conditional URIs.
        let directives = regex::Regex::new(
            r#"(?m)(?:^|[;\n])\s*(?:import|export|part(?:\s+of)?)\s+[^;]*;"#,
        )
        .unwrap();
        let strings = regex::Regex::new(r#"['"]([^'"]+)['"]"#).unwrap();
        let mut edits = vec![];
        let base = lsp_types::Url::from_file_path(&self.original)
            .map_err(|_| "Invalid analysis URI")?;
        for directive in directives.find_iter(&self.generated) {
            for captures in strings.captures_iter(directive.as_str()) {
                let uri = captures.get(1).unwrap();
                let target = if let Some(path) =
                    uri.as_str().strip_prefix(&format!("package:{package}/"))
                {
                    Some(root.join("lib").join(path))
                } else if uri.as_str().contains(':') {
                    lsp_types::Url::parse(uri.as_str())
                        .ok()
                        .and_then(|u| u.to_file_path().ok())
                } else {
                    base.join(uri.as_str())
                        .ok()
                        .and_then(|u| u.to_file_path().ok())
                };
                let Some(target) = target else {
                    continue;
                };
                // Canonicalize lexically, since generated files need not exist.
                let mut normalized = PathBuf::new();
                for part in target.components() {
                    match part {
                        std::path::Component::ParentDir => {
                            normalized.pop();
                        }
                        std::path::Component::CurDir => {}
                        _ => normalized.push(part.as_os_str()),
                    }
                }
                let new = aliases.get(&normalized).cloned().unwrap_or(normalized);
                let new_uri = lsp_types::Url::from_file_path(new)
                    .map_err(|_| "Invalid projected import URI")?
                    .to_string();
                let range =
                    directive.start() + uri.start()..directive.start() + uri.end();
                if self.generated[range.clone()] != new_uri {
                    edits.push((range, new_uri));
                }
            }
        }
        if edits.is_empty() {
            return Ok(());
        }
        let old = self.clone();
        self.generated.clear();
        self.segments.clear();
        let mut last = 0;
        for (range, text) in edits {
            self.append_original(&old, last..range.start);
            let start = self.generated.len();
            self.generated.push_str(&text);
            self.segments.push(Segment {
                raw: old.diagnostic_range(range.clone()),
                generated: start..self.generated.len(),
                variable: true,
            });
            last = range.end;
        }
        self.append_original(&old, last..old.generated.len());
        Ok(())
    }
    fn append_original(&mut self, old: &Projection, range: Range<usize>) {
        let start = self.generated.len();
        self.generated.push_str(&old.generated[range.clone()]);
        for segment in &old.segments {
            let a = range.start.max(segment.generated.start);
            let b = range.end.min(segment.generated.end);
            if a < b {
                let raw = if segment.variable {
                    segment.raw.clone()
                } else {
                    segment.raw.start + a - segment.generated.start
                        ..segment.raw.start + b - segment.generated.start
                };
                self.segments.push(Segment {
                    raw,
                    generated: start + a - range.start..start + b - range.start,
                    variable: segment.variable,
                });
            }
        }
        // Preserve literal insertion points at the beginning/end, including empty files.
        if self.segments.is_empty() && range.is_empty() {
            let raw = old.diagnostic_range(range);
            self.segments.push(Segment {
                raw,
                generated: start..start,
                variable: false,
            });
        }
    }
    fn to_generated(&self, offset: usize) -> Option<usize> {
        if offset > self.raw.len() || !self.raw.is_char_boundary(offset) {
            return None;
        }
        for s in &self.segments {
            if !s.variable && s.raw.contains(&offset) {
                return Some(s.generated.start + offset - s.raw.start);
            }
            if offset == s.raw.start {
                return Some(s.generated.start);
            }
            if offset == s.raw.end {
                return Some(s.generated.end);
            }
        }
        None
    }
    /// Edits must stay within literal text. Never replace a template expansion.
    fn edit_range(&self, range: Range<usize>) -> Option<Range<usize>> {
        if range.start > range.end || range.end > self.generated.len() {
            return None;
        }
        if !self.generated.is_char_boundary(range.start)
            || !self.generated.is_char_boundary(range.end)
        {
            return None;
        }
        if self.segments.iter().any(|s| {
            s.variable
                && range.start < s.generated.end
                && range.end > s.generated.start
        }) {
            return None;
        }
        let map = |offset| {
            self.segments.iter().find_map(|s| {
                if !s.variable
                    && offset >= s.generated.start
                    && offset <= s.generated.end
                {
                    Some(s.raw.start + offset - s.generated.start)
                } else {
                    None
                }
            })
        };
        Some(map(range.start)?..map(range.end)?)
    }
    fn diagnostic_range(&self, range: Range<usize>) -> Range<usize> {
        let map = |offset: usize, end: bool| {
            for s in &self.segments {
                if offset >= s.generated.start && offset <= s.generated.end {
                    return if s.variable {
                        if end { s.raw.end } else { s.raw.start }
                    } else {
                        s.raw.start + offset - s.generated.start
                    };
                }
            }
            self.raw.len()
        };
        let start = map(range.start, false);
        let end = map(range.end, true).max(start);
        start..end
    }
}
#[derive(Clone, Debug)]
pub(super) struct Snapshot {
    version: i64,
    sample: String,
    root: PathBuf,
    files: BTreeMap<PathBuf, Projection>,
    symbols: BTreeMap<String, String>,
}
impl Snapshot {
    pub fn new(draft: &Draft, sample: &str, version: i64) -> Result<Self, String> {
        let entries = draft
            .files
            .iter()
            .map(|(p, s)| (p.clone(), s.clone()))
            .collect::<Vec<_>>();
        let pack = super::super::pack_from_files(&entries)?;
        let rendered = engine::render_pack(&draft.ctx, &pack, sample)?;
        let vars = engine::template_variables(&draft.ctx, &pack, sample)?;
        let private_root = draft
            .ctx
            .root
            .join(format!("_sparrow_template_analysis_{}", std::process::id()))
            .join(format!("r{version}"));
        let aliases = rendered
            .iter()
            .filter(|(p, _)| p.extension().is_some_and(|e| e == "dart"))
            .map(|(p, _)| (draft.ctx.root.join(p), private_root.join(p)))
            .collect::<BTreeMap<_, _>>();
        let mut files = BTreeMap::new();
        let mut symbols = BTreeMap::new();
        for (file, (path, body)) in pack.files.iter().zip(rendered) {
            if path.extension().is_none_or(|e| e != "dart") {
                continue;
            }
            let mut projection = Projection::new(
                file.source.as_ref().map(PathBuf::from),
                file.content.clone(),
                &vars,
            )?;
            if projection.generated != body {
                return Err("Template projection differs from renderer".into());
            }
            // Record complete identifiers containing placeholders, for references inserted by completion.
            for (range, key) in language::placeholders(&projection.raw) {
                if ![
                    "name",
                    "variable",
                    "snake",
                    "suffixed_name",
                    "suffixed_snake",
                ]
                .contains(&key)
                {
                    continue;
                }
                let ident = |c: char| c.is_alphanumeric() || c == '_';
                let start = projection.raw[..range.start]
                    .char_indices()
                    .rev()
                    .take_while(|(_, c)| ident(*c))
                    .last()
                    .map(|(i, _)| i)
                    .unwrap_or(range.start);
                let end = range.end
                    + projection.raw[range.end..]
                        .chars()
                        .take_while(|c| ident(*c))
                        .map(char::len_utf8)
                        .sum::<usize>();
                let token = &projection.raw[start..end];
                let p = Projection::new(None, token.into(), &vars)?;
                symbols.insert(p.generated, token.into());
            }
            projection.original = draft.ctx.root.join(&path);
            projection.rewrite_uris(
                &draft.ctx.package,
                &draft.ctx.root,
                &aliases,
            )?;
            let virtual_path = aliases
                .get(&projection.original)
                .ok_or("Missing analysis URI")?
                .clone();
            let original_uri = if let Ok(relative) = path.strip_prefix("lib") {
                format!(
                    "package:{{{{package}}}}/{}",
                    relative.to_string_lossy().replace('\\', "/")
                )
            } else {
                lsp_types::Url::from_file_path(&projection.original)
                    .map_err(|_| "Invalid original analysis URI")?
                    .to_string()
            };
            symbols.insert(
                lsp_types::Url::from_file_path(&virtual_path)
                    .map_err(|_| "Invalid analysis URI")?
                    .to_string(),
                original_uri,
            );
            files.insert(virtual_path, projection);
        }
        // Current-package auto-imports should remain reusable in another package.
        symbols.insert(
            format!("package:{}/", draft.ctx.package),
            "package:{{package}}/".into(),
        );
        Ok(Self {
            version,
            sample: sample.to_string(),
            root: draft.ctx.root.clone(),
            files,
            symbols,
        })
    }
    fn file_for(&self, source: &Path) -> Option<(&PathBuf, &Projection)> {
        self.files
            .iter()
            .find(|(_, p)| p.source.as_deref() == Some(source))
    }
    fn restore_template_names(&self, text: &str) -> String {
        let mut normalized = text.to_string();
        let mut aliases = self
            .symbols
            .iter()
            .filter(|(key, _)| key.contains(':'))
            .collect::<Vec<_>>();
        aliases.sort_by_key(|(key, _)| std::cmp::Reverse(key.len()));
        for (literal, template) in aliases {
            normalized = normalized.replace(literal, template);
        }
        let text = normalized.as_str();
        let mut result = String::new();
        let mut offset = 0;
        for (start, word) in text
            .split_inclusive(|c: char| !c.is_alphanumeric() && c != '_')
            .scan(0, |at, chunk| {
                let old = *at;
                *at += chunk.len();
                Some((old, chunk))
            })
        {
            result.push_str(&text[offset..start]);
            let end = word
                .char_indices()
                .find(|(_, c)| !c.is_alphanumeric() && *c != '_')
                .map(|(i, _)| i)
                .unwrap_or(word.len());
            let identifier = &word[..end];
            result.push_str(
                self.symbols
                    .get(identifier)
                    .map(String::as_str)
                    .unwrap_or(identifier),
            );
            result.push_str(&word[end..]);
            offset = start + word.len();
        }
        result.push_str(&text[offset..]);
        for (literal, template) in &self.symbols {
            if literal.starts_with("package:") {
                result = result.replace(literal, template);
            }
        }
        result
    }
    fn completion(
        &self,
        path: &Path,
        offset: usize,
        item: CompletionItem,
    ) -> Option<language::Completion> {
        let (output, p) = self.file_for(path)?;
        let generated_offset = p.to_generated(offset)?;
        let edit = completion_edit(&p.generated, generated_offset, &item)?;
        let range = p.edit_range(position_range(&p.generated, edit.range)?)?;
        Some(language::Completion {
            label: format!(
                "Dart · {}{}",
                item.label,
                item.detail
                    .as_ref()
                    .map(|d| format!(" · {d}"))
                    .unwrap_or_default()
            ),
            insert: self
                .restore_template_names(&plain_insert(&edit.new_text, &item)?),
            range,
            analyzer: Some(Choice {
                version: self.version,
                source: path.to_path_buf(),
                output: output.clone(),
                offset,
                expected: p.raw.clone(),
                item,
            }),
        })
    }
    fn resolved_edits(
        &self,
        choice: &Choice,
        item: &CompletionItem,
    ) -> Result<(Vec<(Range<usize>, String)>, usize), String> {
        if choice.version != self.version {
            return Err("Dart completion is stale; request it again".into());
        }
        let p = self.files.get(&choice.output).ok_or("Projection closed")?;
        if p.raw != choice.expected {
            return Err("Template changed since Dart completion".into());
        }
        let offset = p
            .to_generated(choice.offset)
            .ok_or("Cannot complete inside a template variable")?;
        let primary = completion_edit(&p.generated, offset, item)
            .ok_or("Dart completion has no usable edit")?;
        let range = p
            .edit_range(
                position_range(&p.generated, primary.range)
                    .ok_or("Invalid Dart completion range")?,
            )
            .ok_or("Dart completion would replace a template variable")?;
        let text = self.restore_template_names(
            &plain_insert(&primary.new_text, item)
                .ok_or("Unsupported Dart snippet")?,
        );
        let mut cursor = range.start + text.len();
        let mut edits = vec![(range.clone(), text)];
        for edit in item.additional_text_edits.iter().flatten() {
            let r = p
                .edit_range(
                    position_range(&p.generated, edit.range)
                        .ok_or("Invalid Dart import edit")?,
                )
                .ok_or("Dart import edit would replace a template variable")?;
            let text = self.restore_template_names(&edit.new_text);
            if r.end <= range.start {
                cursor = cursor
                    .checked_add(text.len())
                    .and_then(|c| c.checked_sub(r.len()))
                    .ok_or("Invalid completion cursor")?;
            }
            edits.push((r, text));
        }
        edits.sort_by_key(|(r, _)| (r.start, r.end));
        for pair in edits.windows(2) {
            if pair[0].0.end > pair[1].0.start || pair[0].0.start == pair[1].0.start
            {
                return Err("Dart completion edits overlap".into());
            }
        }
        Ok((edits, cursor))
    }
}
pub(super) fn position(text: &str, offset: usize) -> Option<Position> {
    if offset > text.len() || !text.is_char_boundary(offset) {
        return None;
    }
    let prefix = &text[..offset];
    let start = prefix.rfind('\n').map(|n| n + 1).unwrap_or(0);
    Some(Position::new(
        prefix
            .bytes()
            .filter(|c| *c == b'\n')
            .count()
            .try_into()
            .ok()?,
        text[start..offset].encode_utf16().count().try_into().ok()?,
    ))
}
fn offset(text: &str, position: Position) -> Option<usize> {
    let mut start = 0;
    for _ in 0..position.line {
        start += text[start..].find('\n')? + 1;
    }
    let mut units = 0;
    for (byte, c) in text[start..].char_indices() {
        if units == position.character {
            return Some(start + byte);
        }
        if c == '\n' || c == '\r' {
            return None;
        }
        units += c.len_utf16() as u32;
        if units > position.character {
            return None;
        }
    }
    (units == position.character).then_some(text.len())
}
fn position_range(text: &str, range: lsp_types::Range) -> Option<Range<usize>> {
    let start = offset(text, range.start)?;
    let end = offset(text, range.end)?;
    (start <= end).then_some(start..end)
}
fn completion_edit(
    text: &str,
    at: usize,
    item: &CompletionItem,
) -> Option<TextEdit> {
    match &item.text_edit {
        Some(CompletionTextEdit::Edit(e)) => Some(e.clone()),
        Some(CompletionTextEdit::InsertAndReplace(e)) => Some(TextEdit {
            range: e.replace,
            new_text: e.new_text.clone(),
        }),
        None => {
            let begin = text[..at]
                .char_indices()
                .rev()
                .find(|(_, c)| !c.is_alphanumeric() && *c != '_')
                .map(|(i, c)| i + c.len_utf8())
                .unwrap_or(0);
            Some(TextEdit {
                range: lsp_types::Range::new(
                    position(text, begin)?,
                    position(text, at)?,
                ),
                new_text: item
                    .insert_text
                    .clone()
                    .unwrap_or_else(|| item.label.clone()),
            })
        }
    }
}
fn plain_insert(text: &str, item: &CompletionItem) -> Option<String> {
    if item.insert_text_format == Some(lsp_types::InsertTextFormat::SNIPPET) {
        Some(crate::snippet::Snippet::from_str(text).ok()?.text())
    } else {
        Some(text.into())
    }
}

pub(super) struct Session {
    service: DartLspService,
    pub status: RwSignal<String>,
    pub ready: RwSignal<bool>,
    snapshot: RwSignal<Option<Snapshot>>,
    pub diagnostics: RwSignal<Vec<language::Diagnostic>>,
    published: RefCell<BTreeMap<PathBuf, Vec<language::Diagnostic>>>,
    raw_diagnostics: RefCell<BTreeMap<PathBuf, Vec<lsp_types::Diagnostic>>>,
    pub actions: actions::State,
    opened: RefCell<Vec<PathBuf>>,
    request: RwSignal<u64>,
    alive: std::cell::Cell<bool>,
    scope: Scope,
}
impl Session {
    pub fn new(root: PathBuf) -> Rc<Self> {
        let scope = Scope::current();
        let (service, rx) = DartLspService::spawn_for_templates(root);
        let available = service.is_available();
        let status = if available {
            "Starting Dart analyzer…".into()
        } else {
            service.unavailable_reason()
        };
        let session = Rc::new(Self {
            service,
            status: scope.create_rw_signal(status),
            ready: scope.create_rw_signal(false),
            snapshot: scope.create_rw_signal(None),
            diagnostics: scope.create_rw_signal(vec![]),
            published: RefCell::new(BTreeMap::new()),
            raw_diagnostics: RefCell::new(BTreeMap::new()),
            actions: actions::State::new(scope),
            opened: RefCell::new(vec![]),
            request: scope.create_rw_signal(0),
            alive: std::cell::Cell::new(true),
            scope,
        });
        let notification = floem::ext_event::create_signal_from_channel(rx);
        let target = Rc::downgrade(&session);
        floem::reactive::create_effect(move |_| {
            let Some(target) = target.upgrade() else {
                return;
            };
            let event = notification.get();
            if !target.alive.get() {
                return;
            }
            if let Some(event) = event {
                match event {
                    Notification::Ready => {
                        target.ready.set(true);
                        target.status.set(
                            "Dart analyzer · current project dependencies".into(),
                        );
                    }
                    Notification::Error(error) => {
                        target.ready.set(false);
                        target.status.set(format!("Dart analyzer: {error}"));
                        target.invalidate();
                    }
                    Notification::ApplyEdit { id, .. } => {
                        target.service.respond_to_edit(id, false)
                    }
                    Notification::Diagnostics {
                        path,
                        version,
                        diagnostics,
                    } => {
                        let Some(snapshot) = target.snapshot.get_untracked() else {
                            return;
                        };
                        // Each revision has immutable, private URIs. Dart omits
                        // versions, so URI membership also establishes freshness.
                        if version >= 0 && version != snapshot.version {
                            return;
                        }
                        let Some(p) = snapshot.files.get(&path) else {
                            return;
                        };
                        target.raw_diagnostics.borrow_mut().insert(path.clone(), diagnostics.clone());
                        let mapped = diagnostics
                            .into_iter()
                            .filter_map(|d| {
                                let range = position_range(&p.generated, d.range)?;
                                let message = format!(
                                    "Dart {}: {}",
                                    if d.severity
                                        == Some(DiagnosticSeverity::WARNING)
                                    {
                                        "warning"
                                    } else if d.severity
                                        == Some(DiagnosticSeverity::HINT)
                                        || d.severity
                                            == Some(DiagnosticSeverity::INFORMATION)
                                    {
                                        "hint"
                                    } else {
                                        "error"
                                    },
                                    d.message
                                );
                                Some(if let Some(source) = &p.source {
                                    language::Diagnostic {
                                        path: source.clone(),
                                        range: p.diagnostic_range(range),
                                        message,
                                    }
                                } else {
                                    language::Diagnostic {
                                        path: "pack.yaml".into(),
                                        range: 0..1,
                                        message: format!(
                                            "{}:{}:{} · {message}",
                                            path.strip_prefix(&snapshot.root)
                                                .unwrap_or(&path)
                                                .display(),
                                            d.range.start.line + 1,
                                            d.range.start.character + 1
                                        ),
                                    }
                                })
                            })
                            .collect::<Vec<_>>();
                        target.published.borrow_mut().insert(path, mapped);
                        let mut all = target
                            .published
                            .borrow()
                            .values()
                            .flatten()
                            .cloned()
                            .collect::<Vec<_>>();
                        all.sort_by(|a, b| {
                            (&a.path, a.range.start, &a.message).cmp(&(
                                &b.path,
                                b.range.start,
                                &b.message,
                            ))
                        });
                        all.dedup();
                        target.diagnostics.set(all);
                    }
                    _ => {}
                }
            }
        });
        session
    }
    pub fn invalidate(&self) {
        self.request.update(|n| *n += 1);
        self.snapshot.set(None);
        self.published.borrow_mut().clear();
        self.raw_diagnostics.borrow_mut().clear();
        self.actions.clear();
        self.diagnostics.set(vec![]);
    }
    pub fn update(&self, draft: &Draft, sample: &str, version: u64) {
        if !self.ready.get_untracked() {
            return;
        }
        let snapshot = match Snapshot::new(draft, sample, version as i64) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                self.status.set(format!("Dart analyzer paused: {error}"));
                return;
            }
        };
        for path in self.opened.borrow().iter() {
            if !snapshot.files.contains_key(path) {
                self.service.send_close(path.clone());
            }
        }
        for (path, p) in &snapshot.files {
            self.service.send_change(
                path.clone(),
                p.generated.clone(),
                snapshot.version,
            );
        }
        *self.opened.borrow_mut() = snapshot.files.keys().cloned().collect();
        self.snapshot.set(Some(snapshot));
        self.status
            .set("Dart analyzer · current project dependencies".into());
    }
    pub fn close(&self) {
        if !self.alive.replace(false) {
            return;
        }
        // View cleanup may already have disposed the editor's cursor signals.
        // Do not publish diagnostics or wake its effects during teardown.
        self.published.borrow_mut().clear();
        self.raw_diagnostics.borrow_mut().clear();
        for path in self.opened.borrow_mut().drain(..) {
            self.service.send_close(path);
        }
        self.service.shutdown_template_session();
    }
    pub fn request_completions(
        self: &Rc<Self>,
        path: &Path,
        ed: &floem::views::editor::Editor,
        selected: RwSignal<PathBuf>,
        suggestions: RwSignal<Vec<language::Completion>>,
        explicit: bool,
    ) {
        let _ = self.ready.get();
        let snapshot = self.snapshot.get();
        self.request.update(|n| *n += 1);
        let ticket = self.request.get_untracked();
        let Some(snapshot) = snapshot else {
            return;
        };
        let Some((output, p)) = snapshot.file_for(path) else {
            return;
        };
        let raw_offset = ed.cursor.get_untracked().offset();
        let Some(at) = p
            .to_generated(raw_offset)
            .and_then(|o| position(&p.generated, o))
        else {
            return;
        };
        if !explicit {
            let prefix = &p.raw[..raw_offset.min(p.raw.len())];
            let last = prefix.chars().last();
            if !last.is_some_and(|c| c == '.' || c.is_alphanumeric() || c == '_') {
                return;
            }
        }
        let path = path.to_path_buf();
        let output = output.clone();
        let target = self.clone();
        let ed = ed.clone();
        floem::action::exec_after(
            std::time::Duration::from_millis(if explicit { 0 } else { 180 }),
            move |_| {
                if !target.alive.get() || target.request.get_untracked() != ticket {
                    return;
                }
                let callback_target = target.clone();
                let cb = floem::ext_event::create_ext_action(
                    target.scope,
                    move |items: Option<Vec<CompletionItem>>| {
                        if !callback_target.alive.get()
                            || callback_target.request.get_untracked() != ticket
                            || selected.get_untracked() != path
                            || ed.cursor.get_untracked().offset() != raw_offset
                            || callback_target
                                .snapshot
                                .get_untracked()
                                .is_none_or(|s| s.version != snapshot.version)
                        {
                            return;
                        }
                        let Some((_, projection)) = snapshot.file_for(&path) else {
                            return;
                        };
                        if ed.doc().text().to_string() != projection.raw {
                            return;
                        }
                        let prefix = projection.raw[..raw_offset]
                            .chars()
                            .rev()
                            .take_while(|c| c.is_alphanumeric() || *c == '_')
                            .collect::<String>()
                            .chars()
                            .rev()
                            .collect::<String>()
                            .to_lowercase();
                        let mut items = items.unwrap_or_default();
                        items.sort_by(|a, b| {
                            a.sort_text
                                .as_ref()
                                .unwrap_or(&a.label)
                                .cmp(b.sort_text.as_ref().unwrap_or(&b.label))
                        });
                        let mut mapped = items
                            .into_iter()
                            .filter(|i| {
                                i.filter_text
                                    .as_ref()
                                    .unwrap_or(&i.label)
                                    .to_lowercase()
                                    .starts_with(&prefix)
                            })
                            .filter_map(|item| {
                                snapshot.completion(&path, raw_offset, item)
                            })
                            .take(60)
                            .collect::<Vec<_>>();
                        if !mapped.is_empty() {
                            mapped.extend(
                                suggestions
                                    .get_untracked()
                                    .into_iter()
                                    .filter(|c| c.analyzer.is_none()),
                            );
                            suggestions.set(mapped);
                        }
                    },
                );
                target.service.completion_with_trigger(
                    &output,
                    at.line,
                    at.character,
                    1,
                    None,
                    move |items, _| cb(items),
                );
            },
        );
    }
    pub fn accept(
        self: &Rc<Self>,
        ed: &floem::views::editor::Editor,
        choice: &language::Completion,
        draft: RwSignal<Option<Draft>>,
        selected: RwSignal<PathBuf>,
        plan: RwSignal<Option<engine::Plan>>,
        error: RwSignal<String>,
    ) {
        let Some(choice) = choice.analyzer.clone() else {
            super::accept_completion(ed, choice, draft, plan, error, selected);
            return;
        };
        let Some(snapshot) = self.snapshot.get_untracked() else {
            error.set("Dart completion is stale; request it again".into());
            return;
        };
        if snapshot.version != choice.version
            || selected.get_untracked() != choice.source
            || ed.doc().text().to_string() != choice.expected
        {
            error.set("Template changed; request Dart completion again".into());
            return;
        }
        let target = self.clone();
        let ed = ed.clone();
        let item = choice.item.clone();
        let cb = floem::ext_event::create_ext_action(
            self.scope,
            move |resolved: Option<CompletionItem>| {
                if !target.alive.get()
                    || target
                        .snapshot
                        .get_untracked()
                        .is_none_or(|s| s.version != choice.version)
                    || selected.get_untracked() != choice.source
                    || ed.doc().text().to_string() != choice.expected
                    || ed.cursor.get_untracked().offset() != choice.offset
                {
                    return;
                }
                let Some(resolved) = resolved else {
                    error.set("Dart completion could not be resolved".into());
                    return;
                };
                match snapshot.resolved_edits(&choice, &resolved) {
                    Err(e) => error.set(e),
                    Ok((edits, cursor)) => {
                        use floem::views::editor::core::{
                            editor::EditType, selection::Selection,
                        };
                        ed.doc().edit(
                            &mut edits.iter().map(|(r, s)| {
                                (Selection::region(r.start, r.end), s.as_str())
                            }),
                            EditType::Completion,
                        );
                        ed.cursor.update(|c| c.set_offset(cursor, false, false));
                        let body = ed.doc().text().to_string();
                        plan.set(None);
                        draft.update(|d| {
                            if let Some(d) = d {
                                d.files.insert(choice.source.clone(), body);
                            }
                        });
                        error.set(String::new());
                        if let Some(id) = ed.editor_view_id.get_untracked() {
                            id.request_focus();
                        }
                    }
                }
            },
        );
        self.service
            .resolve_completion(item, move |resolved| cb(resolved));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        time::{Duration, Instant},
    };
    pub(super) fn fixture(body: &str) -> (tempfile::TempDir, Draft) {
        let root = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join("pubspec.yaml"),
            "name: analyzer_fixture
environment:
  sdk: '>=3.9.0 <4.0.0'
",
        )
        .unwrap();
        let ctx = engine::context(root.path(), root.path()).unwrap();
        let manifest = "schema: 2
id: team.analyzer
version: 1.0.0
title: Analyzer
target: package
files:
  - path: lib/generated/{{snake}}.dart
    source: templates/main.dart.tmpl
";
        let files = BTreeMap::from([
            (PathBuf::from("pack.yaml"), manifest.into()),
            (PathBuf::from("templates/main.dart.tmpl"), body.into()),
        ]);
        let draft = Draft {
            files: files.clone(),
            original: files,
            directory: root.path().join(".sparrow/scaffolds/team.analyzer"),
            create_only: false,
            ctx,
            source: "Test".into(),
        };
        (root, draft)
    }
    fn vars() -> BTreeMap<&'static str, String> {
        BTreeMap::from([
            ("name", "Orders".into()),
            ("variable", "orders".into()),
            ("snake", "orders".into()),
        ])
    }
    #[test]
    fn source_map_tracks_whitespace_unicode_and_named_parameter_braces() {
        let raw = "// é🙂
class {{ name }} { void f({{{name}}? value}) {} }
";
        let p = Projection::new(Some("a.dart.tmpl".into()), raw.into(), &vars())
            .unwrap();
        assert_eq!(
            p.generated,
            "// é🙂
class Orders { void f({Orders? value}) {} }
"
        );
        let raw_offset = raw.find("value").unwrap();
        assert_eq!(
            p.to_generated(raw_offset),
            Some(p.generated.find("value").unwrap())
        );
        let range = p.generated.find("value").unwrap()
            ..p.generated.find("value").unwrap() + 5;
        assert_eq!(p.edit_range(range), Some(raw_offset..raw_offset + 5));
        let inside = raw.find("name").unwrap() + 1;
        assert!(p.to_generated(inside).is_none());
        let range = p.generated.find("Orders").unwrap()
            ..p.generated.find("Orders").unwrap() + 6;
        assert_eq!(&p.raw[p.diagnostic_range(range.clone())], "{{ name }}");
        assert!(p.edit_range(range).is_none());
        assert!(p.edit_range(0..0).is_some());
    }
    #[test]
    fn utf16_positions_handle_surrogates_crlf_and_invalid_offsets() {
        let text = "é🙂 first
next";
        let at = text.find(" first").unwrap();
        let pos = position(text, at).unwrap();
        assert_eq!(pos, Position::new(0, 3));
        assert_eq!(offset(text, pos), Some(at));
        assert_eq!(offset(text, Position::new(0, 2)), None);
        assert_eq!(offset(text, Position::new(1, 4)), Some(text.len()));
        assert_eq!(offset(text, Position::new(10, 0)), None);
        assert_eq!(position(text, 1), None);
    }
    #[test]
    fn projections_match_generator_and_preserve_generated_symbol_references() {
        let (_root, draft) = fixture(
            "class {{name}}Model {}
class InMemory{{name}}Repository {}
void main() {}
",
        );
        let s = Snapshot::new(&draft, "Orders", 7).unwrap();
        let (_, p) = s.file_for(Path::new("templates/main.dart.tmpl")).unwrap();
        assert!(p.generated.contains("class OrdersModel"));
        assert_eq!(
            s.restore_template_names(
                "OrdersModel value = OrdersModel(); InMemoryOrdersRepository();"
            ),
            "{{name}}Model value = {{name}}Model(); InMemory{{name}}Repository();"
        );
        assert_eq!(
            s.restore_template_names(
                "import 'package:analyzer_fixture/shared.dart';"
            ),
            "import 'package:{{package}}/shared.dart';"
        );
    }
    #[test]
    fn completion_and_auto_import_edits_map_to_one_safe_raw_document() {
        let (_root, draft) = fixture(
            "class {{name}}Model {}
void main() { final value = Ord; }
",
        );
        let s = Snapshot::new(&draft, "Orders", 8).unwrap();
        let (output, p) = s.file_for(Path::new("templates/main.dart.tmpl")).unwrap();
        let start = p.generated.find("Ord;").unwrap();
        let mut item = CompletionItem {
            label: "OrdersModel".into(),
            text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                range: lsp_types::Range::new(
                    position(&p.generated, start).unwrap(),
                    position(&p.generated, start + 3).unwrap(),
                ),
                new_text: "OrdersModel()".into(),
            })),
            ..Default::default()
        };
        let candidate = s
            .completion(
                Path::new("templates/main.dart.tmpl"),
                p.raw.find("Ord;").unwrap() + 3,
                item.clone(),
            )
            .unwrap();
        assert_eq!(candidate.insert, "{{name}}Model()");
        let choice = candidate.analyzer.unwrap();
        assert_eq!(choice.output, *output);
        item.additional_text_edits = Some(vec![TextEdit {
            range: lsp_types::Range::new(Position::new(0, 0), Position::new(0, 0)),
            new_text: "import 'package:analyzer_fixture/other.dart';
"
            .into(),
        }]);
        let (edits, cursor) = s.resolved_edits(&choice, &item).unwrap();
        let mut raw = p.raw.clone();
        for (range, text) in edits.iter().rev() {
            raw.replace_range(range.clone(), text);
        }
        assert!(raw.contains("final value = {{name}}Model();"));
        assert!(raw.starts_with("import 'package:{{package}}/other.dart';"));
        assert_eq!(&raw[cursor..cursor + 1], ";");
        let mut newer = s.clone();
        newer.version += 1;
        assert!(newer.resolved_edits(&choice, &item).is_err());
        item.additional_text_edits = Some(vec![TextEdit {
            range: match &item.text_edit {
                Some(CompletionTextEdit::Edit(e)) => e.range,
                _ => unreachable!(),
            },
            new_text: "overlap".into(),
        }]);
        assert!(s.resolved_edits(&choice, &item).is_err());
    }
    #[test]
    fn edits_inside_expansions_are_rejected_and_snippets_are_plain_text() {
        let (_root, draft) = fixture(
            "class {{name}} {}
",
        );
        let s = Snapshot::new(&draft, "Orders", 1).unwrap();
        let (_, p) = s.file_for(Path::new("templates/main.dart.tmpl")).unwrap();
        let start = p.generated.find("Orders").unwrap();
        let item = CompletionItem {
            label: "Unsafe".into(),
            text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                range: lsp_types::Range::new(
                    position(&p.generated, start).unwrap(),
                    position(&p.generated, start + 6).unwrap(),
                ),
                new_text: "Other".into(),
            })),
            ..Default::default()
        };
        assert!(
            s.completion(
                Path::new("templates/main.dart.tmpl"),
                p.raw.find("{{name}}").unwrap(),
                item
            )
            .is_none()
        );
        let item = CompletionItem {
            insert_text_format: Some(lsp_types::InsertTextFormat::SNIPPET),
            ..Default::default()
        };
        assert_eq!(
            plain_insert("add(${1:value})$0", &item),
            Some("add(value)".into())
        );
    }
    #[test]
    fn immutable_revisions_and_import_part_graphs_preserve_source_locations() {
        let (_root, mut draft) = fixture(
            "import 'package:analyzer_fixture/generated/orders_model.dart';\npart 'orders_part.dart';\nclass {{name}} { String value = 42; }\n",
        );
        let manifest = draft.files.get_mut(Path::new("pack.yaml")).unwrap();
        manifest.push_str("  - path: lib/generated/orders_model.dart\n    source: templates/model.dart.tmpl\n  - path: lib/generated/orders_part.dart\n    source: templates/part.dart.tmpl\n");
        draft.files.insert(
            "templates/model.dart.tmpl".into(),
            "class OrdersModel {}".into(),
        );
        draft.files.insert(
            "templates/part.dart.tmpl".into(),
            "part of 'orders.dart';".into(),
        );
        let a = Snapshot::new(&draft, "Orders", 10).unwrap();
        let b = Snapshot::new(&draft, "Orders", 11).unwrap();
        assert!(a.files.keys().all(|p| !b.files.contains_key(p)));
        let (_, main) = b.file_for(Path::new("templates/main.dart.tmpl")).unwrap();
        let (model_path, _) =
            b.file_for(Path::new("templates/model.dart.tmpl")).unwrap();
        let (part_path, part) =
            b.file_for(Path::new("templates/part.dart.tmpl")).unwrap();
        assert!(
            main.generated.contains(
                lsp_types::Url::from_file_path(model_path).unwrap().as_str()
            )
        );
        assert!(
            main.generated.contains(
                lsp_types::Url::from_file_path(part_path).unwrap().as_str()
            )
        );
        // The temporary file URI may also contain the digits 42.
        let start = main.generated.find("value = 42").unwrap() + "value = ".len();
        assert_eq!(&main.raw[main.diagnostic_range(start..start + 2)], "42");
        let (main_path, _) =
            b.file_for(Path::new("templates/main.dart.tmpl")).unwrap();
        assert!(
            part.generated.contains(
                lsp_types::Url::from_file_path(main_path).unwrap().as_str()
            )
        );
        let new_import = format!(
            "import '{}';",
            lsp_types::Url::from_file_path(model_path).unwrap()
        );
        assert_eq!(
            b.restore_template_names(&new_import),
            "import 'package:{{package}}/generated/orders_model.dart';"
        );
        assert!(b.files.keys().all(|p| !p.exists()));
    }
    #[test]
    fn invalid_drafts_never_become_analyzer_overlays() {
        let (_root, mut draft) = fixture("class {{unknown}} {}");
        assert!(Snapshot::new(&draft, "Example", 1).is_err());
        draft.files.insert(
            "templates/main.dart.tmpl".into(),
            "class {{name}} {}".into(),
        );
        let s = Snapshot::new(&draft, "Example", 2).unwrap();
        assert!(s.files.keys().all(|p| p.starts_with(&draft.ctx.root)
            && p.extension().is_some_and(|e| e == "dart")
            && !p.exists()));
    }
    #[test]
    fn read_only_server_without_sdk_does_not_bootstrap_package_resolution() {
        let root = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join("pubspec.yaml"),
            "name: unavailable
",
        )
        .unwrap();
        fs::create_dir(root.path().join(".sparrow")).unwrap();
        fs::write(
            root.path().join(".sparrow/settings.toml"),
            format!(
                "[core]
dart-sdk-path = '{}'
",
                root.path().join("missing-sdk").display()
            ),
        )
        .unwrap();
        let (service, _) =
            DartLspService::spawn_for_templates(root.path().to_path_buf());
        assert!(!service.is_available());
        assert!(!root.path().join(".dart_tool").exists());
        assert!(!root.path().join("pubspec.lock").exists());
    }
    #[test]
    #[ignore = "requires SPARROW_TEMPLATE_DART_SDK for an isolated real language-server check"]
    fn real_analyzer_reports_type_errors_and_type_aware_completions_without_project_writes()
     {
        let sdk = std::env::var("SPARROW_TEMPLATE_DART_SDK").unwrap();
        let (root, draft) = fixture(
            "class {{name}} {
  String bad = 42;
  void f() { final value = 123; value. }
}
",
        );
        fs::create_dir(root.path().join(".sparrow")).unwrap();
        fs::write(
            root.path().join(".sparrow/settings.toml"),
            format!(
                "[core]
dart-sdk-path = '{sdk}'
"
            ),
        )
        .unwrap();
        fs::create_dir(root.path().join(".dart_tool")).unwrap();
        fs::write(root.path().join(".dart_tool/package_config.json"),r#"{"configVersion":2,"packages":[{"name":"analyzer_fixture","rootUri":"../","packageUri":"lib/","languageVersion":"3.9"}]}"#).unwrap();
        let before = fs::read_to_string(root.path().join("pubspec.yaml")).unwrap();
        let snapshot = Snapshot::new(&draft, "Orders", 11).unwrap();
        let (path, p) = snapshot
            .file_for(Path::new("templates/main.dart.tmpl"))
            .unwrap();
        let (service, rx) =
            DartLspService::spawn_for_templates(root.path().to_path_buf());
        let deadline = Instant::now() + Duration::from_secs(35);
        loop {
            match rx
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .unwrap()
            {
                Notification::Ready => break,
                Notification::Error(e) => panic!("{e}"),
                _ => {}
            }
        }
        service.send_change(path.clone(), p.generated.clone(), snapshot.version);
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            match rx
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .unwrap()
            {
                Notification::Diagnostics {
                    path: got,
                    version,
                    diagnostics,
                } if got == *path => {
                    if let Some(d) = diagnostics.iter().find(|d| {
                        d.message.contains("String") && d.message.contains("int")
                    }) {
                        assert!(version < 0 || version == snapshot.version);
                        assert!(path.to_string_lossy().contains("/r11/"));
                        let mapped = p.diagnostic_range(
                            position_range(&p.generated, d.range).unwrap(),
                        );
                        assert!(p.raw[mapped].contains("42"));
                        break;
                    }
                }
                Notification::Error(e) => panic!("{e}"),
                _ => {}
            }
        }
        let at = p.generated.find("value.").unwrap() + 6;
        let pos = position(&p.generated, at).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        service.completion_with_trigger(
            path,
            pos.line,
            pos.character,
            1,
            None,
            move |items, _| {
                let _ = tx.send(items);
            },
        );
        let items = rx.recv_timeout(Duration::from_secs(25)).unwrap().unwrap();
        let item = items
            .into_iter()
            .find(|i| i.label.starts_with("toRadixString"))
            .expect("int-specific member completion");
        let raw_offset = p.raw.find("value.").unwrap() + 6;
        let choice = snapshot
            .completion(
                Path::new("templates/main.dart.tmpl"),
                raw_offset,
                item.clone(),
            )
            .unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        service.resolve_completion(item, move |item| {
            let _ = tx.send(item);
        });
        let item = rx.recv_timeout(Duration::from_secs(25)).unwrap().unwrap();
        let (edits, _) = snapshot
            .resolved_edits(choice.analyzer.as_ref().unwrap(), &item)
            .unwrap();
        assert!(edits.iter().any(|(_, s)| s.contains("toRadixString")));
        // Resolve an unimported SDK type and map its import edit as well.
        let mut draft2 = draft.clone();
        draft2.files.insert(
            "templates/main.dart.tmpl".into(),
            "class {{name}} { void f() { final random = Ran; } }".into(),
        );
        let second = Snapshot::new(&draft2, "Orders", 12).unwrap();
        let (second_path, second_projection) = second
            .file_for(Path::new("templates/main.dart.tmpl"))
            .unwrap();
        service.send_change(
            second_path.clone(),
            second_projection.generated.clone(),
            second.version,
        );
        let generated = second_projection.generated.find("Ran;").unwrap() + 3;
        let at = position(&second_projection.generated, generated).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        service.completion_with_trigger(
            second_path,
            at.line,
            at.character,
            1,
            None,
            move |items, _| {
                let _ = tx.send(items);
            },
        );
        let items = rx.recv_timeout(Duration::from_secs(25)).unwrap().unwrap();
        let item = items
            .into_iter()
            .find(|item| item.label.starts_with("Random"))
            .expect("unimported dart:math Random completion");
        let candidate = second
            .completion(
                Path::new("templates/main.dart.tmpl"),
                second_projection.raw.find("Ran;").unwrap() + 3,
                item.clone(),
            )
            .unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        service.resolve_completion(item, move |item| {
            let _ = tx.send(item);
        });
        let resolved = rx.recv_timeout(Duration::from_secs(25)).unwrap().unwrap();
        let (edits, _) = second
            .resolved_edits(candidate.analyzer.as_ref().unwrap(), &resolved)
            .unwrap();
        assert!(
            edits.iter().any(|(_, text)| text.contains("dart:math")),
            "auto-import edits: {edits:?}"
        );
        assert!(second.files.keys().all(|p| !p.exists()));
        service.send_close(second_path.clone());
        assert!(snapshot.files.keys().all(|p| !p.exists()));
        assert!(!root.path().join("lib").exists());
        assert!(!root.path().join("pubspec.lock").exists());
        assert_eq!(
            fs::read_to_string(root.path().join("pubspec.yaml")).unwrap(),
            before
        );
        service.send_close(path.clone());
        // Dialog cleanup must stop its private server even while a pending
        // completion callback retains another service handle.
        let retained = service.clone();
        service.shutdown_template_session();
        let deadline = Instant::now() + Duration::from_secs(5);
        while retained.is_available() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!retained.is_available(), "private server did not shut down");
    }
}
