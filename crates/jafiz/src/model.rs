//! The domain model: a suite of manual test scenarios as parsed from Markdown,
//! a run recording one pass over it, and the pure derivations both front-ends
//! need (a scenario's rolled-up status, per-run tallies, staleness).
//!
//! Nothing here touches the filesystem or Win32 — it is the shared vocabulary
//! `parser`, `store`, `runs`, `report` and the GUI all speak.

use std::path::PathBuf;

/// The verdict on a single step of a single run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepStatus {
    /// Not executed yet.
    Pending,
    /// Behaved as the expected result describes.
    Pass,
    /// Did not behave as expected.
    Fail,
    /// Could not be executed — a dependency failed, or the environment is
    /// missing something. Distinct from `Fail`: nothing was proven about it.
    Blocked,
    /// Deliberately not tested this run.
    Skip,
}

impl StepStatus {
    /// The value persisted in `.runs/*.json`.
    pub fn code(&self) -> &'static str {
        match self {
            StepStatus::Pending => "pending",
            StepStatus::Pass => "pass",
            StepStatus::Fail => "fail",
            StepStatus::Blocked => "blocked",
            StepStatus::Skip => "skip",
        }
    }

    /// The inverse of `code` — parses a status back from its persisted
    /// `.runs/*.json` value. `None` if it isn't one of the five known codes.
    pub fn from_code(code: &str) -> Option<StepStatus> {
        match code {
            "pending" => Some(StepStatus::Pending),
            "pass" => Some(StepStatus::Pass),
            "fail" => Some(StepStatus::Fail),
            "blocked" => Some(StepStatus::Blocked),
            "skip" => Some(StepStatus::Skip),
            _ => None,
        }
    }

    /// The symbol used in reports and in the GUI's status column.
    pub fn symbol(&self) -> &'static str {
        match self {
            StepStatus::Pending => "⬜",
            StepStatus::Pass => "✅",
            StepStatus::Fail => "❌",
            StepStatus::Blocked => "⛔",
            StepStatus::Skip => "➖",
        }
    }

    /// True once a verdict has been recorded — `Skip` counts as executed.
    pub fn is_decided(&self) -> bool {
        !matches!(self, StepStatus::Pending)
    }
}

/// A scenario's status, rolled up from its steps' verdicts in the current run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScenarioStatus {
    /// No run yet, or no step of this scenario has a verdict.
    Pending,
    /// Some steps are decided and none of them failed or blocked, but at
    /// least one step is still pending.
    Running,
    /// Every step is decided, and none failed or blocked.
    Pass,
    /// At least one step's verdict is `Fail` — outranks every other status,
    /// even a step that hasn't run yet (see `scenario_status`).
    Fail,
    /// At least one step's verdict is `Blocked`, and none has failed.
    Blocked,
}

impl ScenarioStatus {
    /// The symbol shown next to a scenario in reports and in the GUI's
    /// scenario list.
    pub fn symbol(&self) -> &'static str {
        match self {
            ScenarioStatus::Pending => "⬜",
            ScenarioStatus::Running => "▶",
            ScenarioStatus::Pass => "✅",
            ScenarioStatus::Fail => "❌",
            ScenarioStatus::Blocked => "⛔",
        }
    }
}

/// One step of a scenario, as written in the Markdown.
#[derive(Clone, Debug)]
pub struct Step {
    /// 1-based position within the scenario. Assigned by the parser from the
    /// item's position, never from the number written in the file.
    pub number: usize,
    /// What the tester does.
    pub action: String,
    /// What should happen. Empty when the step was written without a `→`.
    pub expected: String,
}

/// One test scenario: an id, a title and an ordered list of steps.
#[derive(Clone, Debug)]
pub struct Scenario {
    /// Stable identifier (`SC-01`). Explicit in the heading when the author
    /// wrote one; otherwise generated from the scenario's position.
    pub id: String,
    pub title: String,
    /// Free text between the heading and the first step.
    pub description: String,
    pub tags: Vec<String>,
    /// What must be true before the first step runs.
    pub precondition: String,
    /// Any other `key: value` line under the heading, preserved in order.
    pub meta: Vec<(String, String)>,
    pub steps: Vec<Step>,
}

