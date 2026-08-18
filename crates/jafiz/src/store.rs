//! Everything that touches the filesystem: where suites live, which ones are
//! there, and the JSON that records their runs.
//!
//! Location resolution is a cascade (spec §3) so the same binary serves two
//! habits without a setting: a suite that belongs to a repo lives in the
//! repo's `tests/manual/` and is versioned with it — and Claude finds it
//! automatically because it is already running in that repo — while a suite
//! about a product with no local checkout lives in the central library.

use crate::model::{Cursor, Run, RunFile, StepResult, StepStatus};
use crate::parser::{parse_file, ParseOutcome};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// Directory names a project-local suite folder may use, in priority order.
const PROJECT_DIRS: [&str; 2] = ["tests/manual", ".jafiz"];
/// How far up the tree the project search walks before giving up.
const MAX_ASCENT: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocationKind {
    /// `--dir` on the CLI, or the folder chosen in the GUI.
    Explicit,
    /// The `JAFIZ_DIR` environment variable.
    Env,
    /// A `tests/manual` or `.jafiz` folder found by walking up from the cwd.
    Project,
    /// The central library under %LOCALAPPDATA%.
    Library,
}

impl LocationKind {
    pub fn label(&self) -> &'static str {
        match self {
            LocationKind::Explicit => "--dir",
            LocationKind::Env => "JAFIZ_DIR",
            LocationKind::Project => "projeto",
            LocationKind::Library => "biblioteca",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Location {
    pub dir: PathBuf,
    pub kind: LocationKind,
}

/// The central library: `%LOCALAPPDATA%\Software Imperial\Foundry32\jafiz\suites`.
/// Shares the hub's root so uninstalling Foundry32 takes everything with it.
pub fn library_dir() -> PathBuf {
    let local = std::env::var("LOCALAPPDATA")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| {
            let home = std::env::var("USERPROFILE").unwrap_or_else(|_| ".".into());
            format!("{home}\\AppData\\Local")
        });
    PathBuf::from(local)
        .join("Software Imperial")
        .join("Foundry32")
        .join("jafiz")
        .join("suites")
}

/// Walks up from `start` looking for a project-local suite folder.
pub fn find_project_dir(start: &Path) -> Option<PathBuf> {
    let mut current = Some(start);
    for _ in 0..MAX_ASCENT {
        let dir = current?;
        for candidate in PROJECT_DIRS {
            let path = dir.join(candidate.replace('/', "\\"));
            if path.is_dir() {
                return Some(path);
            }
        }
        current = dir.parent();
    }
    None
}

/// The cascade from spec §3. The library is created on demand so a first run
/// with no arguments always lands somewhere writable.
pub fn resolve_location(explicit: Option<&Path>) -> Location {
    if let Some(dir) = explicit {
        return Location { dir: dir.to_path_buf(), kind: LocationKind::Explicit };
    }
    if let Ok(dir) = std::env::var("JAFIZ_DIR") {
        if !dir.trim().is_empty() {
            return Location { dir: PathBuf::from(dir), kind: LocationKind::Env };
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        if let Some(dir) = find_project_dir(&cwd) {
            return Location { dir, kind: LocationKind::Project };
        }
    }
    let dir = library_dir();
    let _ = std::fs::create_dir_all(&dir);
    Location { dir, kind: LocationKind::Library }
}

/// Every `.md` in a location, sorted by file name. Non-recursive: a location
/// is a flat folder of suites, and `.runs` is the only subdirectory JAFIZ owns.
pub fn list_suites(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|e| e.eq_ignore_ascii_case("md")))
        .collect();
    paths.sort();
    paths
}

/// Finds a suite by file stem, case-insensitively; falls back to a substring
/// match on the stem so `jafiz report check` finds `checkout.md`.
pub fn find_suite(dir: &Path, name: &str) -> Option<PathBuf> {
    let wanted = name.trim().trim_end_matches(".md").to_lowercase();
    let suites = list_suites(dir);
    let stem_of = |p: &PathBuf| {
        p.file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default()
    };
    suites
        .iter()
        .find(|p| stem_of(p) == wanted)
        .or_else(|| suites.iter().find(|p| stem_of(p).contains(&wanted)))
        .cloned()
}

