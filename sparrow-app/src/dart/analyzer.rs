//! Runs `dart analyze --format=json` on a debounce and emits diagnostics.
//!
//! The worker thread:
//!   1. waits for a request, drains any newer ones within `DEBOUNCE`,
//!   2. spawns `dart analyze --format=json <path>` to completion,
//!   3. parses the JSON, converts to LSP diagnostics, emits one notification
//!      per source file.
//!
//! Staleness is handled with a monotonic `epoch` per request: the UI keeps
//! the highest epoch it has applied per file and discards anything older.
//! That is correct even if two analyses of the same file are ever in flight.

use std::{
    collections::HashMap,
    io::{Read, Seek},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    thread,
    time::Duration,
};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, unbounded};
use lsp_types::{Diagnostic, DiagnosticSeverity, NumberOrString, Position, Range};
use serde::Deserialize;

const DEBOUNCE: Duration = Duration::from_millis(500);

// ── Public API ──────────────────────────────────────────────────────

/// Result of one `dart analyze` run, fanned out per source file.
#[derive(Debug, Clone)]
pub enum DartNotification {
    Diagnostics {
        path: PathBuf,
        epoch: u64,
        diagnostics: Vec<Diagnostic>,
    },
    /// The analyzer failed to run (dart not installed, bad project, etc.).
    Error(String),
}

/// Handle to the background worker. Cheap to clone.
#[derive(Clone)]
pub struct DartAnalyzerService {
    tx: Sender<AnalyzeRequest>,
    pub notifications: Receiver<DartNotification>,
    available: std::sync::Arc<AtomicBool>,
    next_epoch: std::sync::Arc<AtomicU64>,
}

impl DartAnalyzerService {
    pub fn new() -> Self {
        let (tx, rx) = unbounded();
        let (notif_tx, notif_rx) = unbounded();

        let available = std::sync::Arc::new(AtomicBool::new(check_dart_available()));
        if !available.load(Ordering::Relaxed) {
            tracing::warn!(
                "`dart` not found on PATH; Dart diagnostics are disabled. \
                 Install the Dart SDK or add it to PATH to enable them."
            );
        }

        let worker = Worker { rx, notif_tx };
        thread::Builder::new()
            .name("DartAnalyzer".into())
            .spawn(move || worker.run())
            .expect("spawn DartAnalyzer worker");

        Self {
            tx,
            notifications: notif_rx,
            available,
            next_epoch: std::sync::Arc::new(AtomicU64::new(1)),
        }
    }

    /// Queue an analysis of `path`. Returns the epoch assigned to this request.
    /// No-op (returns current epoch) if `dart` is not available.
    pub fn request(&self, path: PathBuf) -> u64 {
        let epoch = self.next_epoch.fetch_add(1, Ordering::Relaxed);
        if !self.available.load(Ordering::Relaxed) {
            return epoch;
        }
        if let Err(err) = self.tx.send(AnalyzeRequest { path, epoch }) {
            tracing::error!("failed to queue Dart analysis: {err}");
        }
        epoch
    }

    pub fn is_available(&self) -> bool {
        self.available.load(Ordering::Relaxed)
    }
}

// ── Worker ──────────────────────────────────────────────────────────

#[derive(Debug)]
struct AnalyzeRequest {
    path: PathBuf,
    epoch: u64,
}

struct Worker {
    rx: Receiver<AnalyzeRequest>,
    notif_tx: Sender<DartNotification>,
}

