//! Local-only context building and Ollama commit-message generation.

use std::{
    path::{Component, Path, PathBuf},
    process::Command,
    sync::LazyLock,
    time::Duration,
};

use globset::{Glob, GlobSetBuilder};
use regex::Regex;
use serde::{Deserialize, Serialize};
use sparrow_rpc::source_control::FileDiff;

static SECRET_PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    vec![
        Regex::new(r"(?s)-----BEGIN [^-]+PRIVATE KEY-----.*?-----END [^-]+PRIVATE KEY-----")
            .unwrap(),
        Regex::new(r"(?i)\b(?:gh[pousr]_[A-Za-z0-9_]{20,}|github_pat_[A-Za-z0-9_]{20,}|sk-[A-Za-z0-9_-]{20,}|AKIA[0-9A-Z]{16})\b")
            .unwrap(),
        Regex::new(r"(?i)(authorization\s*[:=]\s*(?:bearer|basic)\s+)[A-Za-z0-9._~+/-]{12,}={0,2}")
            .unwrap(),
        Regex::new(r#"(?i)((?:api[_-]?key|access[_-]?token|token|password|passwd|secret)\s*[:=]\s*["']?)[^\s"'`,;]{8,}"#)
            .unwrap(),
    ]
});

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PromptContext {
    pub paths: Vec<String>,
    pub diff: String,
    pub truncated: bool,
    pub redacted_values: usize,
}

#[derive(Deserialize)]
struct ModelList {
    models: Vec<ModelEntry>,
}

#[derive(Deserialize)]
struct ModelEntry {
    name: String,
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    stream: bool,
    temperature: f32,
    max_tokens: u32,
    messages: [ChatMessage<'a>; 2],
}

#[derive(Serialize)]
struct ChatMessage<'a> {
    role: &'a str,
    content: String,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: ResponseMessage,
}

#[derive(Deserialize)]
struct ResponseMessage {
    content: String,
}

fn local_base(endpoint: &str) -> Result<url::Url, String> {
    let url = url::Url::parse(endpoint.trim())
        .map_err(|_| "Enter a valid local Ollama URL.".to_string())?;
    let host = url.host_str().unwrap_or_default();
    if url.scheme() != "http"
        || !matches!(host, "localhost" | "127.0.0.1" | "::1")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(
            "AI commit messages only support a local loopback Ollama endpoint."
                .into(),
        );
    }
    Ok(url)
}

fn excluded(path: &Path, patterns: &str) -> Result<bool, String> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        builder.add(
            Glob::new(pattern)
                .map_err(|_| format!("Invalid AI exclusion glob: {pattern}"))?,
        );
    }
    let set = builder
        .build()
        .map_err(|_| "Unable to build AI exclusion patterns.".to_string())?;
    Ok(set.is_match(path))
}

fn relative_path(
    root: &Path,
    original_root: &Path,
    path: &Path,
) -> Result<PathBuf, String> {
    let relative = if path.is_absolute() {
        path.strip_prefix(original_root)
            .or_else(|_| path.strip_prefix(root))
            .map_err(|_| "A selected change is outside the workspace.".to_string())?
    } else {
        path
    };
    if relative.as_os_str().is_empty()
        || relative.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err("A selected change has an unsafe workspace path.".into());
    }
    Ok(relative.to_path_buf())
}

fn safe_path_label(path: &Path) -> String {
    path.to_string_lossy()
        .chars()
        .map(|character| {
            if character.is_control() {
                '?'
            } else {
                character
            }
        })
        .collect()
}