impl Scenario {
    /// The step with this 1-based number.
    pub fn step(&self, number: usize) -> Option<&Step> {
        self.steps.iter().find(|s| s.number == number)
    }
}

/// A whole suite — one Markdown file.
#[derive(Clone, Debug)]
pub struct Suite {
    /// File stem — the suite's id, and the name of its `.runs/<stem>.json`.
    pub stem: String,
    pub path: PathBuf,
    pub title: String,
    pub meta: Vec<(String, String)>,
    pub scenarios: Vec<Scenario>,
}

impl Suite {
    /// The scenario with this id (`SC-01`), if the suite has one.
    pub fn scenario(&self, id: &str) -> Option<&Scenario> {
        self.scenarios.iter().find(|s| s.id == id)
    }

    /// Every step in every scenario, summed — the denominator for a run's
    /// progress.
    pub fn total_steps(&self) -> usize {
        self.scenarios.iter().map(|s| s.steps.len()).sum()
    }
}

/// The verdict recorded for one step in one run.
#[derive(Clone, Debug)]
pub struct StepResult {
    pub scenario: String,
    /// 1-based step number within the scenario.
    pub step: usize,
    pub status: StepStatus,
    pub note: String,
    /// Paths to screenshots or files the tester attached.
    pub evidence: Vec<String>,
    /// When the verdict was recorded (`YYYY-MM-DD HH:MM:SS`, local time).
    pub at: String,
    /// The step's action text as it read when marked. Kept so a suite edited
    /// underneath a run can be detected instead of silently re-attributing an
    /// old verdict to a rewritten step.
    pub action: String,
}

/// What is being tested right now.
#[derive(Clone, Debug)]
pub struct Cursor {
    pub scenario: String,
    pub step: usize,
}

/// One pass over a suite.
#[derive(Clone, Debug)]
pub struct Run {
    /// `YYYYMMDD-HHMMSS`, unique within the suite.
    pub id: String,
    /// When the run began (`YYYY-MM-DD HH:MM:SS`, local time).
    pub started_at: String,
    /// `None` while the run is still open.
    pub finished_at: Option<String>,
    /// Free text describing where this ran (URL, build, browser, test user).
    pub environment: String,
    /// The step the GUI has open right now, if any.
    pub current: Option<Cursor>,
    /// Every verdict recorded so far — at most one per step; see `set_result`.
    pub results: Vec<StepResult>,
}

impl Run {
    /// The recorded verdict for a step, if one has been logged this run.
    pub fn result(&self, scenario: &str, step: usize) -> Option<&StepResult> {
        self.results.iter().find(|r| r.scenario == scenario && r.step == step)
    }

    /// The verdict on a step, or `Pending` when it has none.
    pub fn status_of(&self, scenario: &str, step: usize) -> StepStatus {
        self.result(scenario, step).map_or(StepStatus::Pending, |r| r.status)
    }

    /// Records a verdict, replacing any previous one for the same step — a
    /// step is re-testable within a run, and only the latest verdict counts.
    pub fn set_result(&mut self, result: StepResult) {
        match self
            .results
            .iter_mut()
            .find(|r| r.scenario == result.scenario && r.step == result.step)
        {
            Some(existing) => *existing = result,
            None => self.results.push(result),
        }
    }

    /// True while the run has no `finished_at` yet — still being recorded.
    pub fn is_open(&self) -> bool {
        self.finished_at.is_none()
    }
}

/// Everything recorded for one suite: its whole run history.
#[derive(Clone, Debug, Default)]
pub struct RunFile {
    pub suite: String,
    /// Id of the run the GUI is currently recording into.
    pub active_run: Option<String>,
    /// Oldest first.
    pub runs: Vec<Run>,
}

impl RunFile {
    /// The run the GUI is currently recording into, if any.
    pub fn active(&self) -> Option<&Run> {
        let id = self.active_run.as_deref()?;
        self.runs.iter().find(|r| r.id == id)
    }

    /// Mutable access to the active run, for recording a new verdict into it.
    pub fn active_mut(&mut self) -> Option<&mut Run> {
        let id = self.active_run.clone()?;
        self.runs.iter_mut().find(|r| r.id == id)
    }

    /// The run with this id, from anywhere in the history.
    pub fn run(&self, id: &str) -> Option<&Run> {
        self.runs.iter().find(|r| r.id == id)
    }

