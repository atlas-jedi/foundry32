//! The only module that mutates a run. Every entry point takes the whole
//! `RunFile` and edits its active run, so a caller can never record into a
//! finished one by mistake, and the GUI's "save after every change" is a
//! single `store::save_runs` away.

use crate::clock::{run_id, timestamp};
use crate::model::{Cursor, Run, RunFile, StepResult, StepStatus, Suite};

/// Opens a new run and makes it active. Any previously active run is closed
/// first: two open runs over one suite would make "what is being tested right
/// now" ambiguous, which is the question this tool exists to answer.
pub fn start_run(file: &mut RunFile, suite: &Suite, environment: &str) -> String {
    finish_run(file);
    let id = unique_id(file);
    let current = suite.scenarios.first().map(|s| Cursor {
        scenario: s.id.clone(),
        step: s.steps.first().map_or(1, |step| step.number),
    });
    file.runs.push(Run {
        id: id.clone(),
        started_at: timestamp(),
        finished_at: None,
        environment: environment.trim().to_string(),
        current,
        results: Vec::new(),
    });
    file.suite = suite.stem.clone();
    file.active_run = Some(id.clone());
    id
}

/// Two runs started in the same second would collide on the timestamp id, so
/// a suffix is appended until the id is free.
fn unique_id(file: &RunFile) -> String {
    let base = run_id();
    if file.run(&base).is_none() {
        return base;
    }
    (2..)
        .map(|n| format!("{base}-{n}"))
        .find(|candidate| file.run(candidate).is_none())
        .unwrap_or(base)
}

/// Closes the active run. Returns false when there was none open.
pub fn finish_run(file: &mut RunFile) -> bool {
    let Some(run) = file.active_mut() else { return false };
    if run.is_open() {
        run.finished_at = Some(timestamp());
    }
    file.active_run = None;
    true
}

/// Records a verdict on one step of the active run, stamping the step's
/// current action text so a later edit to the suite can be detected.
pub fn mark(
    file: &mut RunFile,
    suite: &Suite,
    scenario: &str,
    step: usize,
    status: StepStatus,
) -> bool {
    let action = suite
        .scenario(scenario)
        .and_then(|s| s.step(step))
        .map(|s| s.action.clone())
        .unwrap_or_default();
    let Some(run) = file.active_mut() else { return false };
    let previous = run.result(scenario, step);
    let (note, evidence) = previous.map_or((String::new(), Vec::new()), |r| {
        (r.note.clone(), r.evidence.clone())
    });
    run.set_result(StepResult {
        scenario: scenario.to_string(),
        step,
        status,
        note,
        evidence,
        at: timestamp(),
        action,
    });
    true
}

/// Attaches (or replaces) the tester's note on a step. Creating a bare result
/// when there is none lets a note be written before the verdict — the usual
/// order when something looks off mid-step.
pub fn set_note(file: &mut RunFile, scenario: &str, step: usize, note: &str) -> bool {
    let Some(run) = file.active_mut() else { return false };
    match run.results.iter_mut().find(|r| r.scenario == scenario && r.step == step) {
        Some(result) => result.note = note.to_string(),
        None => run.set_result(StepResult {
            scenario: scenario.to_string(),
            step,
            status: StepStatus::Pending,
            note: note.to_string(),
            evidence: Vec::new(),
            at: timestamp(),
            action: String::new(),
        }),
    }
    true
}

/// Appends an evidence path to a step, ignoring an exact duplicate.
pub fn add_evidence(file: &mut RunFile, scenario: &str, step: usize, path: &str) -> bool {
    let Some(run) = file.active_mut() else { return false };
    match run.results.iter_mut().find(|r| r.scenario == scenario && r.step == step) {
        Some(result) => {
            if !result.evidence.iter().any(|e| e == path) {
                result.evidence.push(path.to_string());
            }
        }
        None => run.set_result(StepResult {
            scenario: scenario.to_string(),
            step,
            status: StepStatus::Pending,
            note: String::new(),
            evidence: vec![path.to_string()],
            at: timestamp(),
            action: String::new(),
        }),
    }
    true
}

/// Points "what is being tested right now" at a specific step.
pub fn set_current(file: &mut RunFile, scenario: &str, step: usize) -> bool {
    let Some(run) = file.active_mut() else { return false };
    run.current = Some(Cursor { scenario: scenario.to_string(), step });
    true
}

/// The first step with no verdict, scanning suite order from just after
/// `after` and wrapping to the start — so marking the last step of a scenario
/// lands on whatever is still open rather than dead-ending.
pub fn next_pending(suite: &Suite, run: &Run, after: Option<&Cursor>) -> Option<Cursor> {
    let ordered: Vec<Cursor> = suite
        .scenarios
        .iter()
        .flat_map(|s| {
            s.steps.iter().map(|step| Cursor { scenario: s.id.clone(), step: step.number })
        })
        .collect();
    let start = after
        .and_then(|c| {
            ordered.iter().position(|x| x.scenario == c.scenario && x.step == c.step).map(|i| i + 1)
        })
        .unwrap_or(0);
    ordered
        .iter()
        .cycle()
        .skip(start)
        .take(ordered.len())
        .find(|c| !run.status_of(&c.scenario, c.step).is_decided())
        .cloned()
}

/// Moves the current marker to the next step still awaiting a verdict, and
/// returns where it landed (`None` when the run is complete).
pub fn advance(file: &mut RunFile, suite: &Suite) -> Option<Cursor> {
    let run = file.active()?;
    let next = next_pending(suite, run, run.current.as_ref());
    let run = file.active_mut()?;
    run.current = next.clone();
    next
}