fn added_file_diff(
    root: &Path,
    path: &Path,
    max_chars: usize,
) -> Result<(String, bool), String> {
    let full_path = root.join(path);
    let canonical_root = root
        .canonicalize()
        .map_err(|_| "Unable to resolve the project directory.".to_string())?;
    let canonical_path = full_path
        .canonicalize()
        .map_err(|_| "A selected new file is no longer available.".to_string())?;
    if !canonical_path.starts_with(canonical_root)
        || std::fs::symlink_metadata(&full_path)
            .map(|metadata| metadata.file_type().is_symlink())
            .unwrap_or(true)
    {
        return Err("A selected new file resolves outside the project.".into());
    }
    let metadata = std::fs::metadata(&canonical_path)
        .map_err(|_| "Unable to inspect a selected new file.".to_string())?;
    if !metadata.is_file() || metadata.len() > (max_chars as u64).saturating_mul(4) {
        return Ok((
            format!(
                "New file {} omitted (binary or over the size limit).",
                safe_path_label(path)
            ),
            true,
        ));
    }
    let bytes = std::fs::read(&canonical_path)
        .map_err(|_| "Unable to read a selected new file.".to_string())?;
    if bytes.contains(&0) {
        return Ok((
            format!("New file {} omitted (binary).", safe_path_label(path)),
            false,
        ));
    }
    let text = String::from_utf8_lossy(&bytes);
    let truncated = text.chars().count() > max_chars;
    let content = text.chars().take(max_chars).collect::<String>();
    Ok((
        format!("--- new file: {}\n+++\n{content}", safe_path_label(path)),
        truncated,
    ))
}

fn tracked_diff(
    root: &Path,
    paths: &[PathBuf],
    max_bytes: usize,
) -> Result<(String, bool), String> {
    use std::io::Read;
    let mut command = Command::new("git");
    command
        .args([
            "--literal-pathspecs",
            "diff",
            "--no-ext-diff",
            "--no-textconv",
            "--unified=2",
            "HEAD",
            "--",
        ])
        .args(paths)
        .current_dir(root)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|_| "Unable to build the selected Git diff.".to_string())?;
    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err("Unable to read the selected Git diff.".into());
    };
    let mut bytes = Vec::new();
    let read = stdout
        .take(max_bytes.saturating_add(1) as u64)
        .read_to_end(&mut bytes);
    if read.is_err() {
        let _ = child.kill();
        let _ = child.wait();
        return Err("Unable to read the selected Git diff.".into());
    }
    let truncated = bytes.len() > max_bytes;
    if truncated {
        let _ = child.kill();
    }
    let status = child.wait().map_err(|_| {
        "Unable to finish building the selected Git diff.".to_string()
    })?;
    if !status.success() && !truncated {
        return Err("Git could not build the selected diff.".into());
    }
    bytes.truncate(max_bytes);
    let diff = String::from_utf8_lossy(&bytes);
    if diff.contains("Binary files ") {
        Ok(("Binary change omitted from AI context.".into(), truncated))
    } else {
        let mut diff = diff.into_owned();
        if truncated {
            diff.push_str("\n[Git diff output truncated by Sparrow.]");
        }
        Ok((diff, truncated))
    }
}

pub fn redact_secrets(input: &str) -> (String, usize) {
    let mut text = input.to_string();
    let mut redacted = 0;
    for pattern in SECRET_PATTERNS.iter() {
        redacted += pattern.find_iter(&text).count();
        text = pattern
            .replace_all(&text, |captures: &regex::Captures<'_>| {
                captures.get(1).map_or_else(
                    || "[REDACTED]".to_string(),
                    |prefix| format!("{}[REDACTED]", prefix.as_str()),
                )
            })
            .into_owned();
    }
    (text, redacted)
}

