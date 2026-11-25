//! Branches, commit graph, changed files and details backed by local Git.
use super::reference_tools::output;
use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    path::{Path, PathBuf},
};
mod view;

pub use view::panel;

#[derive(Clone, Debug, PartialEq)]
struct Commit {
    hash: String,
    parents: Vec<String>,
    subject: String,
    author: String,
    committer: String,
    date: String,
    refs: String,
    graph: String,
    graph_width: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BranchKind {
    Local,
    Remote,
    Tag,
}

#[derive(Clone, Debug, PartialEq)]
struct Branch {
    name: String,
    reference: String,
    kind: BranchKind,
    current: bool,
    ahead: u32,
    behind: u32,
}

#[derive(Clone, Default, PartialEq)]
struct Log {
    commits: Vec<Commit>,
    branches: Vec<Branch>,
    root: PathBuf,
    error: String,
}
#[derive(Clone, Debug, PartialEq)]
struct ChangedFile {
    status: String,
    path: PathBuf,
}
#[derive(Clone, Default, PartialEq)]
struct Details {
    message: String,
    files: Vec<ChangedFile>,
    error: String,
}

/// Filters applied to the log query.
#[derive(Clone, Default)]
struct Filters {
    branch: String,
    user: String,
    since: String,
    path: String,
    query: String,
    match_case: bool,
    regex: bool,
    date_order: bool,
    limit: usize,
}

const LANE_PALETTE: [&str; 8] = [
    "#8C78E8", "#42B5A4", "#E6AB5D", "#5E9FE6", "#D97CB0", "#A3BD66", "#E07A5F", "#5BC0DE",
];

fn commits(text: &str) -> Vec<Commit> {
    text.lines()
        .filter_map(|line| {
            let parts = line.split('\x1f').collect::<Vec<_>>();
            if parts.len() != 7 {
                return None;
            }
            Some(Commit {
                hash: parts[0].into(),
                parents: parts[1].split_whitespace().map(str::to_string).collect(),
                subject: parts[2].into(),
                author: parts[3].into(),
                date: parts[4].into(),
                refs: parts[5].into(),
                committer: parts[6].into(),
                graph: String::new(),
                graph_width: 0.0,
            })
        })
        .collect()
}

/// Lays out lanes and renders one SVG per row. Each lane keeps its colour for
/// as long as the branch line lives, like the IDE graph.
fn graph(commits: &mut [Commit]) {
    let mut lanes = Vec::<String>::new();
    let mut colors = Vec::<usize>::new();
    let mut next_color = 0usize;
    let x = |lane: usize| 9 + lane * 16;
    let paint = |c: usize| LANE_PALETTE[c % LANE_PALETTE.len()];

    for commit in commits {
        let old = lanes.iter().position(|hash| *hash == commit.hash);
        let index = old.unwrap_or_else(|| {
            lanes.push(commit.hash.clone());
            colors.push(next_color);
            next_color += 1;
            lanes.len() - 1
        });
        let before = lanes.clone();
        let before_colors = colors.clone();
        let commit_color = colors[index];
        lanes.remove(index);
        colors.remove(index);
        for (offset, parent) in commit.parents.iter().enumerate() {
            if !lanes.contains(parent) {
                let at = (index + offset).min(lanes.len());
                lanes.insert(at, parent.clone());
                // First parent continues this line; merged-in parents start a new one.
                let color = if offset == 0 {
                    commit_color
                } else {
                    next_color += 1;
                    next_color - 1
                };
                colors.insert(at, color);
            }
        }

        let mut paths = String::new();
        for (lane, hash) in before.iter().enumerate() {
            if hash == &commit.hash {
                continue;
            }
            if let Some(next) = lanes.iter().position(|item| item == hash) {
                paths.push_str(&format!(
                    "<path stroke='{}' d='M{} 0 C{} 12 {} 12 {} 24'/>",
                    paint(before_colors[lane]),
                    x(lane),
                    x(lane),
                    x(next),
                    x(next)
                ));
            }
        }
        if old.is_some() {
            paths.push_str(&format!(
                "<path stroke='{}' d='M{} 0 V12'/>",
                paint(commit_color),
                x(index)
            ));
        }
        for parent in &commit.parents {
            if let Some(next) = lanes.iter().position(|item| item == parent) {
                paths.push_str(&format!(
                    "<path stroke='{}' d='M{} 12 C{} 20 {} 18 {} 24'/>",
                    paint(colors[next]),
                    x(index),
                    x(index),
                    x(next),
                    x(next)
                ));
            }
        }
        let width = before.len().max(lanes.len()).max(index + 1) * 16 + 6;
        commit.graph_width = width as f32;
        let merge = commit.parents.len() > 1;
        commit.graph = format!(
            "<svg xmlns='http://www.w3.org/2000/svg' width='{width}' height='24' viewBox='0 0 {width} 24'><g fill='none' stroke-width='2' stroke-linecap='round'>{paths}</g><circle cx='{}' cy='12' r='{}' fill='{}' stroke='{}' stroke-width='1.5'/></svg>",
            x(index),
            if merge { 4.0 } else { 3.5 },
            if merge { "#1E1F22" } else { paint(commit_color) },
            paint(commit_color)
        );
    }
}

// ───────────────────────── ref chips (HEAD -> main, origin/main, tag: v1) ─────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChipKind {
    Head,
    Local,
    Remote,
    Tag,
}

