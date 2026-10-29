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
    path::PathBuf,
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
    Command::new("dart")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn run_dart_analyze(
    path: &PathBuf,
    _epoch: u64,
) -> Result<HashMap<PathBuf, Vec<Diagnostic>>, String> {
    let output = Command::new("dart")
        .arg("analyze")
        .arg("--format=json")
        .arg(path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("failed to spawn `dart analyze`: {e}"))?;

    // `dart analyze` exit code semantics:
    //   0 = no issues   1 = issues found   other = real failure
    // Both 0 and 1 produce valid JSON on stdout. Anything else is an error.
    let code = output.status.code().unwrap_or(-1);
    if code != 0 && code != 1 {
        return Err(format!(
            "`dart analyze` failed (exit {code}): {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    let json: AnalyzeJson = serde_json::from_slice(&output.stdout).map_err(|e| {
        format!(
            "failed to parse `dart analyze` JSON: {e}\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout).trim(),
            String::from_utf8_lossy(&output.stderr).trim(),
        )
    })?;

    let mut by_file: HashMap<PathBuf, Vec<Diagnostic>> = HashMap::new();
    for raw in json.diagnostics {
        let file = raw.location.file.clone();
        by_file
            .entry(file)
            .or_default()
            .push(convert_diagnostic(raw));
    }
    for diags in by_file.values_mut() {
        diags.sort_by_key(|d| (d.range.start.line, d.range.start.character));
    }
    Ok(by_file)
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
        code_description: None,
        source: Some("dart".into()),
        message: raw.problem_message,
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
    #[allow(dead_code)]
    correction_message: Option<String>,
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
}
