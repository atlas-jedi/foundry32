//! Model → Markdown. Four renderings over the same data:
//!
//! - `report` — the compact result Claude reads: failures first, with the step
//!   that failed and the tester's note; passing scenarios collapsed to a line.
//! - `show` — the whole suite with every step annotated, for reading a suite
//!   in the terminal.
//! - `status_line` / `progress_line` — one-liners for `jafiz status` and the
//!   GUI status bar.
//! - `list_row` — one line per suite for `jafiz list`.
//!
//! Bilingual: the CLI passes the language from settings, so a report comes
//! back in the language the user chose in the GUI.

use crate::model::{
    scenario_done, scenario_status, tally, Run, Scenario, ScenarioStatus, StepResult, StepStatus,
    Suite,
};
use foundry_common::lang::Lang;

struct T {
    run_line: &'static str,
    running: &'static str,
    finished: &'static str,
    environment: &'static str,
    progress: &'static str,
    steps: &'static str,
    scenarios_word: &'static str,
    scenarios_ok: &'static str,
    scenarios_skipped: &'static str,
    failed: &'static str,
    blocked: &'static str,
    step_word: &'static str,
    verdict_fail: &'static str,
    verdict_blocked: &'static str,
    verdict_skip: &'static str,
    /// A step that passed but carries a note or evidence — the verdict word
    /// that keeps the tester's own words attached to what actually happened.
    verdict_pass: &'static str,
    /// A step annotated before any verdict was recorded (`runs::set_note`
    /// deliberately allows that: something looks off mid-step).
    verdict_pending: &'static str,
    evidence: &'static str,
    label_tags: &'static str,
    label_pre: &'static str,
    section_passed: &'static str,
    section_pending: &'static str,
    step_count: &'static str,
    no_run: &'static str,
    testing_now: &'static str,
    all_done: &'static str,
    stale: &'static str,
}

static PT: T = T {
    run_line: "Execução %I · iniciada %S",
    running: "em andamento",
    finished: "encerrada %F",
    environment: "Ambiente: %E",
    progress: "Progresso: %D/%T passos",
    steps: "passos",
    scenarios_word: "cenários",
    scenarios_ok: "%N cenários OK",
    scenarios_skipped: "%N pulado",
    failed: "%N falhou",
    blocked: "%N bloqueado",
    step_word: "Passo",
    verdict_fail: "FALHOU",
    verdict_blocked: "BLOQUEADO",
    verdict_skip: "PULADO",
    verdict_pass: "PASSOU",
    verdict_pending: "PENDENTE",
    evidence: "evidência",
    label_tags: "tags:",
    label_pre: "pré:",
    section_passed: "Passaram",
    section_pending: "Não executados",
    step_count: "%N passos",
    no_run: "Nenhuma execução registrada.",
    testing_now: "testando agora: %C passo %S",
    all_done: "todos os passos com veredito",
    stale: "(o passo mudou no arquivo depois desta marcação)",
};

static EN: T = T {
    run_line: "Run %I · started %S",
    running: "in progress",
    finished: "finished %F",
    environment: "Environment: %E",
    progress: "Progress: %D/%T steps",
    steps: "steps",
    scenarios_word: "scenarios",
    scenarios_ok: "%N scenarios OK",
    scenarios_skipped: "%N skipped",
    failed: "%N failed",
    blocked: "%N blocked",
    step_word: "Step",
    verdict_fail: "FAILED",
    verdict_blocked: "BLOCKED",
    verdict_skip: "SKIPPED",
    verdict_pass: "PASSED",
    verdict_pending: "PENDING",
    evidence: "evidence",
    label_tags: "tags:",
    label_pre: "pre:",
    section_passed: "Passed",
    section_pending: "Not executed",
    step_count: "%N steps",
    no_run: "No run recorded.",
    testing_now: "testing now: %C step %S",
    all_done: "every step has a verdict",
    stale: "(the step changed in the file after this verdict)",
};

fn t(lang: Lang) -> &'static T {
    match lang {
        Lang::PtBr => &PT,
        Lang::En => &EN,
    }
}

/// `SC-02 · Cupom de desconto`
fn heading(scenario: &Scenario) -> String {
    if scenario.title.is_empty() {
        scenario.id.clone()
    } else {
        format!("{} · {}", scenario.id, scenario.title)
    }
}

/// `Aplicar o cupom BLACK10 → o total cai 10%`
fn step_text(action: &str, expected: &str) -> String {
    if expected.is_empty() {
        action.to_string()
    } else {
        format!("{action} → {expected}")
    }
}

fn header(suite: &Suite, run: Option<&Run>, lang: Lang) -> String {
    let tr = t(lang);
    let mut out = format!("# JAFIZ · {}\n", suite.title);
    let Some(run) = run else {
        out.push_str(tr.no_run);
        out.push('\n');
        return out;
    };
    let state = match &run.finished_at {
        Some(at) => tr.finished.replace("%F", at),
        None => tr.running.to_string(),
    };
    out.push_str(&format!(
        "{} · {}\n",
        tr.run_line.replace("%I", &run.id).replace("%S", &run.started_at),
        state
    ));
    if !run.environment.is_empty() {
        out.push_str(&tr.environment.replace("%E", &run.environment));
        out.push('\n');
    }
    out.push_str(&progress_line(suite, Some(run), lang));
    out.push('\n');
    out
}