impl Worker {
    fn run(self) {
        loop {
            // Wait for the first request.
            let req = match self.rx.recv() {
                Ok(r) => r,
                Err(_) => return, // channel closed
            };
            let mut pending = req;

            // Debounce: keep replacing `pending` until DEBOUNCE passes
            // without a new request.
            loop {
                match self.rx.recv_timeout(DEBOUNCE) {
                    Ok(r) => pending = r,
                    Err(RecvTimeoutError::Timeout) => break,
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }

            self.analyze(pending);
        }
    }

    fn analyze(&self, req: AnalyzeRequest) {
        match run_dart_analyze(&req.path, req.epoch) {
            Ok(by_file) => {
                for (path, diagnostics) in by_file {
                    if let Err(err) =
                        self.notif_tx.send(DartNotification::Diagnostics {
                            path,
                            epoch: req.epoch,
                            diagnostics,
                        })
                    {
                        tracing::error!("dart notif channel closed: {err}");
                        return;
                    }
                }
            }
            Err(err) => {
                let _ = self.notif_tx.send(DartNotification::Error(err));
            }
        }
    }
}

// ── dart executable plumbing ────────────────────────────────────────

fn check_dart_available() -> bool {
    crate::dart::sdk::is_dart_available(None)
}

fn run_dart_analyze(
    path: &Path,
    _epoch: u64,
) -> Result<HashMap<PathBuf, Vec<Diagnostic>>, String> {
    inspect_workspace(path, &AtomicBool::new(false))?
        .ok_or_else(|| "Analysis cancelled.".into())
}

/// Return one complete snapshot of saved-file diagnostics. Temporary output
/// files keep the subprocess from blocking on a full pipe while it is polled.
pub(crate) fn inspect_workspace(
    path: &Path,
    cancel: &AtomicBool,
) -> Result<Option<HashMap<PathBuf, Vec<Diagnostic>>>, String> {
    if cancel.load(Ordering::Relaxed) {
        return Ok(None);
    }
    let root = if path.is_dir() {
        path
    } else {
        path.parent().unwrap_or(path)
    };
    let dart = crate::dart::sdk::find_dart_binary(Some(root)).ok_or_else(|| {
        "Configure a Dart or Flutter SDK to inspect this project.".to_owned()
    })?;
    let mut stdout = tempfile::tempfile().map_err(|e| e.to_string())?;
    let mut stderr = tempfile::tempfile().map_err(|e| e.to_string())?;
    let mut child = Command::new(dart)
        .current_dir(root)
        .env("CI", "true")
        .arg("analyze")
        .arg("--format=json")
        .arg(path)
        .stdin(Stdio::null())
        .stdout(stdout.try_clone().map_err(|e| e.to_string())?)
        .stderr(stderr.try_clone().map_err(|e| e.to_string())?)
        .spawn()
        .map_err(|e| format!("Could not start Dart analysis: {e}"))?;
    let status = loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(None);
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => thread::sleep(Duration::from_millis(50)),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("Could not wait for Dart analysis: {error}"));
            }
        }
    };
    let mut json_bytes = Vec::new();
    stdout
        .rewind()
        .and_then(|_| stdout.read_to_end(&mut json_bytes))
        .map_err(|error| format!("Could not read Dart analysis results: {error}"))?;
    // Dart uses a nonzero exit status when it finds errors. A valid JSON
    // report is still a completed inspection.
    let json: AnalyzeJson =
        serde_json::from_slice(&json_bytes).map_err(|error| {
            let mut details = String::new();
            let _ = stderr
                .rewind()
                .and_then(|_| stderr.read_to_string(&mut details));
            format!(
                "Dart analysis did not return a report ({status}): {}",
                if details.trim().is_empty() {
                    error.to_string()
                } else {
                    details.trim().to_owned()
                }
            )
        })?;

    let mut by_file: HashMap<PathBuf, Vec<Diagnostic>> = HashMap::new();
    for raw in json.diagnostics {
        let file = if raw.location.file.is_absolute() {
            raw.location.file.clone()
        } else {
            root.join(&raw.location.file)
        };
        by_file
            .entry(file)
            .or_default()
            .push(convert_diagnostic(raw));
    }
    for diags in by_file.values_mut() {
        diags.sort_by_key(|d| (d.range.start.line, d.range.start.character));
    }
    Ok(Some(by_file))
}

fn convert_diagnostic(raw: DartDiagnostic) -> Diagnostic {
    let severity = match raw.severity.as_str() {
        "ERROR" => DiagnosticSeverity::ERROR,
        "WARNING" => DiagnosticSeverity::WARNING,
        "INFO" => DiagnosticSeverity::INFORMATION,
        _ => DiagnosticSeverity::HINT,
    };
    // Dart analyzer positions are 1-based; LSP is 0-based.
    let pos = |p: DartPosition| {
        Position::new(p.line.saturating_sub(1), p.column.saturating_sub(1))
    };
    Diagnostic {
        range: Range {
            start: pos(raw.location.range.start),
            end: pos(raw.location.range.end),
        },
        severity: Some(severity),
        code: Some(NumberOrString::String(raw.code)),
        code_description: raw
            .documentation
            .and_then(|url| url::Url::parse(&url).ok())
            .filter(|url| matches!(url.scheme(), "https" | "http"))
            .map(|href| lsp_types::CodeDescription { href }),
        source: Some("dart".into()),
        message: match raw.correction_message {
            Some(hint) if !hint.is_empty() => {
                format!("{}\n{hint}", raw.problem_message)
            }
            _ => raw.problem_message,
        },
        related_information: None,
        tags: None,
        data: None,
    }
}