#[derive(Clone, Debug, PartialEq)]
struct RefChip {
    text: String,
    kind: ChipKind,
}

/// Parses `%D` decorations. `locals` disambiguates `feature/x` from `origin/x`.
fn ref_chips(refs: &str, locals: &HashSet<String>) -> Vec<RefChip> {
    refs.split(", ")
        .filter(|r| !r.is_empty())
        .map(|r| {
            if let Some(name) = r.strip_prefix("HEAD -> ") {
                RefChip {
                    text: name.to_string(),
                    kind: ChipKind::Head,
                }
            } else if r == "HEAD" {
                RefChip {
                    text: "HEAD".into(),
                    kind: ChipKind::Head,
                }
            } else if let Some(tag) = r.strip_prefix("tag: ") {
                RefChip {
                    text: tag.to_string(),
                    kind: ChipKind::Tag,
                }
            } else if locals.contains(r) || !r.contains('/') {
                RefChip {
                    text: r.to_string(),
                    kind: ChipKind::Local,
                }
            } else {
                RefChip {
                    text: r.to_string(),
                    kind: ChipKind::Remote,
                }
            }
        })
        .collect()
}

// ───────────────────────── branch tree ─────────────────────────

#[derive(Clone, Debug, PartialEq)]
enum TreeRow {
    Head,
    Section {
        id: &'static str,
        title: &'static str,
        open: bool,
    },
    Folder {
        id: String,
        name: String,
        depth: usize,
        open: bool,
    },
    Ref {
        branch: Branch,
        name: String,
        depth: usize,
    },
}

fn is_default_name(name: &str) -> bool {
    matches!(name, "main" | "master")
}

fn append_tree(
    items: Vec<(Vec<String>, Branch)>,
    depth: usize,
    id: &str,
    closed: &BTreeSet<String>,
    searching: bool,
    out: &mut Vec<TreeRow>,
) {
    let mut folders: BTreeMap<String, Vec<(Vec<String>, Branch)>> = BTreeMap::new();
    let mut leaves: Vec<(String, Branch)> = Vec::new();
    for (mut segments, branch) in items {
        if segments.len() <= 1 {
            leaves.push((segments.pop().unwrap_or_default(), branch));
        } else {
            let first = segments.remove(0);
            folders.entry(first).or_default().push((segments, branch));
        }
    }
    leaves.sort_by_key(|(name, b)| (!b.current, name.to_lowercase()));
    let pinned = |name: &str, branch: &Branch| {
        branch.current || (branch.kind != BranchKind::Tag && is_default_name(name))
    };
    for (name, branch) in leaves.iter().filter(|(n, b)| pinned(n, b)) {
        out.push(TreeRow::Ref {
            branch: branch.clone(),
            name: name.clone(),
            depth,
        });
    }
    for (name, children) in folders {
        let folder_id = format!("{id}/{name}");
        let open = searching || !closed.contains(&folder_id);
        out.push(TreeRow::Folder {
            id: folder_id.clone(),
            name,
            depth,
            open,
        });
        if open {
            append_tree(children, depth + 1, &folder_id, closed, searching, out);
        }
    }
    for (name, branch) in leaves.iter().filter(|(n, b)| !pinned(n, b)) {
        out.push(TreeRow::Ref {
            branch: branch.clone(),
            name: name.clone(),
            depth,
        });
    }
}