pub fn build_context(
    root: &Path,
    diffs: &[FileDiff],
    max_diff_chars: usize,
    exclude_globs: &str,
) -> Result<PromptContext, String> {
    let original_root = root.to_path_buf();
    let root = root
        .canonicalize()
        .map_err(|_| "Unable to resolve the project directory.".to_string())?;
    let max_chars = max_diff_chars.clamp(1_000, 50_000);
    let mut paths = Vec::new();
    let mut chunks = Vec::new();
    let mut redacted_path_values = 0;
    let mut input_truncated = false;
    for diff in diffs {
        let selected_paths: Vec<&Path> = match diff {
            FileDiff::Modified(path)
            | FileDiff::Added(path)
            | FileDiff::Deleted(path) => {
                vec![path]
            }
            FileDiff::Renamed(new, old) => vec![new, old],
        };
        let mut relative = Vec::new();
        for path in selected_paths {
            let path = relative_path(&root, &original_root, path)?;
            if excluded(&path, exclude_globs)? {
                continue;
            }
            let (label, redacted) = redact_secrets(&safe_path_label(&path));
            redacted_path_values += redacted;
            if !paths.contains(&label) {
                paths.push(label);
            }
            relative.push(path);
        }
        if relative.is_empty() {
            continue;
        }
        let (chunk, was_truncated) = match diff {
            FileDiff::Added(path) => {
                let path = relative_path(&root, &original_root, path)?;
                if excluded(&path, exclude_globs)? {
                    continue;
                }
                added_file_diff(&root, &path, max_chars)?
            }
            FileDiff::Modified(_)
            | FileDiff::Deleted(_)
            | FileDiff::Renamed(_, _) => {
                tracked_diff(&root, &relative, max_chars.saturating_mul(4))?
            }
        };
        input_truncated |= was_truncated;
        if !chunk.trim().is_empty() {
            chunks.push(chunk);
        }
    }
    if chunks.is_empty() {
        return Err("No safe text changes are selected for AI context.".into());
    }
    let joined = chunks.join("\n\n");
    let (mut diff, redacted_values) = redact_secrets(&joined);
    const TRUNCATION_NOTE: &str = "\n[Diff context truncated by Sparrow.]";
    let diff_limit_exceeded = diff.chars().count() > max_chars;
    let truncated = input_truncated || diff_limit_exceeded;
    if truncated {
        let content_limit =
            max_chars.saturating_sub(TRUNCATION_NOTE.chars().count());
        diff = diff.chars().take(content_limit).collect();
        diff.push_str(TRUNCATION_NOTE);
    }
    Ok(PromptContext {
        paths,
        diff,
        truncated,
        redacted_values: redacted_values + redacted_path_values,
    })
}

pub fn list_models(endpoint: &str) -> Result<Vec<String>, String> {
    let base = local_base(endpoint)?;
    let url = base
        .join("api/tags")
        .map_err(|_| "Invalid local Ollama endpoint.".to_string())?;
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(8))
        .build()
        .map_err(|_| "Unable to prepare a local Ollama request.".to_string())?;
    let response = client.get(url).send().map_err(|_| {
        "Ollama is unavailable. Start it locally, then refresh models.".to_string()
    })?;
    if !response.status().is_success() {
        return Err("Ollama could not list installed models.".into());
    }
    let models: ModelList = response
        .json()
        .map_err(|_| "Ollama returned an unreadable model list.".to_string())?;
    Ok(models.models.into_iter().map(|model| model.name).collect())
}