// ── JSON shapes ─────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct AnalyzeJson {
    #[allow(dead_code)]
    version: u32,
    diagnostics: Vec<DartDiagnostic>,
}

#[derive(Debug, Deserialize)]
struct DartDiagnostic {
    code: String,
    severity: String,
    #[serde(rename = "type")]
    #[allow(dead_code)]
    diagnostic_type: String,
    location: DiagnosticLocation,
    #[serde(rename = "problemMessage")]
    problem_message: String,
    #[serde(rename = "correctionMessage")]
    correction_message: Option<String>,
    documentation: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DiagnosticLocation {
    file: PathBuf,
    range: DartRange,
}

#[derive(Debug, Deserialize)]
struct DartRange {
    start: DartPosition,
    end: DartPosition,
}

#[derive(Debug, Deserialize)]
struct DartPosition {
    line: u32,
    column: u32,
    #[allow(dead_code)]
    offset: u32,
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn diag(file: &str, line: u32, col: u32, sev: &str, code: &str) -> Diagnostic {
        let raw = DartDiagnostic {
            code: code.into(),
            severity: sev.into(),
            diagnostic_type: "LINT".into(),
            location: DiagnosticLocation {
                file: PathBuf::from(file),
                range: DartRange {
                    start: DartPosition {
                        line,
                        column: col,
                        offset: 0,
                    },
                    end: DartPosition {
                        line,
                        column: col + 1,
                        offset: 0,
                    },
                },
            },
            problem_message: "msg".into(),
            correction_message: None,
            documentation: None,
        };
        convert_diagnostic(raw)
    }

    #[test]
    fn converts_severity() {
        assert_eq!(
            diag("a.dart", 1, 1, "ERROR", "x").severity,
            Some(DiagnosticSeverity::ERROR)
        );
        assert_eq!(
            diag("a.dart", 1, 1, "WARNING", "x").severity,
            Some(DiagnosticSeverity::WARNING)
        );
        assert_eq!(
            diag("a.dart", 1, 1, "INFO", "x").severity,
            Some(DiagnosticSeverity::INFORMATION)
        );
        assert_eq!(
            diag("a.dart", 1, 1, "WHATEVER", "x").severity,
            Some(DiagnosticSeverity::HINT)
        );
    }

    #[test]
    fn shifts_positions_to_zero_based() {
        let d = diag("a.dart", 3, 7, "ERROR", "x");
        assert_eq!(d.range.start, Position::new(2, 6));
        assert_eq!(d.range.end, Position::new(2, 7));
    }

    #[test]
    fn carries_code_as_number_or_string() {
        let d = diag("a.dart", 1, 1, "ERROR", "unused_import");
        match d.code {
            Some(NumberOrString::String(s)) => assert_eq!(s, "unused_import"),
            other => panic!("unexpected code: {other:?}"),
        }
    }

    #[test]
    fn available_flag_does_not_panic() {
        // Can't assert true/false (depends on the CI machine) — just verify
        // construction works.
        let svc = DartAnalyzerService::new();
        let _ = svc.is_available();
    }

    #[test]
    #[ignore = "requires a local Dart SDK"]
    fn analyzes_saved_workspace_and_reports_errors_from_nonzero_exit() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("main.dart");
        std::fs::write(
            root.path().join("pubspec.yaml"),
            "name: inspection_test\nenvironment:\n  sdk: '>=3.0.0 <4.0.0'\n",
        )
        .unwrap();
        std::fs::write(&path, "void main() { print(missing); }\n").unwrap();
        let cancel = AtomicBool::new(false);
        let findings = inspect_workspace(root.path(), &cancel).unwrap().unwrap();
        let errors = &findings[&path.canonicalize().unwrap()];
        assert_eq!(errors[0].severity, Some(DiagnosticSeverity::ERROR));
        assert_eq!(
            errors[0].code,
            Some(NumberOrString::String("undefined_identifier".into()))
        );
        assert!(errors[0].code_description.is_some());

        std::fs::write(&path, "void main() {}\n").unwrap();
        assert!(
            inspect_workspace(root.path(), &cancel)
                .unwrap()
                .unwrap()
                .is_empty()
        );
        cancel.store(true, Ordering::Relaxed);
        assert!(inspect_workspace(root.path(), &cancel).unwrap().is_none());
    }
}