fn build_tree(branches: &[Branch], query: &str, closed: &BTreeSet<String>) -> Vec<TreeRow> {
    let query = query.to_lowercase();
    let searching = !query.is_empty();
    let mut out = vec![TreeRow::Head];
    for (id, title, kind) in [
        ("local", "Local", BranchKind::Local),
        ("remote", "Remote", BranchKind::Remote),
        ("tags", "Tags", BranchKind::Tag),
    ] {
        let items = branches
            .iter()
            .filter(|b| b.kind == kind && b.name.to_lowercase().contains(&query))
            .map(|b| {
                let segments = if kind == BranchKind::Tag {
                    vec![b.name.clone()]
                } else {
                    b.name.split('/').map(str::to_string).collect()
                };
                (segments, b.clone())
            })
            .collect::<Vec<_>>();
        if items.is_empty() && searching {
            continue;
        }
        let open = searching || !closed.contains(id);
        out.push(TreeRow::Section { id, title, open });
        if open {
            // Tags stay flat; remote names become the first folder level.
            if kind == BranchKind::Tag {
                let mut items = items;
                items.sort_by_key(|(_, b)| b.name.to_lowercase());
                for (segments, branch) in items {
                    out.push(TreeRow::Ref {
                        name: segments.into_iter().next().unwrap_or_default(),
                        branch,
                        depth: 0,
                    });
                }
            } else {
                append_tree(items, 0, id, closed, searching, &mut out);
            }
        }
    }
    out
}

fn files(text: &str) -> Vec<ChangedFile> {
    let fields = text
        .split('\0')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();
    fields
        .chunks_exact(2)
        .map(|parts| ChangedFile {
            status: parts[0].into(),
            path: PathBuf::from(parts[1]),
        })
        .collect()
}

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

fn parse_branches(text: &str) -> Vec<Branch> {
    text.lines()
        .filter_map(|line| {
            let parts = line.split('\x1f').collect::<Vec<_>>();
            if parts.len() != 4 {
                return None;
            }
            let reference = parts[0];
            let (kind, name) = if let Some(n) = reference.strip_prefix("refs/heads/") {
                (BranchKind::Local, n)
            } else if let Some(n) = reference.strip_prefix("refs/remotes/") {
                (BranchKind::Remote, n)
            } else if let Some(n) = reference.strip_prefix("refs/tags/") {
                (BranchKind::Tag, n)
            } else {
                return None;
            };
            if kind == BranchKind::Remote && name.ends_with("/HEAD") {
                return None;
            }
            let (ahead, behind) = parse_track(parts[3]);
            Some(Branch {
                name: name.to_string(),
                reference: reference.into(),
                kind,
                current: parts[2] == "*",
                ahead,
                behind,
            })
        })
        .collect()
}

