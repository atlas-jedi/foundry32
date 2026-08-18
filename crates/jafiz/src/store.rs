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

/// Which rung of the cascade (spec §3) a location was resolved from.
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
    /// Short label for where a location came from, so a status line or the
    /// GUI's location picker can tell an explicit `--dir` apart from a folder
    /// found by the project search or from the shared library.
    pub fn label(&self) -> &'static str {
        match self {
            LocationKind::Explicit => "--dir",
            LocationKind::Env => "JAFIZ_DIR",
            LocationKind::Project => "projeto",
            LocationKind::Library => "biblioteca",
        }
    }
}

/// A resolved suite folder, together with which rung of the cascade produced
/// it — so callers and error messages can say where a suite came from, not
/// just where it is.
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

/// Every suite whose file stem matches `name`, case-insensitively. An exact
/// stem match wins outright and returns only itself; otherwise every substring
/// match comes back, so a caller can refuse an ambiguous name instead of
/// silently picking one. An empty query matches nothing — `contains("")` is
/// true for every string, which would otherwise return the whole directory.
pub fn match_suites(dir: &Path, name: &str) -> Vec<PathBuf> {
    let wanted = name.trim().trim_end_matches(".md").to_lowercase();
    if wanted.is_empty() {
        return Vec::new();
    }
    let suites = list_suites(dir);
    let stem_of = |path: &Path| {
        path.file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default()
    };
    if let Some(exact) = suites.iter().find(|path| stem_of(path) == wanted) {
        return vec![exact.clone()];
    }
    suites.into_iter().filter(|path| stem_of(path).contains(&wanted)).collect()
}

/// The single suite `name` resolves to, or `None` when it matches none or
/// several. Callers that want to explain an ambiguity use `match_suites`.
pub fn find_suite(dir: &Path, name: &str) -> Option<PathBuf> {
    let mut matches = match_suites(dir, name);
    if matches.len() == 1 {
        matches.pop()
    } else {
        None
    }
}

/// Reads and parses a suite from disk — the store's side of the module split
/// documented in `lib.rs`: every filesystem touch goes through `store`, even
/// one that immediately hands off to `parser`.
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
        // The filename is the identity, not the JSON's own `suite` field:
        // a run file copied alongside its suite still names the original, and
        // trusting it would make the next verdict overwrite that suite's
        // history instead of this one's.
        suite: stem.to_string(),
        active_run: value["active_run"].as_str().map(str::to_string),
        runs: runs.unwrap_or_default(),
    }
}

/// Writes a suite's run history as pretty-printed JSON — readable when
/// someone opens it directly, and diffable once it lands in git.
///
/// The destination comes from `file.suite`, which is only ever the stem the
/// history was loaded (or started) under — never the `suite` field read back
/// out of the JSON, which a copied file gets wrong. The field is still
/// written, because it tells a human which suite an opened file belongs to.
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
