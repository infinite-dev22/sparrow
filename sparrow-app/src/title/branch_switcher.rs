//! JetBrains-style branch switcher popup.
//!
//! Layout (top to bottom):
//!   * search field + "Fetch" and "Settings" icon buttons
//!   * Update / Commit / Push
//!   * New Branch / New Worktree / Checkout Tag or Revision
//!   * scrollable tree: Recent, Local, Remote, Tags
//!
//! Hovering a ref row opens a second-level action menu to the right of the
//! popup; hovering "Tracked Branch '…'" in that menu opens a third level.
//! Every git operation that needs user input (new branch name, rename, …) is
//! sent through `SparrowWorkbenchCommand::GitCommand`, which opens the
//! reviewable command dialog so the user can edit the arguments before running.

use std::{rc::Rc, sync::Arc};

use floem::{
    AnyView, IntoView, View,
    event::EventListener,
    ext_event::create_ext_action,
    keyboard::{Key, NamedKey},
    kurbo::Point,
    peniko::Color,
    reactive::{
        Memo, ReadSignal, RwSignal, Scope, SignalGet, SignalTrack, SignalUpdate,
        SignalWith, create_effect, create_memo,
    },
    style::{CursorStyle, Display},
    views::{Decorators, container, dyn_stack, empty, label, stack, svg},
};

use crate::{
    command::{CommandKind, SparrowCommand, SparrowWorkbenchCommand},
    config::{SparrowConfig, color::SparrowColor, icon::SparrowIcons},
    listener::Listener,
    widgets::{divider::Divider, sparrow_scroll, text_input::TextInputBuilder},
    window_tab::WindowTabData,
};

// ───────────────────────────── geometry ─────────────────────────────

const PANEL_W: f64 = 375.0;
const ROW_H: f32 = 22.0;
const SEP_H: f64 = 9.0; // 1px line + 4px margin above and below
const SEARCH_H: f64 = 46.0;
/// search block + 3 separators + 6 action rows
const HEADER_H: f64 = SEARCH_H + SEP_H * 3.0 + ROW_H as f64 * 6.0;
/// 1px panel border + header
const TREE_TOP: f64 = 1.0 + HEADER_H;
const TREE_PAD: f64 = 4.0;
const TREE_MAX_H: f32 = 360.0;
const MENU_PAD: f32 = 6.0;

// ───────────────────────────── icons ─────────────────────────────

mod ic {
    pub const STAR: &str = r#"<polygon fill="currentColor" points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2"/>"#;
    pub const TAG: &str = r#"<path d="M12.586 2.586A2 2 0 0 0 11.172 2H4a2 2 0 0 0-2 2v7.172a2 2 0 0 0 .586 1.414l8.704 8.704a2.426 2.426 0 0 0 3.42 0l6.58-6.58a2.426 2.426 0 0 0 0-3.42z"/><circle cx="7.5" cy="7.5" r=".5" fill="currentColor"/>"#;
    pub const COMMIT: &str = r#"<circle cx="12" cy="12" r="3"/><line x1="3" x2="9" y1="12" y2="12"/><line x1="15" x2="21" y1="12" y2="12"/>"#;
    pub const ARROW_UP_RIGHT: &str = r#"<path d="M7 7h10v10"/><path d="M7 17 17 7"/>"#;
    pub const ARROW_DOWN_LEFT: &str = r#"<path d="M17 7 7 17"/><path d="M17 17H7V7"/>"#;
    pub const FORK: &str = r#"<circle cx="12" cy="18" r="3"/><circle cx="6" cy="6" r="3"/><circle cx="18" cy="6" r="3"/><path d="M18 9v2c0 .6-.4 1-1 1H7c-.6 0-1-.4-1-1V9"/><path d="M12 12v3"/>"#;
    pub const CHECK: &str = r#"<path d="M20 6 9 17l-5-5"/>"#;
    pub const PLUS: &str = r#"<path d="M5 12h14"/><path d="M12 5v14"/>"#;
}

fn lucide(body: &str) -> String {
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">{body}</svg>"#
    )
}

fn raw_icon(
    body: &'static str,
    size: f32,
    color: impl Fn() -> Color + 'static,
) -> impl View {
    svg(move || lucide(body)).style(move |s| {
        s.size(size, size).min_width(size).flex_shrink(0.0_f32).color(color())
    })
}

fn theme_icon(
    config: ReadSignal<Arc<SparrowConfig>>,
    name: &'static str,
    size: f32,
    color: impl Fn(&SparrowConfig) -> Color + 'static,
) -> impl View {
    svg(move || config.get().ui_svg(name)).style(move |s| {
        s.size(size, size)
            .min_width(size)
            .flex_shrink(0.0_f32)
            .color(color(&config.get()))
    })
}

// ───────────────────────────── colours ─────────────────────────────

fn fg(c: &SparrowConfig) -> Color {
    c.color(SparrowColor::EDITOR_FOREGROUND)
}
fn dim(c: &SparrowConfig) -> Color {
    c.color(SparrowColor::EDITOR_DIM)
}
fn selected_bg(c: &SparrowConfig) -> Color {
    c.color(SparrowColor::SPARROW_BUTTON_PRIMARY_BACKGROUND)
        .multiply_alpha(0.55)
}
fn hover_bg(c: &SparrowConfig) -> Color {
    c.color(SparrowColor::PANEL_HOVERED_BACKGROUND)
}
fn green() -> Color {
    Color::from_rgb8(0x5F, 0xB8, 0x65)
}
fn blue() -> Color {
    Color::from_rgb8(0x6C, 0x9B, 0xE0)
}
fn amber() -> Color {
    Color::from_rgb8(0xE8, 0xB3, 0x4A)
}

// ───────────────────────────── git data ─────────────────────────────

#[derive(Clone, Debug, PartialEq)]
struct LocalInfo {
    name: String,
    upstream: Option<String>,
    ahead: u32,
    behind: u32,
}

#[derive(Clone, Debug, Default)]
struct Snapshot {
    locals: Vec<LocalInfo>,
    remotes: Vec<String>,
    /// Most recently checked-out branches, newest first.
    recent: Vec<String>,
}

// --- pure helpers (unit-tested) ---
fn parse_track(track: &str) -> (u32, u32) {
    let (mut ahead, mut behind) = (0, 0);
    for part in track.split(',') {
        let part = part.trim();
        if let Some(n) = part.strip_prefix("ahead ") {
            ahead = n.trim().parse().unwrap_or(0);
        } else if let Some(n) = part.strip_prefix("behind ") {
            behind = n.trim().parse().unwrap_or(0);
        }
    }
    (ahead, behind)
}