/// `Progresso: 12/18 passos · 2 cenários OK · 1 falhou · 1 bloqueado`
pub fn progress_line(suite: &Suite, run: Option<&Run>, lang: Lang) -> String {
    let tr = t(lang);
    let counts = tally(suite, run);
    let mut parts = vec![tr
        .progress
        .replace("%D", &counts.done_steps.to_string())
        .replace("%T", &counts.total_steps.to_string())];
    if counts.scenarios_pass > 0 {
        parts.push(tr.scenarios_ok.replace("%N", &counts.scenarios_pass.to_string()));
    }
    if counts.scenarios_skipped > 0 {
        parts.push(tr.scenarios_skipped.replace("%N", &counts.scenarios_skipped.to_string()));
    }
    if counts.scenarios_fail > 0 {
        parts.push(tr.failed.replace("%N", &counts.scenarios_fail.to_string()));
    }
    if counts.scenarios_blocked > 0 {
        parts.push(tr.blocked.replace("%N", &counts.scenarios_blocked.to_string()));
    }
    parts.join(" · ")
}

/// The compact report Claude reads.
pub fn report(suite: &Suite, run: Option<&Run>, lang: Lang) -> String {
    let tr = t(lang);
    let mut out = header(suite, run, lang);
    let Some(run) = run else { return out };

    // Anything needing attention comes first, in suite order, with the
    // offending step verbatim. Skipped steps qualify: `scenario_status` rolls
    // an all-skipped scenario up to `Pass`, and a scenario nobody executed
    // reading as verified is the one lie this report must never tell. So does
    // an annotated step — see `is_annotated`.
    for scenario in &suite.scenarios {
        if !needs_detail(scenario, run) {
            continue;
        }
        out.push_str(&format!(
            "\n## {} {}\n",
            detail_symbol(scenario, run),
            heading(scenario)
        ));
        for step in &scenario.steps {
            let Some(result) = run.result(&scenario.id, step.number) else { continue };
            let verdict = match result.status {
                StepStatus::Fail => tr.verdict_fail,
                StepStatus::Blocked => tr.verdict_blocked,
                StepStatus::Skip => tr.verdict_skip,
                // A step the tester wrote on is rendered whatever its verdict,
                // with its own status word — never dropped, and never dressed
                // up as a failure it was not.
                StepStatus::Pass if is_annotated(result) => tr.verdict_pass,
                StepStatus::Pending if is_annotated(result) => tr.verdict_pending,
                _ => continue,
            };
            out.push_str(&format!(
                "- {} {} — {}\n",
                tr.step_word,
                step.number,
                step_text(&step.action, &step.expected)
            ));
            let note = if result.note.is_empty() {
                String::new()
            } else {
                format!(": {}", result.note)
            };
            out.push_str(&format!("  **{verdict}**{note}\n"));
            if crate::model::is_stale(step, result) {
                out.push_str(&format!("  {}\n", tr.stale));
            }
            for path in &result.evidence {
                out.push_str(&format!("  {}: {path}\n", tr.evidence));
            }
        }
    }

    let mut passed = Vec::new();
    let mut pending = Vec::new();
    for scenario in &suite.scenarios {
        if needs_detail(scenario, run) {
            continue; // already rendered in full above
        }
        let line = format!(
            "- {} ({})\n",
            heading(scenario),
            tr.step_count.replace("%N", &scenario.steps.len().to_string())
        );
        // Exhaustive on purpose: a new ScenarioStatus variant must force a
        // decision here rather than silently vanishing from every report.
        match scenario_status(scenario, Some(run)) {
            ScenarioStatus::Pass => passed.push(line),
            ScenarioStatus::Pending => pending.push(line),
            ScenarioStatus::Running => pending.push(format!(
                "- {} ({}/{})\n",
                heading(scenario),
                scenario_done(scenario, Some(run)),
                scenario.steps.len()
            )),
            ScenarioStatus::Fail | ScenarioStatus::Blocked => {}
        }
    }
    if !passed.is_empty() {
        out.push_str(&format!("\n## ✅ {}\n", tr.section_passed));
        out.extend(passed);
    }
    if !pending.is_empty() {
        out.push_str(&format!("\n## ⬜ {}\n", tr.section_pending));
        out.extend(pending);
    }
    out
}

/// True when the tester deliberately recorded something against a step: a note
/// or an evidence path.
///
/// Such a step has to reach the report whatever its verdict. "passou, mas
/// demorou 8s" is exactly what this tool exists to carry back, and so is a
/// screenshot of a step that worked; a note written before any verdict counts
/// too, which is why `runs::set_note` creates a bare `Pending` result. Rolling
/// the scenario up to a one-line ✅ would silently throw all three away.
fn is_annotated(result: &StepResult) -> bool {
    !result.note.is_empty() || !result.evidence.is_empty()
}