    /// The run a report defaults to: the active one, else the most recent.
    pub fn latest(&self) -> Option<&Run> {
        self.active().or_else(|| self.runs.last())
    }
}

/// A step's verdict count across a suite, plus the scenario-level roll-up.
#[derive(Clone, Copy, Debug, Default)]
pub struct Tally {
    pub total_steps: usize,
    pub done_steps: usize,
    pub pass: usize,
    pub fail: usize,
    pub blocked: usize,
    pub skip: usize,
    pub scenarios_pass: usize,
    /// Scenarios whose every step was skipped. Kept apart from `scenarios_pass`
    /// because such a scenario rolls up to `Pass` (a skip counts as decided)
    /// yet nobody actually tested it.
    pub scenarios_skipped: usize,
    pub scenarios_fail: usize,
    pub scenarios_blocked: usize,
    pub scenarios_pending: usize,
}

/// A scenario's status rolled up from its steps.
///
/// Precedence is fail › blocked › running/pending › pass: one failed step
/// makes the scenario failed no matter how many passed, and a scenario is only
/// `Pass` once every step has a verdict and none is fail or blocked.
pub fn scenario_status(scenario: &Scenario, run: Option<&Run>) -> ScenarioStatus {
    let Some(run) = run else { return ScenarioStatus::Pending };
    let mut decided = 0usize;
    let mut blocked = false;
    for step in &scenario.steps {
        match run.status_of(&scenario.id, step.number) {
            StepStatus::Fail => return ScenarioStatus::Fail,
            StepStatus::Blocked => {
                blocked = true;
                decided += 1;
            }
            StepStatus::Pass | StepStatus::Skip => decided += 1,
            StepStatus::Pending => {}
        }
    }
    if blocked {
        ScenarioStatus::Blocked
    } else if decided == 0 {
        ScenarioStatus::Pending
    } else if decided < scenario.steps.len() {
        ScenarioStatus::Running
    } else {
        ScenarioStatus::Pass
    }
}

/// How many of a scenario's steps have a verdict.
pub fn scenario_done(scenario: &Scenario, run: Option<&Run>) -> usize {
    let Some(run) = run else { return 0 };
    scenario
        .steps
        .iter()
        .filter(|s| run.status_of(&scenario.id, s.number).is_decided())
        .count()
}

/// Counts every step verdict and every scenario roll-up in one pass — what the
/// report header and the GUI status bar both display.
pub fn tally(suite: &Suite, run: Option<&Run>) -> Tally {
    let mut t = Tally { total_steps: suite.total_steps(), ..Tally::default() };
    for scenario in &suite.scenarios {
        if let Some(run) = run {
            for step in &scenario.steps {
                match run.status_of(&scenario.id, step.number) {
                    StepStatus::Pass => t.pass += 1,
                    StepStatus::Fail => t.fail += 1,
                    StepStatus::Blocked => t.blocked += 1,
                    StepStatus::Skip => t.skip += 1,
                    StepStatus::Pending => continue,
                }
                t.done_steps += 1;
            }
        }
        match scenario_status(scenario, run) {
            // An all-skipped scenario also rolls up to `Pass`, but counting it
            // as OK would overstate the run in the one line a reader trusts
            // most. A step-less scenario is not "all skipped" — it is empty.
            ScenarioStatus::Pass => {
                let all_skipped = !scenario.steps.is_empty()
                    && scenario.steps.iter().all(|step| {
                        run.is_some_and(|r| {
                            r.status_of(&scenario.id, step.number) == StepStatus::Skip
                        })
                    });
                if all_skipped {
                    t.scenarios_skipped += 1;
                } else {
                    t.scenarios_pass += 1;
                }
            }
            ScenarioStatus::Fail => t.scenarios_fail += 1,
            ScenarioStatus::Blocked => t.scenarios_blocked += 1,
            ScenarioStatus::Pending | ScenarioStatus::Running => t.scenarios_pending += 1,
        }
    }
    t
}

/// True when the suite was edited after this verdict was recorded — the step's
/// action text no longer matches what the tester saw.
pub fn is_stale(step: &Step, result: &StepResult) -> bool {
    !result.action.is_empty() && result.action != step.action
}