pub fn generate_message(
    endpoint: &str,
    model: &str,
    style: &str,
    language: &str,
    context: &PromptContext,
) -> Result<String, String> {
    if model.trim().is_empty() {
        return Err("Choose an installed Ollama model in Settings → Version Control → AI Commit Messages.".into());
    }
    let base = local_base(endpoint)?;
    let url = base
        .join("v1/chat/completions")
        .map_err(|_| "Invalid local Ollama endpoint.".to_string())?;
    let [system, user] = prompt_messages(context, style, language);
    let request = ChatRequest {
        model,
        stream: false,
        temperature: 0.1,
        max_tokens: 80,
        messages: [
            ChatMessage {
                role: "system",
                content: system,
            },
            ChatMessage {
                role: "user",
                content: user,
            },
        ],
    };
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(90))
        .build()
        .map_err(|_| "Unable to prepare a local Ollama request.".to_string())?;
    let response = client.post(url).json(&request).send().map_err(|_| {
        "Ollama did not complete the request. Check the local service and model."
            .to_string()
    })?;
    if !response.status().is_success() {
        return Err(format!(
            "Ollama returned HTTP {} while generating a commit subject.",
            response.status().as_u16()
        ));
    }
    use std::io::Read;
    const MAX_RESPONSE_BYTES: usize = 256 * 1024;
    let mut response = response;
    let mut response_bytes = Vec::new();
    response
        .by_ref()
        .take((MAX_RESPONSE_BYTES + 1) as u64)
        .read_to_end(&mut response_bytes)
        .map_err(|_| "Ollama returned an unreadable completion.".to_string())?;
    if response_bytes.len() > MAX_RESPONSE_BYTES {
        return Err("Ollama's completion response exceeded the size limit.".into());
    }
    let response: ChatResponse = serde_json::from_slice(&response_bytes)
        .map_err(|_| "Ollama returned an unreadable completion.".to_string())?;
    let generated = response
        .choices
        .into_iter()
        .next()
        .map(|choice| choice.message.content)
        .ok_or_else(|| "Ollama returned no commit subject.".to_string())?;
    let (cleaned, _) = redact_secrets(&clean_subject(&generated));
    if cleaned.is_empty() {
        return Err("Ollama returned an empty commit subject.".into());
    }
    Ok(cleaned)
}

pub fn prompt_messages(
    context: &PromptContext,
    style: &str,
    language: &str,
) -> [String; 2] {
    [
        format!(
            "Write one concise commit subject in {language}, in {style} style. Use only the selected-file diff. Do not include markdown, quotes, a body, or claims not supported by the diff. Return only the subject line."
        ),
        format!(
            "Selected paths:\n{}\n\nRedacted diff:\n{}",
            context.paths.join("\n"),
            context.diff
        ),
    ]
}