pub fn load_suite(path: &Path) -> std::io::Result<ParseOutcome> {
    parse_file(path)
}

/// `<dir>\.runs\<stem>.json` — progress lives beside the suite, never inside it.
pub fn runs_path(dir: &Path, stem: &str) -> PathBuf {
    dir.join(".runs").join(format!("{stem}.json"))
}

/// Reads a suite's run history. A missing or unreadable file yields an empty
/// history: progress is a convenience, and a corrupt one must never stop the
/// user from testing.
pub fn load_runs(dir: &Path, stem: &str) -> RunFile {
    let empty = RunFile { suite: stem.to_string(), ..RunFile::default() };
    let Ok(text) = std::fs::read_to_string(runs_path(dir, stem)) else { return empty };
    let Ok(value) = serde_json::from_str::<Value>(&text) else { return empty };
    let runs = value["runs"].as_array().map(|a| a.iter().map(run_from_json).collect());
    RunFile {
        suite: value["suite"].as_str().unwrap_or(stem).to_string(),
        active_run: value["active_run"].as_str().map(str::to_string),
        runs: runs.unwrap_or_default(),
    }
}

pub fn save_runs(dir: &Path, file: &RunFile) -> Result<(), String> {
    let path = runs_path(dir, &file.suite);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    }
    let body = serde_json::to_string_pretty(&json!({
        "schema": 1,
        "suite": file.suite,
        "active_run": file.active_run,
        "runs": file.runs.iter().map(run_to_json).collect::<Vec<_>>(),
    }))
    .map_err(|e| e.to_string())?;
    std::fs::write(&path, body).map_err(|e| format!("write {}: {e}", path.display()))
}

fn run_to_json(run: &Run) -> Value {
    json!({
        "id": run.id,
        "started_at": run.started_at,
        "finished_at": run.finished_at,
        "environment": run.environment,
        "current": run.current.as_ref().map(|c| json!({ "scenario": c.scenario, "step": c.step })),
        "results": run.results.iter().map(|r| json!({
            "scenario": r.scenario,
            "step": r.step,
            "status": r.status.code(),
            "note": r.note,
            "evidence": r.evidence,
            "at": r.at,
            "action": r.action,
        })).collect::<Vec<_>>(),
    })
}

fn run_from_json(value: &Value) -> Run {
    Run {
        id: value["id"].as_str().unwrap_or_default().to_string(),
        started_at: value["started_at"].as_str().unwrap_or_default().to_string(),
        finished_at: value["finished_at"].as_str().map(str::to_string),
        environment: value["environment"].as_str().unwrap_or_default().to_string(),
        current: value["current"].as_object().map(|c| Cursor {
            scenario: c.get("scenario").and_then(Value::as_str).unwrap_or_default().to_string(),
            step: c.get("step").and_then(Value::as_u64).unwrap_or(1) as usize,
        }),
        results: value["results"]
            .as_array()
            .map(|a| a.iter().map(result_from_json).collect())
            .unwrap_or_default(),
    }
}

fn result_from_json(value: &Value) -> StepResult {
    StepResult {
        scenario: value["scenario"].as_str().unwrap_or_default().to_string(),
        step: value["step"].as_u64().unwrap_or(1) as usize,
        status: value["status"]
            .as_str()
            .and_then(StepStatus::from_code)
            .unwrap_or(StepStatus::Pending),
        note: value["note"].as_str().unwrap_or_default().to_string(),
        evidence: value["evidence"]
            .as_array()
            .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
            .unwrap_or_default(),
        at: value["at"].as_str().unwrap_or_default().to_string(),
        action: value["action"].as_str().unwrap_or_default().to_string(),
    }
}