fn parse_ref_line(line: &str) -> Option<RefLine> {
    let mut parts = line.split('\0');
    let full = parts.next()?;
    let upstream = parts.next().unwrap_or("");
    let track = parts.next().unwrap_or("");
    if let Some(name) = full.strip_prefix("refs/heads/") {
        let (ahead, behind) = parse_track(track);
        Some(RefLine::Local(LocalInfo {
            name: name.to_string(),
            upstream: (!upstream.is_empty()).then(|| upstream.to_string()),
            ahead,
            behind,
        }))
    } else if let Some(name) = full.strip_prefix("refs/remotes/") {
        (!name.ends_with("/HEAD")).then(|| RefLine::Remote(name.to_string()))
    } else {
        None
    }
}

enum RefLine {
    Local(LocalInfo),
    Remote(String),
}

/// `git reflog --format=%gs` lines: "checkout: moving from A to B".
fn parse_recent(reflog: &str, existing: &[LocalInfo], limit: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in reflog.lines() {
        let Some(rest) = line.strip_prefix("checkout: moving from ") else {
            continue;
        };
        let Some((from, to)) = rest.split_once(" to ") else {
            continue;
        };
        for name in [to, from] {
            if existing.iter().any(|l| l.name == name)
                && !out.iter().any(|o| o == name)
            {
                out.push(name.to_string());
            }
        }
        if out.len() >= limit {
            break;
        }
    }
    out.truncate(limit);
    out
}

fn split_remote(upstream: &str) -> (String, String) {
    match upstream.split_once('/') {
        Some((remote, branch)) => (remote.to_string(), branch.to_string()),
        None => ("origin".to_string(), upstream.to_string()),
    }
}
// --- end pure helpers ---

fn load_snapshot(root: &std::path::Path) -> Snapshot {
    let run = |args: &[&str]| -> Option<String> {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(root)
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdin(std::process::Stdio::null())
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    };

    let mut snap = Snapshot::default();
    if let Some(text) = run(&[
        "for-each-ref",
        "--format=%(refname)%00%(upstream:short)%00%(upstream:track,nobracket)",
        "refs/heads",
        "refs/remotes",
    ]) {
        for line in text.lines() {
            match parse_ref_line(line) {
                Some(RefLine::Local(l)) => snap.locals.push(l),
                Some(RefLine::Remote(r)) => snap.remotes.push(r),
                None => {}
            }
        }
    }
    if let Some(reflog) = run(&["reflog", "-n", "300", "--format=%gs"]) {
        snap.recent = parse_recent(&reflog, &snap.locals, 5);
    }
    snap
}

// ───────────────────────────── view model ─────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RefKind {
    Local,
    Remote,
    Tag,
}