/// True when any of a scenario's steps was skipped.
fn has_skip(scenario: &Scenario, run: &Run) -> bool {
    scenario
        .steps
        .iter()
        .any(|step| run.status_of(&scenario.id, step.number) == StepStatus::Skip)
}

/// True when a scenario must be rendered in full rather than summarised: it
/// failed, it was blocked, a step was skipped, or a step carries a note or
/// evidence. The last two cases are why this is not just a status check —
/// `scenario_status` counts a skip as decided, so an all-skipped scenario rolls
/// up to `Pass`, and a passing step's note has no effect on the roll-up at all;
/// either would otherwise collapse into the one-line "passed" list, taking the
/// tester's own words with it.
///
/// This is the sole partition rule: the summary loop skips exactly the
/// scenarios this returns true for, so every scenario appears in exactly one
/// section.
fn needs_detail(scenario: &Scenario, run: &Run) -> bool {
    if matches!(
        scenario_status(scenario, Some(run)),
        ScenarioStatus::Fail | ScenarioStatus::Blocked
    ) {
        return true;
    }
    if has_skip(scenario, run) {
        return true;
    }
    scenario
        .steps
        .iter()
        .any(|step| run.result(&scenario.id, step.number).is_some_and(is_annotated))
}

/// The heading symbol for a detailed scenario: its own status, except that one
/// with a skipped step is marked skipped rather than passed — a scenario nobody
/// executed must never be headed with ✅. A scenario detailed only because a
/// passing step carries a note keeps its own ✅: the note is worth reading, but
/// nothing about it was skipped.
fn detail_symbol(scenario: &Scenario, run: &Run) -> &'static str {
    let status = scenario_status(scenario, Some(run));
    if matches!(status, ScenarioStatus::Fail | ScenarioStatus::Blocked) {
        return status.symbol();
    }
    if has_skip(scenario, run) {
        return StepStatus::Skip.symbol();
    }
    status.symbol()
}

/// The whole suite, every step annotated with its status — `jafiz show`.
pub fn show(suite: &Suite, run: Option<&Run>, lang: Lang) -> String {
    let tr = t(lang);
    let mut out = header(suite, run, lang);
    for scenario in &suite.scenarios {
        out.push_str(&format!(
            "\n## {} {}\n",
            scenario_status(scenario, run).symbol(),
            heading(scenario)
        ));
        if !scenario.tags.is_empty() {
            out.push_str(&format!("{} {}\n", tr.label_tags, scenario.tags.join(", ")));
        }
        if !scenario.precondition.is_empty() {
            out.push_str(&format!("{} {}\n", tr.label_pre, scenario.precondition));
        }
        for step in &scenario.steps {
            let result = run.and_then(|r| r.result(&scenario.id, step.number));
            let status = result.map_or(StepStatus::Pending, |r| r.status);
            out.push_str(&format!(
                "{}. {} {}\n",
                step.number,
                status.symbol(),
                step_text(&step.action, &step.expected)
            ));
            if let Some(result) = result {
                // `show` is the view built to annotate every step, so it is
                // exactly where a verdict recorded against wording the file no
                // longer carries has to surface.
                if crate::model::is_stale(step, result) {
                    out.push_str(&format!("   {}\n", tr.stale));
                }
                if !result.note.is_empty() {
                    out.push_str(&format!("   — {}\n", result.note));
                }
                for path in &result.evidence {
                    out.push_str(&format!("   {}: {path}\n", tr.evidence));
                }
            }
        }
    }
    out
}

/// One line: what is being tested right now — `jafiz status`.
pub fn status_line(suite: &Suite, run: Option<&Run>, lang: Lang) -> String {
    let tr = t(lang);
    let Some(run) = run else { return tr.no_run.to_string() };
    let where_at = match &run.current {
        Some(cursor) => tr
            .testing_now
            .replace("%C", &cursor.scenario)
            .replace("%S", &cursor.step.to_string()),
        None => tr.all_done.to_string(),
    };
    format!(
        "{} · {} · {}",
        suite.title,
        progress_line(suite, Some(run), lang),
        where_at
    )
}

/// One line per suite — `jafiz list`:
/// `checkout   5 cenários  18 passos   20260817-143205 · Progresso: 12/18 …`
pub fn list_row(suite: &Suite, run: Option<&Run>, lang: Lang) -> String {
    let tr = t(lang);
    let state = match run {
        None => tr.no_run.to_string(),
        Some(run) => format!("{} · {}", run.id, progress_line(suite, Some(run), lang)),
    };
    format!(
        "{:<24} {:>3} {:<10} {:>3} {:<7}  {}",
        truncate(&suite.stem, 24),
        suite.scenarios.len(),
        tr.scenarios_word,
        suite.total_steps(),
        tr.steps,
        state
    )
}

/// Truncates by character count (titles carry `·` and accents), adding `…`.
fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}
