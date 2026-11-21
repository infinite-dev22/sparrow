//! Branches, commit graph, changed files and details backed by local Git.
use super::reference_tools::output;
use std::path::{Path, PathBuf};
mod view;

pub use view::panel;

#[derive(Clone, Debug, PartialEq)]
struct Commit {
    hash: String,
    parents: Vec<String>,
    subject: String,
    author: String,
    date: String,
    refs: String,
    graph: String,
}
#[derive(Clone, Debug, PartialEq)]
struct Branch {
    name: String,
    reference: String,
    remote: bool,
    current: bool,
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
fn commits(text: &str) -> Vec<Commit> {
    text.lines()
        .filter_map(|line| {
            let parts = line.split('\x1f').collect::<Vec<_>>();
            if parts.len() != 6 {
                return None;
            }
            Some(Commit {
                hash: parts[0].into(),
                parents: parts[1].split_whitespace().map(str::to_string).collect(),
                subject: parts[2].into(),
                author: parts[3].into(),
                date: parts[4].into(),
                refs: parts[5].into(),
                graph: String::new(),
            })
        })
        .collect()
}
fn graph(commits: &mut [Commit]) {
    let mut lanes = Vec::<String>::new();
    for commit in commits {
        let old = lanes.iter().position(|hash| *hash == commit.hash);
        let index = old.unwrap_or_else(|| {
            lanes.push(commit.hash.clone());
            lanes.len() - 1
        });
        let before = lanes.clone();
        lanes.remove(index);
        for (offset, parent) in commit.parents.iter().enumerate() {
            if !lanes.contains(parent) {
                lanes.insert((index + offset).min(lanes.len()), parent.clone());
            }
        }
        let x = |lane: usize| 9 + lane * 16;
        let colors=["#8C78E8","#42B5A4","#E6AB5D","#5E9FE6","#D97CB0","#A3BD66"];
        let color=|lane:usize|colors[lane%colors.len()];
        let mut paths = String::new();
        for (lane, hash) in before.iter().enumerate() {
            if hash == &commit.hash {
                continue;
            }
            if let Some(next) = lanes.iter().position(|item| item == hash) {
                paths.push_str(&format!(
                    "<path stroke='{}' d='M{} 0 C{} 12 {} 12 {} 24'/>",
                    color(lane),x(lane),x(lane),x(next),x(next)
                ));
            }
        }
        if old.is_some() {
            paths.push_str(&format!("<path stroke='{}' d='M{} 0 V12'/>",color(index), x(index)));
        }
        for parent in &commit.parents {
            if let Some(next) = lanes.iter().position(|item| item == parent) {
                paths.push_str(&format!(
                    "<path stroke='{}' d='M{} 12 C{} 20 {} 18 {} 24'/>",
                    color(next),x(index),x(index),x(next),x(next)
                ));
            }
        }
        let width=(before.len().max(lanes.len()).max(1))*16+6;
        commit.graph = format!(
            "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 {width} 24'><g fill='none' stroke-width='2' stroke-linecap='round'>{paths}</g><circle cx='{}' cy='12' r='{}' fill='{}' stroke='{}' stroke-width='1.5'/></svg>",
            x(index),if commit.parents.len()>1{4.0}else{3.0},if commit.parents.len()>1{"#1E1F22"}else{color(index)},color(index)
        );
    }
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
fn load(
    root: &Path,
    branch: &str,
    user: &str,
    since: &str,
    path: &str,
    query: &str,
    limit: usize,
) -> Log {
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
    let mut args = vec![
        "log".to_string(),
        format!("-{}",limit),
        "--topo-order".into(),
        "--date=format:%d/%m/%Y, %H:%M".into(),
        "--format=%H%x1f%P%x1f%s%x1f%an%x1f%ad%x1f%D".into(),
    ];
    let revision =
        if query.len() >= 4 && query.chars().all(|c| c.is_ascii_hexdigit()) {
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
        args.push("--regexp-ignore-case".into());
        args.push("--fixed-strings".into());
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
        args.push(branch.into());
    }
    args.push("--".into());
    if !path.is_empty() {
        args.push(path.into());
    }
    let result = output(
        Path::new("git"),
        &args.iter().map(String::as_str).collect::<Vec<_>>(),
        Some(&real_root),
    );
    let branches = output(
        Path::new("git"),
        &[
            "for-each-ref",
            "--format=%(refname)%1f%(refname:short)%1f%(HEAD)",
            "refs/heads",
            "refs/remotes",
            "refs/tags",
        ],
        Some(&real_root),
    )
    .unwrap_or_default()
    .lines()
    .filter_map(|line| {
        let parts = line.split('\x1f').collect::<Vec<_>>();
        (parts.len() == 3).then(|| Branch {
            reference: parts[0].into(),
            name: parts[1].into(),
            remote: parts[0].starts_with("refs/remotes/"),
            current: parts[2] == "*",
        })
    })
    .collect();
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
    #[test]
    fn parses_merge_parents_and_null_delimited_paths() {
        let mut rows = commits(
            "a\u{1f}b c\u{1f}Merge\u{1f}Dev\u{1f}Today\u{1f}HEAD\nb\u{1f}d\u{1f}One\u{1f}Dev\u{1f}Today\u{1f}\nc\u{1f}d\u{1f}Two\u{1f}Dev\u{1f}Today\u{1f}",
        );
        graph(&mut rows);
        assert_eq!(rows[0].parents.len(), 2);
        assert!(rows.iter().all(|row| row.graph.contains("<circle")));
        let files = files("M\0lib/a file.dart\0D\0lib/old.dart\0");
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].path, PathBuf::from("lib/a file.dart"));
    }
}