#[derive(Clone, Debug, PartialEq)]
struct Target {
    kind: RefKind,
    name: String,
    upstream: Option<String>,
    is_current: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Section {
    Recent,
    Local,
    Remote,
    Tags,
}

impl Section {
    fn title(self) -> &'static str {
        match self {
            Section::Recent => "Recent",
            Section::Local => "Local",
            Section::Remote => "Remote",
            Section::Tags => "Tags",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Row {
    Header {
        section: Section,
        idx: usize,
    },
    Ref {
        target: Target,
        ahead: u32,
        behind: u32,
        idx: usize,
    },
}

#[derive(Clone, Copy)]
struct Sections {
    recent: RwSignal<bool>,
    local: RwSignal<bool>,
    remote: RwSignal<bool>,
    tags: RwSignal<bool>,
}

impl Sections {
    fn signal(&self, s: Section) -> RwSignal<bool> {
        match s {
            Section::Recent => self.recent,
            Section::Local => self.local,
            Section::Remote => self.remote,
            Section::Tags => self.tags,
        }
    }
}

/// Toggles from the settings (gear) menu.
#[derive(Clone, Copy)]
struct Show {
    recent: RwSignal<bool>,
    remote: RwSignal<bool>,
    tags: RwSignal<bool>,
}

// ───────────────────────────── shared context ─────────────────────────────

#[derive(Clone, Copy)]
struct Ctx {
    config: ReadSignal<Arc<SparrowConfig>>,
    sparrow_command: Listener<SparrowCommand>,
    workbench_command: Listener<SparrowWorkbenchCommand>,
    visible: RwSignal<bool>,
    current_branch: ReadSignal<String>,
    search: Memo<String>,
    sections: Sections,
    /// Row index + target whose second-level menu is open.
    selected: RwSignal<Option<(usize, Target)>>,
    /// Top of the 2nd level menu, relative to the panel's top edge.
    l2_top: RwSignal<f64>,
    l3_open: RwSignal<bool>,
    l3_top: RwSignal<f64>,
    scroll_y: RwSignal<f64>,
    settings_open: RwSignal<bool>,
}

impl Ctx {
    fn dismiss_submenus(&self) {
        self.selected.set(None);
        self.l3_open.set(false);
    }

    fn close(&self) {
        self.dismiss_submenus();
        self.settings_open.set(false);
        self.visible.set(false);
    }

    fn git(&self, args: Vec<String>) {
        self.sparrow_command.send(SparrowCommand {
            kind: CommandKind::Workbench(SparrowWorkbenchCommand::GitCommand),
            data: Some(serde_json::json!(args)),
        });
        self.close();
    }

    fn checkout(&self, reference: &str) {
        self.sparrow_command.send(SparrowCommand {
            kind: CommandKind::Workbench(SparrowWorkbenchCommand::CheckoutReference),
            data: Some(serde_json::json!(reference)),
        });
        self.close();
    }

    fn workbench(&self, cmd: SparrowWorkbenchCommand) {
        self.workbench_command.send(cmd);
        self.close();
    }

    fn select(&self, idx: usize, target: Target) {
        let row_top =
            TREE_TOP + TREE_PAD + ROW_H as f64 * idx as f64 - self.scroll_y.get_untracked();
        // first menu item lines up with the hovered row
        self.l2_top.set(row_top - MENU_PAD as f64 - 1.0);
        self.l3_open.set(false);
        self.settings_open.set(false);
        self.selected.set(Some((idx, target)));
    }
}

macro_rules! sv {
    ($($e:expr),* $(,)?) => { vec![$(String::from($e)),*] };
}

// ───────────────────────────── menu entries ─────────────────────────────

#[derive(Clone, Copy)]
enum Icon {
    Theme(&'static str),
    Raw(&'static str),
}

#[derive(Clone, Copy, PartialEq)]
enum HoverMode {
    /// leave submenus alone
    None,
    /// hovering this entry closes the 3rd level
    CloseL3,
    /// hovering this entry closes every submenu
    Dismiss,
}

#[derive(Clone)]
struct Entry {
    key: String,
    label: String,
    shortcut: &'static str,
    separator: bool,
    enabled: bool,
    arrow: bool,
    checked: Option<bool>,
    icon: Option<Icon>,
    /// reserve the icon column even without an icon
    slot: bool,
    action: Option<Rc<dyn Fn()>>,
    on_hover: Option<Rc<dyn Fn()>>,
    active: Option<Rc<dyn Fn() -> bool>>,
}

impl Entry {
    fn base() -> Self {
        Entry {
            key: String::new(),
            label: String::new(),
            shortcut: "",
            separator: false,
            enabled: true,
            arrow: false,
            checked: None,
            icon: None,
            slot: false,
            action: None,
            on_hover: None,
            active: None,
        }
    }
    fn sep() -> Self {
        Entry {
            separator: true,
            ..Self::base()
        }
    }
    fn item(label: impl Into<String>, action: impl Fn() + 'static) -> Self {
        Entry {
            label: label.into(),
            action: Some(Rc::new(action)),
            ..Self::base()
        }
    }
    fn shortcut(mut self, s: &'static str) -> Self {
        self.shortcut = s;
        self
    }
    fn icon(mut self, i: Icon) -> Self {
        self.icon = Some(i);
        self
    }
    fn slot(mut self) -> Self {
        self.slot = true;
        self
    }
    fn enabled(mut self, e: bool) -> Self {
        self.enabled = e;
        self
    }
    fn arrow(mut self) -> Self {
        self.arrow = true;
        self
    }
    fn checked(mut self, c: bool) -> Self {
        self.checked = Some(c);
        self
    }
}

fn offset_of(entries: &[Entry], idx: usize) -> f64 {
    entries[..idx]
        .iter()
        .map(|e| if e.separator { SEP_H } else { ROW_H as f64 })
        .sum()
}

fn menu_width(entries: &[Entry]) -> f32 {
    let widest = entries
        .iter()
        .filter(|e| !e.separator)
        .map(|e| {
            let mut w = e.label.chars().count() as f32 * 7.2 + 50.0;
            if !e.shortcut.is_empty() {
                w += e.shortcut.chars().count() as f32 * 6.6 + 28.0;
            }
            if e.arrow {
                w += 20.0;
            }
            if e.checked.is_some() {
                w += 24.0;
            }
            w
        })
        .fold(0.0_f32, f32::max);
    widest.clamp(210.0, 520.0)
}

struct MenuBuilder {
    ctx: Ctx,
    key: String,
    hover: HoverMode,
    out: Vec<Entry>,
}

impl MenuBuilder {
    fn new(ctx: Ctx, key: String, hover: HoverMode) -> Self {
        Self {
            ctx,
            key,
            hover,
            out: Vec::new(),
        }
    }

    fn push(&mut self, mut e: Entry) {
        e.key = self.key.clone();
        if e.on_hover.is_none() && !e.separator {
            let ctx = self.ctx;
            e.on_hover = match self.hover {
                HoverMode::None => None,
                HoverMode::CloseL3 => Some(Rc::new(move || ctx.l3_open.set(false))),
                HoverMode::Dismiss => Some(Rc::new(move || ctx.dismiss_submenus())),
            };
        }
        self.out.push(e);
    }

    fn sep(&mut self) {
        if self.out.last().is_some_and(|e| !e.separator) {
            self.push(Entry::sep());
        }
    }

    fn git(&mut self, label: impl Into<String>, args: Vec<String>) {
        let ctx = self.ctx;
        self.push(Entry::item(label, move || ctx.git(args.clone())));
    }

    fn finish(mut self) -> Vec<Entry> {
        while self.out.last().is_some_and(|e| e.separator) {
            self.out.pop();
        }
        self.out
    }
}

fn worktree_args(from: &str) -> Vec<String> {
    sv![
        "worktree",
        "add",
        "-b",
        "new-worktree-branch",
        "../new-worktree",
        from
    ]
}

/// Second-level menu for a branch / tag row.
fn ref_menu(ctx: Ctx, t: &Target) -> Vec<Entry> {
    let cur = ctx.current_branch.get_untracked();
    let name = t.name.clone();
    let mut m = MenuBuilder::new(
        ctx,
        format!("l2:{:?}:{}:{}", t.kind, t.name, t.is_current),
        HoverMode::CloseL3,
    );

    match t.kind {
        RefKind::Local => {
            let upstream = t.upstream.clone();
            if !t.is_current {
                let n = name.clone();
                m.push(Entry::item("Checkout", move || ctx.checkout(&n)));
                m.git(
                    format!("New Branch from '{name}'…"),
                    sv!["switch", "-c", "new-branch", &name],
                );
                m.git(
                    format!("Checkout and Rebase onto '{cur}'"),
                    sv!["rebase", &cur, &name],
                );
                m.sep();
                m.git(
                    format!("Compare with '{cur}'"),
                    sv![
                        "log",
                        "--oneline",
                        "--left-right",
                        "--graph",
                        format!("{cur}...{name}")
                    ],
                );
                m.git("Show Diff with Working Tree", sv!["diff", &name]);
                m.git(
                    format!("New Worktree from '{name}'…"),
                    worktree_args(&name),
                );
                m.sep();
                m.git(
                    format!("Rebase '{cur}' onto '{name}'"),
                    sv!["rebase", &name],
                );
                m.git(
                    format!("Merge '{name}' into '{cur}'"),
                    sv!["merge", "--no-edit", &name],
                );
            } else {
                m.git(
                    format!("New Branch from '{name}'…"),
                    sv!["switch", "-c", "new-branch"],
                );
                m.git("Show Diff with Working Tree", sv!["diff", "HEAD"]);
                m.git(
                    format!("New Worktree from '{name}'…"),
                    worktree_args(&name),
                );
            }
            m.sep();

            // Update
            let ctx_u = ctx;
            match (&upstream, t.is_current) {
                (_, true) => m.git("Update", sv!["pull", "--ff-only"]),
                (Some(up), false) => {
                    let (remote, branch) = split_remote(up);
                    m.git(
                        "Update",
                        sv!["fetch", remote, format!("{branch}:{name}")],
                    )
                }
                (None, false) => {
                    m.push(Entry::item("Update", move || ctx_u.close()).enabled(false))
                }
            }

            // Push
            let push_args = match (&upstream, t.is_current) {
                (Some(_), true) => sv!["push"],
                (None, true) => sv!["push", "-u", "origin", &name],
                (Some(up), false) => sv!["push", split_remote(up).0, &name],
                (None, false) => sv!["push", "-u", "origin", &name],
            };
            let push_label = "Push…";
            let ctx_p = ctx;
            let mut push = Entry::item(push_label, move || ctx_p.git(push_args.clone()));
            if t.is_current {
                push = push.shortcut("Ctrl+Shift+K");
            }
            m.push(push);

            if let Some(up) = &upstream {
                m.push(Entry::item(format!("Tracked Branch '{up}'"), || {}).arrow());
            }
            m.sep();
            m.git(
                "Rename…",
                sv!["branch", "-m", &name, "new-name"],
            );
            if let Some(last) = m.out.last_mut() {
                last.shortcut = "F2";
            }
            if !t.is_current {
                m.git("Delete", sv!["branch", "-d", &name]);
                if let Some(last) = m.out.last_mut() {
                    last.shortcut = "Delete";
                }
            }
        }
        RefKind::Remote => {
            let (remote, branch) = split_remote(&name);
            m.git(
                "Checkout",
                sv!["switch", "--track", &name],
            );
            m.git(
                format!("New Branch from '{name}'…"),
                sv!["switch", "-c", "new-branch", &name],
            );
            m.sep();
            m.git(
                format!("Compare with '{cur}'"),
                sv![
                    "log",
                    "--oneline",
                    "--left-right",
                    "--graph",
                    format!("{cur}...{name}")
                ],
            );
            m.git("Show Diff with Working Tree", sv!["diff", &name]);
            m.git(
                format!("New Worktree from '{name}'…"),
                worktree_args(&name),
            );
            m.sep();
            m.git(
                format!("Rebase '{cur}' onto '{name}'"),
                sv!["rebase", &name],
            );
            m.git(
                format!("Merge '{name}' into '{cur}'"),
                sv!["merge", "--no-edit", &name],
            );
            m.sep();
            m.git(
                format!("Pull into '{cur}' Using Rebase"),
                sv!["pull", "--rebase", &remote, &branch],
            );
            m.git(
                format!("Pull into '{cur}' Using Merge"),
                sv!["pull", "--no-rebase", &remote, &branch],
            );
            m.sep();
            m.git("Delete", sv!["push", &remote, "--delete", &branch]);
            if let Some(last) = m.out.last_mut() {
                last.shortcut = "Delete";
            }
        }
        RefKind::Tag => {
            let n = name.clone();
            m.push(Entry::item("Checkout", move || ctx.checkout(&n)));
            m.git(
                format!("New Branch from '{name}'…"),
                sv!["switch", "-c", "new-branch", &name],
            );
            m.git("Show Diff with Working Tree", sv!["diff", &name]);
            m.sep();
            m.git("Delete", sv!["tag", "-d", &name]);
            if let Some(last) = m.out.last_mut() {
                last.shortcut = "Delete";
            }
        }
    }

    let mut v = m.finish();
    // Wire the "Tracked Branch" entry to the third level.
    if let Some(i) = v.iter().position(|e| e.arrow) {
        let off = offset_of(&v, i);
        v[i].on_hover = Some(Rc::new(move || {
            ctx.l3_top.set(ctx.l2_top.get_untracked() + off);
            ctx.l3_open.set(true);
        }));
        v[i].active = Some(Rc::new(move || ctx.l3_open.get()));
    }
    v
}

/// Third-level menu: actions on the tracked remote branch of a local branch.
fn tracked_menu(ctx: Ctx, t: &Target) -> Vec<Entry> {
    let (RefKind::Local, Some(up)) = (t.kind, t.upstream.clone()) else {
        return Vec::new();
    };
    let cur = ctx.current_branch.get_untracked();
    let name = t.name.clone();
    let (remote, branch) = split_remote(&up);
    let mut m = MenuBuilder::new(
        ctx,
        format!("l3:{}:{}:{}", t.name, up, t.is_current),
        HoverMode::None,
    );

    let n = name.clone();
    let mut checkout = Entry::item("Checkout", move || ctx.checkout(&n));
    checkout = checkout.enabled(!t.is_current);
    m.push(checkout);
    m.git(
        format!("New Branch from '{up}'…"),
        sv!["switch", "-c", "new-branch", &up],
    );
    m.sep();
    m.git(
        format!("Compare with '{name}'"),
        sv![
            "log",
            "--oneline",
            "--left-right",
            "--graph",
            format!("{name}...{up}")
        ],
    );
    m.git("Show Diff with Working Tree", sv!["diff", &up]);
    m.sep();
    if t.is_current {
        m.git(
            format!("Rebase '{name}' onto '{up}'"),
            sv!["rebase", &up],
        );
        m.git(
            format!("Merge '{up}' into '{name}'"),
            sv!["merge", "--no-edit", &up],
        );
    } else {
        m.git(
            format!("Rebase '{name}' onto '{up}'"),
            sv!["rebase", &up, &name],
        );
        let ctx_c = ctx;
        m.push(
            Entry::item(format!("Merge '{up}' into '{name}'"), move || ctx_c.close())
                .enabled(false),
        );
    }
    m.sep();
    m.git(
        format!("New Worktree from '{name}'…"),
        worktree_args(&name),
    );
    m.sep();
    let ctx_c = ctx;
    if t.is_current {
        m.git(
            format!("Pull into '{name}' Using Rebase"),
            sv!["pull", "--rebase", &remote, &branch],
        );
        m.git(
            format!("Pull into '{name}' Using Merge"),
            sv!["pull", "--no-rebase", &remote, &branch],
        );
    } else {
        m.push(
            Entry::item(format!("Pull into '{name}' Using Rebase"), move || {
                ctx_c.close()
            })
                .enabled(false),
        );
        m.push(
            Entry::item(format!("Pull into '{name}' Using Merge"), move || {
                ctx_c.close()
            })
                .enabled(false),
        );
    }
    m.sep();
    let _ = cur;
    let mut delete = Entry::item("Delete", {
        let args = sv!["push", &remote, "--delete", &branch];
        move || ctx.git(args.clone())
    })
        .shortcut("Delete");
    delete = delete.enabled(!t.is_current);
    m.push(delete);
    m.finish()
}

fn top_actions(ctx: Ctx) -> Vec<Entry> {
    let mut m = MenuBuilder::new(ctx, "top".into(), HoverMode::Dismiss);
    m.push(
        Entry::item("Update Project...", move || {
            ctx.git(sv!["pull", "--ff-only"])
        })
            .icon(Icon::Raw(ic::ARROW_DOWN_LEFT))
            .shortcut("Ctrl+T"),
    );
    m.push(
        Entry::item("Commit...", move || {
            ctx.workbench(SparrowWorkbenchCommand::SourceControlCommit)
        })
            .icon(Icon::Raw(ic::COMMIT))
            .shortcut("Ctrl+K"),
    );
    m.push(
        Entry::item("Push...", move || ctx.git(sv!["push"]))
            .icon(Icon::Raw(ic::ARROW_UP_RIGHT))
            .shortcut("Ctrl+Shift+K"),
    );
    m.finish()
}

fn new_actions(ctx: Ctx) -> Vec<Entry> {
    let mut m = MenuBuilder::new(ctx, "new".into(), HoverMode::Dismiss);
    m.push(
        Entry::item("New Branch...", move || {
            ctx.git(sv!["switch", "-c", "new-branch"])
        })
            .icon(Icon::Raw(ic::PLUS))
            .shortcut("Ctrl+Alt+N"),
    );
    m.push(
        Entry::item("New Worktree...", move || {
            ctx.git(sv!["worktree", "add", "../new-worktree"])
        })
            .icon(Icon::Raw(ic::FORK)),
    );
    m.push(
        Entry::item("Checkout Tag or Revision...", move || {
            ctx.workbench(SparrowWorkbenchCommand::PaletteSCMReferences)
        })
            .slot(),
    );
    m.finish()
}

fn settings_entries(ctx: Ctx, show: Show) -> Vec<Entry> {
    let mut m = MenuBuilder::new(ctx, "settings".into(), HoverMode::None);
    for (title, sig) in [
        ("Show Recent Branches", show.recent),
        ("Show Remote Branches", show.remote),
        ("Show Tags", show.tags),
    ] {
        m.push(
            Entry::item(title, move || sig.update(|v| *v = !*v))
                .checked(sig.get_untracked()),
        );
    }
    m.finish()
}

// ───────────────────────────── entry rendering ─────────────────────────────

fn entry_view(config: ReadSignal<Arc<SparrowConfig>>, e: Entry) -> AnyView {
    if e.separator {
        return container(empty().style(move |s| {
            s.height(1.0)
                .margin_vert(4.0)
                .margin_horiz(6.0)
                .background(config.get().color(SparrowColor::SPARROW_BORDER))
        }))
            .into_any();
    }

    let Entry {
        label: text,
        shortcut,
        enabled,
        arrow,
        checked,
        icon,
        slot,
        action,
        on_hover,
        active,
        ..
    } = e;

    let has_slot = slot || icon.is_some() || checked.is_some();
    let text_color = move |c: &SparrowConfig| if enabled { fg(c) } else { dim(c) };

    let icon_view: AnyView = match (checked, icon) {
        (Some(on), _) => raw_icon(ic::CHECK, 14.0, move || {
            if on {
                fg(&config.get())
            } else {
                Color::TRANSPARENT
            }
        })
            .into_any(),
        (None, Some(Icon::Raw(body))) => {
            raw_icon(body, 14.0, move || text_color(&config.get())).into_any()
        }
        (None, Some(Icon::Theme(name))) => {
            theme_icon(config, name, 14.0, move |c| text_color(c)).into_any()
        }
        (None, None) => empty().into_any(),
    };

    stack((
        container(icon_view).style(move |s| {
            s.width(if has_slot { 16.0 } else { 0.0 })
                .height(16.0)
                .margin_right(if has_slot { 8.0 } else { 0.0 })
                .items_center()
                .justify_center()
                .flex_shrink(0.0_f32)
        }),
        label(move || text.clone()).style(move |s| {
            s.color(text_color(&config.get()))
                .font_size(13.0)
                .selectable(false)
        }),
        empty().style(|s| s.flex_grow(1.0).min_width(16.0)),
        label(move || shortcut.to_string()).style(move |s| {
            s.color(dim(&config.get())).font_size(13.0).selectable(false)
        }),
        theme_icon(config, SparrowIcons::ITEM_CLOSED, 12.0, move |c| text_color(c))
            .style(move |s| s.margin_left(8.0).apply_if(!arrow, |s| s.hide())),
    ))
        .on_click_stop(move |_| {
            if enabled {
                if let Some(a) = &action {
                    a();
                }
            }
        })
        .on_event_stop(EventListener::PointerEnter, move |_| {
            if let Some(h) = &on_hover {
                h();
            }
        })
        .style(move |s| {
            let c = config.get();
            let is_active = active.as_ref().is_some_and(|f| f());
            let sel = selected_bg(&c);
            s.height(ROW_H)
                .flex_shrink(0.0_f32)
                .items_center()
                .padding_horiz(8.0)
                .border_radius(4.0)
                .apply_if(is_active, |s| s.background(sel))
                .apply_if(enabled, |s| {
                    s.hover(|s| s.background(sel).cursor(CursorStyle::Pointer))
                })
        })
        .into_any()
}

fn entry_list(
    config: ReadSignal<Arc<SparrowConfig>>,
    entries: impl Fn() -> Vec<Entry> + 'static,
) -> impl View {
    dyn_stack(
        move || entries().into_iter().enumerate().collect::<Vec<_>>(),
        |(i, e)| format!("{}|{i}|{}|{}|{:?}", e.key, e.label, e.enabled, e.checked),
        move |(_, e)| entry_view(config, e),
    )
        .style(|s| s.flex_col().width_full())
}

/// Floating menu chrome (2nd/3rd level and settings menus).
fn floating_menu(
    config: ReadSignal<Arc<SparrowConfig>>,
    entries: RwSignal<Vec<Entry>>,
    visible: impl Fn() -> bool + 'static,
    left: impl Fn() -> f32 + 'static,
    top: impl Fn() -> f32 + 'static,
) -> impl View {
    container(entry_list(config, move || entries.get()))
        .on_click_stop(|_| {})
        .style(move |s| {
            let c = config.get();
            let width = entries.with(|e| menu_width(e));
            let show = visible() && entries.with(|e| !e.is_empty());
            s.absolute()
                .margin_left(left())
                .margin_top(top())
                .width(width)
                .padding(MENU_PAD)
                .background(c.color(SparrowColor::PANEL_BACKGROUND))
                .border(1.0)
                .border_radius(8.0)
                .border_color(c.color(SparrowColor::SPARROW_BORDER))
                .box_shadow_blur(14.0)
                .box_shadow_v_offset(4.0)
                .box_shadow_color(c.color(SparrowColor::SPARROW_DROPDOWN_SHADOW))
                .apply_if(!show, |s| s.hide())
        })
}

// ───────────────────────────── tree rows ─────────────────────────────

fn header_row(ctx: Ctx, section: Section) -> AnyView {
    let config = ctx.config;
    let toggle = ctx.sections.signal(section);
    let search = ctx.search;
    let is_open = move || !search.get().is_empty() || toggle.get();

    stack((
        svg(move || {
            config.get().ui_svg(if is_open() {
                SparrowIcons::ITEM_OPENED
            } else {
                SparrowIcons::ITEM_CLOSED
            })
        })
            .style(move |s| {
                s.size(12.0, 12.0)
                    .margin_right(8.0)
                    .flex_shrink(0.0_f32)
                    .color(dim(&config.get()))
            }),
        label(move || section.title().to_string()).style(move |s| {
            s.font_size(13.0).color(fg(&config.get())).selectable(false)
        }),
    ))
        .on_click_stop(move |_| {
            if search.get_untracked().is_empty() {
                toggle.update(|v| *v = !*v);
            }
        })
        .on_event_stop(EventListener::PointerEnter, move |_| {
            ctx.dismiss_submenus();
        })
        .style(move |s| {
            s.height(ROW_H)
                .margin_horiz(12.0)
                .padding_left(10.0)
                .padding_right(8.0)
                .items_center()
                .border_radius(4.0)
                .hover(|s| s.cursor(CursorStyle::Pointer))
        })
        .into_any()
}

fn ref_row(ctx: Ctx, target: Target, ahead: u32, behind: u32, idx: usize) -> AnyView {
    let config = ctx.config;
    let kind = target.kind;
    let is_current = target.is_current;
    let is_default =
        kind == RefKind::Local && matches!(target.name.as_str(), "main" | "master");
    let upstream_text = if kind == RefKind::Local {
        target.upstream.clone().unwrap_or_default()
    } else {
        String::new()
    };
    let name = target.name.clone();

    let icon: AnyView = if is_default {
        raw_icon(ic::STAR, 14.0, amber).into_any()
    } else if is_current {
        raw_icon(ic::TAG, 14.0, amber).into_any()
    } else if kind == RefKind::Tag {
        raw_icon(ic::TAG, 14.0, move || dim(&config.get())).into_any()
    } else {
        theme_icon(config, SparrowIcons::SCM_BRANCH, 14.0, dim).into_any()
    };

    let is_selected =
        move || ctx.selected.with(|s| matches!(s, Some((i, _)) if *i == idx));

    let enter_target = target.clone();
    let click_target = target;

    stack((
        container(icon).style(|s| {
            s.width(16.0)
                .height(16.0)
                .margin_right(8.0)
                .items_center()
                .justify_center()
                .flex_shrink(0.0_f32)
        }),
        label(move || name.clone()).style(move |s| {
            s.font_size(13.0)
                .min_width(0.0)
                .text_ellipsis()
                .color(fg(&config.get()))
                .selectable(false)
        }),
        stack((
            raw_icon(ic::ARROW_UP_RIGHT, 11.0, green),
            label(move || ahead.to_string())
                .style(|s| s.font_size(12.0).color(green()).selectable(false)),
        ))
            .style(move |s| {
                s.items_center()
                    .gap(2.0)
                    .margin_left(8.0)
                    .flex_shrink(0.0_f32)
                    .apply_if(ahead == 0, |s| s.hide())
            }),
        stack((
            raw_icon(ic::ARROW_DOWN_LEFT, 11.0, blue),
            label(move || behind.to_string())
                .style(|s| s.font_size(12.0).color(blue()).selectable(false)),
        ))
            .style(move |s| {
                s.items_center()
                    .gap(2.0)
                    .margin_left(6.0)
                    .flex_shrink(0.0_f32)
                    .apply_if(behind == 0, |s| s.hide())
            }),
        empty().style(|s| s.flex_grow(1.0).min_width(8.0)),
        label(move || upstream_text.clone()).style(move |s| {
            s.font_size(13.0)
                .color(dim(&config.get()))
                .margin_right(8.0)
                .selectable(false)
        }),
        theme_icon(config, SparrowIcons::ITEM_CLOSED, 12.0, dim),
    ))
        .on_event_stop(EventListener::PointerEnter, move |_| {
            ctx.select(idx, enter_target.clone());
        })
        .on_click_stop(move |_| match click_target.kind {
            RefKind::Local => {
                if click_target.is_current {
                    ctx.close();
                } else {
                    ctx.checkout(&click_target.name);
                }
            }
            RefKind::Remote => ctx.git(sv!["switch", "--track", &click_target.name]),
            RefKind::Tag => ctx.checkout(&click_target.name),
        })
        .style(move |s| {
            let c = config.get();
            let sel = selected_bg(&c);
            s.height(ROW_H)
                .margin_horiz(12.0)
                .padding_left(48.0)
                .padding_right(8.0)
                .items_center()
                .border_radius(4.0)
                .apply_if(is_selected(), |s| s.background(sel))
                .hover(|s| s.cursor(CursorStyle::Pointer))
        })
        .into_any()
}

// ───────────────────────────── small widgets ─────────────────────────────

fn icon_button(
    config: ReadSignal<Arc<SparrowConfig>>,
    icon: impl View + 'static,
    on_click: impl Fn() + 'static,
) -> impl View {
    container(icon)
        .on_click_stop(move |_| on_click())
        .style(move |s| {
            let c = config.get();
            s.size(24.0, 24.0)
                .flex_shrink(0.0_f32)
                .items_center()
                .justify_center()
                .border_radius(4.0)
                .hover(|s| s.background(hover_bg(&c)).cursor(CursorStyle::Pointer))
        })
}

// ───────────────────────────── the popup ─────────────────────────────

pub fn branch_switcher(
    window_tab_data: Rc<WindowTabData>,
    origin: RwSignal<Point>,
) -> impl View {
    let visible = window_tab_data.branch_menu_visible;
    let config = window_tab_data.common.config;
    let source_control = window_tab_data.source_control.clone();
    let current_branch = source_control.branch.read_only();
    let branches = source_control.branches.read_only();
    let tags = source_control.tags.read_only();
    let title_height = window_tab_data.title_height;
    let window_origin = window_tab_data.common.window_origin;

    // Local editor backing the search field.
    let cx = Scope::current();
    let search_editor = window_tab_data
        .main_split
        .editors
        .make_local(cx, window_tab_data.common.clone());
    let search_buffer = search_editor.doc_signal();
    let search_text = create_memo(move |_| {
        search_buffer.with(|d| d.buffer.with(|b| b.to_string().to_lowercase()))
    });

    // ── state ──
    let sections = Sections {
        recent: RwSignal::new(true),
        local: RwSignal::new(true),
        remote: RwSignal::new(false),
        tags: RwSignal::new(false),
    };
    let show = Show {
        recent: RwSignal::new(true),
        remote: RwSignal::new(true),
        tags: RwSignal::new(true),
    };
    let ctx = Ctx {
        config,
        sparrow_command: window_tab_data.common.sparrow_command,
        workbench_command: window_tab_data.common.workbench_command,
        visible,
        current_branch,
        search: search_text,
        sections,
        selected: RwSignal::new(None),
        l2_top: RwSignal::new(0.0),
        l3_open: RwSignal::new(false),
        l3_top: RwSignal::new(0.0),
        scroll_y: RwSignal::new(0.0),
        settings_open: RwSignal::new(false),
    };

    // ── git snapshot (upstreams, ahead/behind, recent branches) ──
    let snapshot: RwSignal<Option<Snapshot>> = RwSignal::new(None);
    let workspace_root = window_tab_data.workspace.path.clone();
    let workspace_is_local = window_tab_data.workspace.kind.is_local();
    let scope = window_tab_data.scope;

    // Reset on open: clear search, close submenus, refresh git info.
    create_effect({
        let search_editor = search_editor.clone();
        move |_| {
            if visible.get() {
                search_editor
                    .doc()
                    .reload(lapce_xi_rope::Rope::from(""), true);
                ctx.dismiss_submenus();
                ctx.settings_open.set(false);
                ctx.scroll_y.set(0.0);
                if workspace_is_local {
                    if let Some(root) = workspace_root.clone() {
                        let send = create_ext_action(scope, move |snap: Snapshot| {
                            snapshot.set(Some(snap));
                        });
                        std::thread::spawn(move || send(load_snapshot(&root)));
                    }
                }
            }
        }
    });

    // ── tree rows ──
    let rows = create_memo(move |_| {
        let q = search_text.get();
        let searching = !q.is_empty();
        let current = current_branch.get();
        let snap = snapshot.get();
        let all = branches.get();
        let tag_names = tags.get();

        let (mut locals, remotes) = match snap.as_ref() {
            Some(s) => (s.locals.clone(), s.remotes.clone()),
            None => {
                // Fallback until `git for-each-ref` returns.
                let mut l = Vec::new();
                let mut r = Vec::new();
                for b in all.iter() {
                    if b.contains('/') {
                        r.push(b.clone());
                    } else {
                        l.push(LocalInfo {
                            name: b.clone(),
                            upstream: None,
                            ahead: 0,
                            behind: 0,
                        });
                    }
                }
                (l, r)
            }
        };
        let recent_names: Vec<String> = match snap.as_ref() {
            Some(s) if !s.recent.is_empty() => s.recent.clone(),
            _ => vec![current.clone()],
        };

        let matches = |n: &str| !searching || n.to_lowercase().contains(&q);

        let rank = |n: &str| -> u8 {
            if n == current {
                0
            } else if n == "main" || n == "master" {
                1
            } else {
                2
            }
        };
        locals.sort_by(|a, b| {
            rank(&a.name)
                .cmp(&rank(&b.name))
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });

        let local_item = |info: &LocalInfo| -> (Target, u32, u32) {
            (
                Target {
                    kind: RefKind::Local,
                    name: info.name.clone(),
                    upstream: info.upstream.clone(),
                    is_current: info.name == current,
                },
                info.ahead,
                info.behind,
            )
        };

        let mut out: Vec<Row> = Vec::new();
        let section = |out: &mut Vec<Row>,
                       sec: Section,
                       items: Vec<(Target, u32, u32)>| {
            if items.is_empty() {
                return;
            }
            out.push(Row::Header { section: sec, idx: 0 });
            if searching || sections.signal(sec).get() {
                for (target, ahead, behind) in items {
                    out.push(Row::Ref {
                        target,
                        ahead,
                        behind,
                        idx: 0,
                    });
                }
            }
        };

        if !searching && show.recent.get() {
            let items = recent_names
                .iter()
                .filter_map(|n| locals.iter().find(|l| &l.name == n))
                .map(&local_item)
                .collect();
            section(&mut out, Section::Recent, items);
        }
        let items = locals
            .iter()
            .filter(|l| matches(&l.name))
            .map(&local_item)
            .collect();
        section(&mut out, Section::Local, items);

        if show.remote.get() {
            let mut remotes = remotes;
            remotes.sort_by_key(|r| r.to_lowercase());
            let items = remotes
                .into_iter()
                .filter(|r| matches(r))
                .map(|name| {
                    (
                        Target {
                            kind: RefKind::Remote,
                            name,
                            upstream: None,
                            is_current: false,
                        },
                        0,
                        0,
                    )
                })
                .collect();
            section(&mut out, Section::Remote, items);
        }
        if show.tags.get() {
            let items = tag_names
                .iter()
                .filter(|t| matches(t))
                .map(|name| {
                    (
                        Target {
                            kind: RefKind::Tag,
                            name: name.clone(),
                            upstream: None,
                            is_current: false,
                        },
                        0,
                        0,
                    )
                })
                .collect();
            section(&mut out, Section::Tags, items);
        }

        for (i, row) in out.iter_mut().enumerate() {
            match row {
                Row::Header { idx, .. } | Row::Ref { idx, .. } => *idx = i,
            }
        }
        out
    });

    // Rows changed under an open submenu → close it.
    create_effect(move |_| {
        rows.track();
        ctx.dismiss_submenus();
    });

    // ── floating menu contents ──
    let l2_entries: RwSignal<Vec<Entry>> = RwSignal::new(Vec::new());
    let l3_entries: RwSignal<Vec<Entry>> = RwSignal::new(Vec::new());
    let settings_menu: RwSignal<Vec<Entry>> = RwSignal::new(Vec::new());

    create_effect(move |_| match ctx.selected.get() {
        Some((_, t)) => {
            l2_entries.set(ref_menu(ctx, &t));
            l3_entries.set(tracked_menu(ctx, &t));
        }
        None => {
            l2_entries.set(Vec::new());
            l3_entries.set(Vec::new());
        }
    });
    create_effect(move |_| {
        // re-run when a toggle flips so the check marks update
        show.recent.track();
        show.remote.track();
        show.tags.track();
        settings_menu.set(settings_entries(ctx, show));
    });

    // Panel position (same anchoring as before: under the branch chip).
    let panel_pos = move || {
        let pt = origin.get() - window_origin.get().to_vec2();
        (pt.x, title_height.get() as f64 + 2.0)
    };

    // ── search row ──
    let search_row = stack((
        container(stack((
            theme_icon(config, SparrowIcons::SEARCH, 14.0, dim)
                .style(|s| s.margin_right(8.0)),
            TextInputBuilder::new()
                .build_editor(search_editor)
                .placeholder(|| "Search for branches and actions".to_string())
                .style(|s| {
                    s.flex_grow(1.0)
                        .min_width(0.0)
                        .height(24.0)
                        .padding_horiz(0.0)
                        .border(0.0)
                        .background(Color::TRANSPARENT)
                }),
        ))
            .style(|s| s.items_center().width_full()))
            .style(move |s| {
                let c = config.get();
                s.flex_grow(1.0)
                    .min_width(0.0)
                    .height(28.0)
                    .padding_horiz(8.0)
                    .items_center()
                    .border(1.0)
                    .border_radius(5.0)
                    .border_color(c.color(SparrowColor::SPARROW_BORDER))
                    .background(c.color(SparrowColor::EDITOR_BACKGROUND))
            }),
        icon_button(
            config,
            raw_icon(ic::ARROW_DOWN_LEFT, 15.0, move || fg(&config.get())),
            move || ctx.git(sv!["fetch", "--all", "--prune"]),
        ),
        icon_button(
            config,
            theme_icon(config, SparrowIcons::SETTINGS, 15.0, fg),
            move || {
                ctx.dismiss_submenus();
                ctx.settings_open.update(|v| *v = !*v);
            },
        ),
    ))
        .style(|s| {
            s.height(SEARCH_H as f32)
                .flex_shrink(0.0_f32)
                .padding_horiz(12.0)
                .items_center()
                .gap(6.0)
        });

    let divider = move || {
        Divider::new(config)
            .spacing(4.0)
            .indent(0.0)
            .into_view()
    };

    // ── tree ──
    let tree = dyn_stack(
        move || rows.get(),
        |row| format!("{row:?}"),
        move |row| match row {
            Row::Header { section, .. } => header_row(ctx, section),
            Row::Ref {
                target,
                ahead,
                behind,
                idx,
            } => ref_row(ctx, target, ahead, behind, idx),
        },
    )
        .style(|s| s.flex_col().width_full().padding_vert(TREE_PAD as f32));

    let tree_scroll = sparrow_scroll(tree)
        .on_scroll(move |rect| {
            if (rect.y0 - ctx.scroll_y.get_untracked()).abs() > f64::EPSILON {
                ctx.scroll_y.set(rect.y0);
                ctx.dismiss_submenus();
            }
        })
        .style(|s| s.max_height(TREE_MAX_H).width_full().flex_shrink(1.0_f32));

    // ── panel ──
    let panel = container(
        stack((
            search_row,
            divider(),
            entry_list(config, move || top_actions(ctx)).style(|s| {
                s.flex_col().width_full().padding_horiz(12.0).flex_shrink(0.0_f32)
            }),
            divider(),
            entry_list(config, move || new_actions(ctx)).style(|s| {
                s.flex_col().width_full().padding_horiz(12.0).flex_shrink(0.0_f32)
            }),
            divider(),
            tree_scroll,
        ))
            .style(|s| s.flex_col().width_full()),
    )
        .keyboard_navigable()
        .on_key_down(
            Key::Named(NamedKey::Escape),
            |m| m.is_empty(),
            move |_| ctx.close(),
        )
        .on_click_stop(|_| {})
        .style(move |s| {
            let c = config.get();
            let (px, py) = panel_pos();
            s.absolute()
                .margin_left(px as f32)
                .margin_top(py as f32)
                .width(PANEL_W as f32)
                .padding(0.0)
                .background(c.color(SparrowColor::PANEL_BACKGROUND))
                .border(1.0)
                .border_radius(10.0)
                .border_color(c.color(SparrowColor::SPARROW_BORDER))
                .box_shadow_blur(16.0)
                .box_shadow_v_offset(4.0)
                .box_shadow_color(c.color(SparrowColor::SPARROW_DROPDOWN_SHADOW))
        });

    // ── floating menus ──
    let l2_menu = floating_menu(
        config,
        l2_entries,
        move || ctx.selected.with(Option::is_some),
        move || (panel_pos().0 + PANEL_W - 2.0) as f32,
        move || (panel_pos().1 + ctx.l2_top.get()) as f32,
    );
    let l3_menu = floating_menu(
        config,
        l3_entries,
        move || ctx.l3_open.get() && ctx.selected.with(Option::is_some),
        move || {
            let l2_w = l2_entries.with(|e| menu_width(e)) as f64;
            (panel_pos().0 + PANEL_W - 2.0 + l2_w - 2.0) as f32
        },
        move || (panel_pos().1 + ctx.l3_top.get()) as f32,
    );
    let settings_menu_view = floating_menu(
        config,
        settings_menu,
        move || ctx.settings_open.get(),
        move || {
            let w = settings_menu.with(|e| menu_width(e)) as f64;
            (panel_pos().0 + PANEL_W - w - 12.0) as f32
        },
        move || (panel_pos().1 + 40.0) as f32,
    );

    stack((
        // Catches clicks outside the popup.
        empty()
            .on_click_stop(move |_| ctx.close())
            .style(|s| s.absolute().size_full()),
        panel,
        l2_menu,
        l3_menu,
        settings_menu_view,
    ))
        .style(move |s| {
            s.absolute()
                .size_full()
                .display(if visible.get() {
                    Display::Flex
                } else {
                    Display::None
                })
                .z_index(100)
        })
}

// ───────────────────────────── tests ─────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn track_parsing() {
        assert_eq!(parse_track(""), (0, 0));
        assert_eq!(parse_track("ahead 5"), (5, 0));
        assert_eq!(parse_track("behind 2"), (0, 2));
        assert_eq!(parse_track("ahead 5, behind 2"), (5, 2));
        assert_eq!(parse_track("gone"), (0, 0));
    }

    #[test]
    fn ref_lines() {
        let l = "refs/heads/sparrow\0origin/sparrow\0ahead 5";
        match parse_ref_line(l) {
            Some(RefLine::Local(i)) => {
                assert_eq!(i.name, "sparrow");
                assert_eq!(i.upstream.as_deref(), Some("origin/sparrow"));
                assert_eq!((i.ahead, i.behind), (5, 0));
            }
            _ => panic!("expected local"),
        }
        match parse_ref_line("refs/heads/feature/x\0\0") {
            Some(RefLine::Local(i)) => {
                assert_eq!(i.name, "feature/x");
                assert!(i.upstream.is_none());
            }
            _ => panic!("expected local"),
        }
        assert!(matches!(
            parse_ref_line("refs/remotes/origin/master\0\0"),
            Some(RefLine::Remote(n)) if n == "origin/master"
        ));
        assert!(parse_ref_line("refs/remotes/origin/HEAD\0\0").is_none());
        assert!(parse_ref_line("refs/tags/v1\0\0").is_none());
    }

    #[test]
    fn recent_from_reflog() {
        let locals = ["sparrow", "master", "old"]
            .map(|n| LocalInfo {
                name: n.into(),
                upstream: None,
                ahead: 0,
                behind: 0,
            })
            .to_vec();
        let log = "checkout: moving from master to sparrow\n\
                   commit: something\n\
                   checkout: moving from old to master\n\
                   checkout: moving from gone to old";
        assert_eq!(
            parse_recent(log, &locals, 5),
            vec!["sparrow", "master", "old"]
        );
        assert_eq!(parse_recent(log, &locals, 2), vec!["sparrow", "master"]);
    }

    #[test]
    fn remote_split() {
        assert_eq!(
            split_remote("origin/feature/x"),
            ("origin".to_string(), "feature/x".to_string())
        );
        assert_eq!(
            split_remote("main"),
            ("origin".to_string(), "main".to_string())
        );
    }

    #[test]
    fn header_height_matches_layout() {
        // 46 search + 3 * 9 separators + 6 * 22 rows
        assert_eq!(HEADER_H, 205.0);
    }
}