fn load(root: &Path, filters: &Filters) -> Log {
    let real_root = match output(
        Path::new("git"),
        &["rev-parse", "--show-toplevel"],
        Some(root),
    ) {
        Ok(root) => PathBuf::from(root.trim()),
        Err(error) => {
            return Log {
                error,
                ..Default::default()
            };
        }
    };
    let Filters {
        branch,
        user,
        since,
        path,
        query,
        match_case,
        regex,
        date_order,
        limit,
    } = filters;
    let mut args = vec![
        "log".to_string(),
        format!("-{limit}"),
        if *date_order {
            "--date-order".into()
        } else {
            "--topo-order".into()
        },
        "--date=format:%d/%m/%Y, %H:%M".into(),
        "--format=%H%x1f%P%x1f%s%x1f%an%x1f%ad%x1f%D%x1f%cn".into(),
    ];
    let revision = if query.len() >= 4 && query.chars().all(|c| c.is_ascii_hexdigit()) {
        output(
            Path::new("git"),
            &[
                "rev-parse",
                "--verify",
                "--end-of-options",
                &format!("{query}^{{commit}}"),
            ],
            Some(&real_root),
        )
            .ok()
            .map(|value| value.trim().to_string())
    } else {
        None
    };
    if revision.is_none() && !query.is_empty() {
        if !*match_case {
            args.push("--regexp-ignore-case".into());
        }
        args.push(if *regex {
            "--extended-regexp".into()
        } else {
            "--fixed-strings".into()
        });
        args.push(format!("--grep={query}"));
    }
    if !user.is_empty() {
        args.push(format!("--author={user}"));
    }
    if !since.is_empty() {
        args.push(format!("--since={since}"));
    }
    if let Some(revision) = revision {
        args[1] = "-1".into();
        args.push("--end-of-options".into());
        args.push(revision);
    } else if branch.is_empty() {
        args.push("--all".into());
    } else {
        args.push("--end-of-options".into());
        args.push(branch.clone());
    }
    args.push("--".into());
    if !path.is_empty() {
        args.push(path.clone());
    }
    let result = output(
        Path::new("git"),
        &args.iter().map(String::as_str).collect::<Vec<_>>(),
        Some(&real_root),
    );
    let branches = parse_branches(
        &output(
            Path::new("git"),
            &[
                "for-each-ref",
                "--format=%(refname)%1f%(refname:short)%1f%(HEAD)%1f%(upstream:track,nobracket)",
                "refs/heads",
                "refs/remotes",
                "refs/tags",
            ],
            Some(&real_root),
        )
            .unwrap_or_default(),
    );
    match result {
        Ok(text) => {
            let mut commits = commits(&text);
            graph(&mut commits);
            Log {
                commits,
                branches,
                root: real_root,
                error: String::new(),
            }
        }
        Err(error) => Log {
            branches,
            root: real_root,
            error,
            ..Default::default()
        },
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    const F: &str = "\u{1f}";

    fn row(fields: [&str; 7]) -> String {
        fields.join(F)
    }

    #[test]
    fn parses_merge_parents_and_null_delimited_paths() {
        let text = [
            row(["a", "b c", "Merge", "Dev", "Today", "HEAD", "Dev"]),
            row(["b", "d", "One", "Dev", "Today", "", "Other"]),
            row(["c", "d", "Two", "Dev", "Today", "", "Dev"]),
        ]
            .join("\n");
        let mut rows = commits(&text);
        graph(&mut rows);
        assert_eq!(rows[0].parents.len(), 2);
        assert_eq!(rows[1].committer, "Other");
        assert!(rows.iter().all(|row| row.graph.contains("<circle")));
        assert!(rows.iter().all(|row| row.graph_width >= 22.0));
        let files = files("M\0lib/a file.dart\0D\0lib/old.dart\0");
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].path, PathBuf::from("lib/a file.dart"));
    }

    #[test]
    fn lanes_keep_their_colour_while_the_line_lives() {
        // a(merge of b,c) -> b -> c, all on a shared base d.
        let text = [
            row(["a", "b c", "m", "x", "t", "", "x"]),
            row(["b", "d", "b", "x", "t", "", "x"]),
            row(["c", "d", "c", "x", "t", "", "x"]),
            row(["d", "", "d", "x", "t", "", "x"]),
        ]
            .join("\n");
        let mut rows = commits(&text);
        graph(&mut rows);
        // The first parent keeps the commit's colour (lane 0 stays purple).
        let first = LANE_PALETTE[0];
        assert!(rows[0].graph.contains(first));
        assert!(rows[1].graph.contains(first));
        // The merged-in parent runs in a different colour on the next row.
        assert!(rows[1].graph.contains(LANE_PALETTE[1]));
    }

    #[test]
    fn decorations_become_chips() {
        let locals: HashSet<String> = ["main".to_string(), "feature/x".to_string()].into();
        let chips = ref_chips(
            "HEAD -> main, origin/main, tag: v1.0, feature/x, origin/y",
            &locals,
        );
        let kinds: Vec<_> = chips.iter().map(|c| c.kind).collect();
        assert_eq!(
            kinds,
            vec![
                ChipKind::Head,
                ChipKind::Remote,
                ChipKind::Tag,
                ChipKind::Local,
                ChipKind::Remote
            ]
        );
        assert_eq!(chips[2].text, "v1.0");
        assert!(ref_chips("", &locals).is_empty());
    }

    fn sample_branches() -> Vec<Branch> {
        parse_branches(
            &[
                "refs/heads/sparrow\u{1f}sparrow\u{1f}*\u{1f}ahead 8",
                "refs/heads/master\u{1f}master\u{1f} \u{1f}",
                "refs/heads/feature/login\u{1f}feature/login\u{1f} \u{1f}",
                "refs/remotes/origin/HEAD\u{1f}origin/HEAD\u{1f} \u{1f}",
                "refs/remotes/origin/master\u{1f}origin/master\u{1f} \u{1f}",
                "refs/remotes/origin/fix/a\u{1f}origin/fix/a\u{1f} \u{1f}",
                "refs/remotes/origin/zeta\u{1f}origin/zeta\u{1f} \u{1f}",
                "refs/tags/v0.0.2\u{1f}v0.0.2\u{1f} \u{1f}",
                "refs/tags/v0.0.1\u{1f}v0.0.1\u{1f} \u{1f}",
            ]
                .join("\n"),
        )
    }

    #[test]
    fn branch_parsing_tracks_kind_and_ahead() {
        let b = sample_branches();
        assert_eq!(b.len(), 8, "origin/HEAD is dropped");
        let cur = b.iter().find(|b| b.current).unwrap();
        assert_eq!((cur.name.as_str(), cur.ahead), ("sparrow", 8));
        assert_eq!(parse_track("ahead 5, behind 2"), (5, 2));
        assert_eq!(parse_track("gone"), (0, 0));
        assert!(b
            .iter()
            .any(|b| b.kind == BranchKind::Tag && b.name == "v0.0.1"));
    }

    fn names(rows: &[TreeRow]) -> Vec<String> {
        rows.iter()
            .map(|r| match r {
                TreeRow::Head => "HEAD".into(),
                TreeRow::Section { title, .. } => format!("§{title}"),
                TreeRow::Folder { name, depth, .. } => format!("{depth}/{name}/"),
                TreeRow::Ref { name, depth, .. } => format!("{depth}:{name}"),
            })
            .collect()
    }

    #[test]
    fn tree_pins_current_and_default_then_folders_then_leaves() {
        let rows = build_tree(&sample_branches(), "", &BTreeSet::new());
        assert_eq!(
            names(&rows),
            vec![
                "HEAD",
                "§Local",
                "0:sparrow",
                "0:master",
                "0/feature/",
                "1:login",
                "§Remote",
                "0/origin/",
                "1:master",
                "1/fix/",
                "2:a",
                "1:zeta",
                "§Tags",
                "0:v0.0.1",
                "0:v0.0.2",
            ]
        );
    }

    #[test]
    fn tree_collapse_and_search() {
        let mut closed = BTreeSet::new();
        closed.insert("remote".to_string());
        closed.insert("local/feature".to_string());
        let rows = build_tree(&sample_branches(), "", &closed);
        let n = names(&rows);
        assert!(n.contains(&"0/feature/".to_string()));
        assert!(!n.contains(&"1:login".to_string()), "folder collapsed");
        assert!(!n.contains(&"0/origin/".to_string()), "remote collapsed");
        // searching expands everything and hides empty sections
        let rows = build_tree(&sample_branches(), "LOGIN", &closed);
        assert_eq!(
            names(&rows),
            vec!["HEAD", "§Local", "0/feature/", "1:login"]
        );
    }
}