pub fn clean_subject(generated: &str) -> String {
    let mut subject = generated
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with("```"))
        .unwrap_or_default()
        .trim_matches(['`', '"', '\''])
        .trim()
        .to_string();
    if let Some((prefix, rest)) = subject.split_once(':')
        && matches!(
            prefix.to_ascii_lowercase().as_str(),
            "commit message" | "subject"
        )
    {
        subject = rest.trim().to_string();
    }
    if subject.chars().count() > 72 {
        subject = subject.chars().take(72).collect::<String>();
        if let Some((before, _)) = subject.rsplit_once(' ') {
            subject = before.to_string();
        }
    }
    subject
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
    };

    #[test]
    fn selected_added_files_are_redacted_bounded_and_exclusions_are_respected() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("src/main.rs");
        std::fs::create_dir_all(source.parent().unwrap()).unwrap();
        std::fs::write(
            &source,
            "const API_TOKEN = \"ghp_abcdefghijklmnopqrstuvwxyz1234567890\";\n"
                .to_string()
                + &"// change\n".repeat(150),
        )
        .unwrap();
        let second = root.path().join("src/second.rs");
        std::fs::write(&second, "// another selected file\n".repeat(80)).unwrap();
        std::fs::write(root.path().join(".env.local"), "TOKEN=super-secret-value\n")
            .unwrap();
        let selected = vec![
            FileDiff::Added(source),
            FileDiff::Added(second),
            FileDiff::Added(root.path().join(".env.local")),
        ];
        let context =
            build_context(root.path(), &selected, 1_000, "**/.env*").unwrap();
        assert_eq!(context.paths, vec!["src/main.rs", "src/second.rs"]);
        assert!(context.diff.contains("[REDACTED]"), "{}", context.diff);
        assert!(
            !context
                .diff
                .contains("ghp_abcdefghijklmnopqrstuvwxyz1234567890")
        );
        assert!(!context.diff.contains("super-secret-value"));
        assert!(context.truncated);
        assert!(context.diff.chars().count() < 1_050);
    }

    #[test]
    fn local_endpoint_validation_rejects_remote_hosts() {
        assert!(local_base("http://localhost:11434").is_ok());
        assert!(local_base("http://127.0.0.1:11434").is_ok());
        assert!(local_base("https://api.openai.com").is_err());
        assert!(local_base("http://user:pass@127.0.0.1:11434").is_err());
    }

    #[test]
    fn completion_subject_is_cleaned_and_bounded() {
        assert_eq!(
            clean_subject("```text\nSubject: Add settings control\n```"),
            "Add settings control"
        );
        let long = clean_subject(&"word ".repeat(30));
        assert!(long.chars().count() <= 72);
        assert!(!long.ends_with(' '));
    }

    #[test]
    fn redaction_removes_keys_authorization_headers_and_private_keys() {
        let input = concat!(
            "TOKEN=super-secret-value\n",
            "Authorization: Bearer abcdefghijklmnopqrstuvwxyz012345\n",
            "-----BEGIN RSA PRIVATE KEY-----\nprivate-material\n-----END RSA PRIVATE KEY-----"
        );
        let (redacted, count) = redact_secrets(input);
        assert_eq!(count, 3);
        assert!(!redacted.contains("super-secret-value"));
        assert!(!redacted.contains("abcdefghijklmnopqrstuvwxyz012345"));
        assert!(!redacted.contains("private-material"));
        assert!(redacted.contains("[REDACTED]"));
    }

    #[test]
    fn tracked_diff_is_path_scoped_and_bounded() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("src")).unwrap();
        let file = root.path().join("src/tracked.rs");
        std::fs::write(&file, "fn old() {}\n").unwrap();
        let run_git = |args: &[&str]| {
            let output = Command::new("git")
                .args(args)
                .current_dir(root.path())
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        };
        run_git(&["init", "-q"]);
        run_git(&["config", "user.name", "Sparrow Test"]);
        run_git(&["config", "user.email", "sparrow-test@example.invalid"]);
        run_git(&["add", "src/tracked.rs"]);
        run_git(&["commit", "-q", "-m", "initial"]);
        std::fs::write(&file, format!("{}\n", "fn changed() {}".repeat(500)))
            .unwrap();

        let context =
            build_context(root.path(), &[FileDiff::Modified(file)], 1_000, "")
                .unwrap();
        assert!(context.truncated);
        assert!(context.diff.contains("Diff context truncated"));
        assert!(context.diff.chars().count() <= 1_000);

        assert!(
            build_context(
                root.path(),
                &[FileDiff::Modified(PathBuf::from("../outside.rs"))],
                1_000,
                "",
            )
            .is_err()
        );
    }

    #[test]
    fn ollama_model_discovery_and_completion_use_local_http_only() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            for (path, body) in [
                ("/api/tags", r#"{"models":[{"name":"qwen2.5-coder:7b"}]}"#),
                (
                    "/v1/chat/completions",
                    r#"{"choices":[{"message":{"content":"Add local model settings"}}]}"#,
                ),
            ] {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0; 8192];
                let size = stream.read(&mut request).unwrap();
                let request = String::from_utf8_lossy(&request[..size]);
                assert!(request.contains(path));
                if path == "/v1/chat/completions" {
                    assert!(request.contains("src/main.rs"));
                    assert!(request.contains("[REDACTED]"));
                    assert!(request.contains("\"max_tokens\":80"));
                }
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )
                .unwrap();
            }
        });
        let endpoint = format!("http://{address}");
        assert_eq!(list_models(&endpoint).unwrap(), vec!["qwen2.5-coder:7b"]);
        let context = PromptContext {
            paths: vec!["src/main.rs".into()],
            diff: "const TOKEN=[REDACTED]".into(),
            truncated: false,
            redacted_values: 1,
        };
        assert_eq!(
            generate_message(
                &endpoint,
                "qwen2.5-coder:7b",
                "imperative",
                "English",
                &context
            )
            .unwrap(),
            "Add local model settings"
        );
        server.join().unwrap();
    }
}
