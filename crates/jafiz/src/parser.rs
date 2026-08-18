//! Markdown → `Suite`. A line-based parser, deliberately not a Markdown
//! library: the format is a small, closed subset (spec §4) and the workspace
//! takes no dependency it can avoid.
//!
//! The parser is tolerant on purpose — an LLM writing "ordinary Markdown"
//! should produce a valid suite without being taught a DSL. Anything it cannot
//! interpret becomes a `Diagnostic` rather than a hard failure, so `jafiz
//! check` can explain the problem instead of just refusing the file.

use crate::model::{Scenario, Step, Suite};
use std::path::Path;

/// The canonical example suite — the fixture `jafiz check` and `--dump` run
/// against, and the skeleton `jafiz new` copies.
pub const EXAMPLE: &str = include_str!("../assets/example.md");
/// The format contract `jafiz format` prints (pt-BR).
pub const GUIDE_PT: &str = include_str!("../assets/format-guide.pt.md");
/// The format contract `jafiz format` prints (en).
pub const GUIDE_EN: &str = include_str!("../assets/format-guide.en.md");

/// How serious a diagnostic is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    /// The suite cannot be executed as written.
    Error,
    /// Usable, but something will bite later.
    Warning,
}

impl Severity {
    /// The word `jafiz check` prints next to a diagnostic of this severity.
    pub fn label(&self) -> &'static str {
        match self {
            Severity::Error => "erro",
            Severity::Warning => "aviso",
        }
    }
}

/// One problem the parser noticed while reading the suite.
#[derive(Clone, Debug)]
pub struct Diagnostic {
    /// 1-based line in the source file; 0 when the problem is the file itself.
    pub line: usize,
    pub severity: Severity,
    pub message: String,
}

/// A suite as parsed, plus everything the parser noticed about the source text.
pub struct ParseOutcome {
    pub suite: Suite,
    pub diagnostics: Vec<Diagnostic>,
}

impl ParseOutcome {
    /// True when at least one diagnostic is `Severity::Error`.
    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(|d| d.severity == Severity::Error)
    }
}

/// Where the parser is inside a scenario — metadata and description are only
/// accepted before the first step, so a `key: value` line further down is
/// treated as prose instead of silently becoming metadata.
enum Section {
    Preamble,
    Steps,
}

/// Parses `text` into a `Suite`. Never fails outright — anything the format
/// can't make sense of becomes a diagnostic instead (see the module doc).
pub fn parse(path: &Path, text: &str) -> ParseOutcome {
    let stem = path.file_stem().map_or_else(String::new, |s| s.to_string_lossy().to_string());
    let mut suite = Suite {
        stem: stem.clone(),
        path: path.to_path_buf(),
        title: String::new(),
        meta: Vec::new(),
        scenarios: Vec::new(),
    };
    let mut diagnostics = Vec::new();
    let mut section = Section::Preamble;

    for (index, raw) in text.trim_start_matches('\u{feff}').lines().enumerate() {
        let line_no = index + 1;
        let line = raw.trim_end();
        let trimmed = line.trim();
        let indented = line.starts_with("  ") || line.starts_with('\t');

        // Rule 9 — ignored lines.
        let horizontal_rule =
            trimmed.len() >= 3 && trimmed.chars().all(|c| c == '-' || c == '=');
        if trimmed.is_empty()
            || trimmed.starts_with("<!--")
            || trimmed.starts_with('>')
            || horizontal_rule
        {
            continue;
        }

        // Rule 1 — suite title.
        if let Some(rest) = trimmed.strip_prefix("# ") {
            if suite.title.is_empty() {
                suite.title = rest.trim().to_string();
            }
            continue;
        }

        // Rule 3 — a new scenario.
        if let Some(rest) = trimmed.strip_prefix("## ") {
            let position = suite.scenarios.len() + 1;
            let (id, title, explicit) = split_heading(rest.trim(), position);
            if !explicit {
                diagnostics.push(Diagnostic {
                    line: line_no,
                    severity: Severity::Warning,
                    message: format!("cenário sem id explícito; usando '{id}'"),
                });
            }
            if suite.scenarios.iter().any(|s| s.id == id) {
                diagnostics.push(Diagnostic {
                    line: line_no,
                    severity: Severity::Error,
                    message: format!("id de cenário duplicado: {id}"),
                });
            }
            suite.scenarios.push(Scenario {
                id,
                title,
                description: String::new(),
                tags: Vec::new(),
                precondition: String::new(),
                meta: Vec::new(),
                steps: Vec::new(),
            });
            section = Section::Preamble;
            continue;
        }

        // Rule 2 — suite metadata, before any scenario. Handled by emptiness
        // rather than by `let … else` on `last_mut()`: pushing to `suite.meta`
        // inside the else of a borrow taken from `suite` is NLL problem case
        // #3 and does not compile on stable.
        if suite.scenarios.is_empty() {
            if let Some((key, value)) = split_meta(trimmed) {
                suite.meta.push((key, value));
            }
            continue;
        }
        let Some(scenario) = suite.scenarios.last_mut() else { continue };

        // Rule 6 — a step.
        if let Some(text) = strip_list_marker(trimmed) {
            let number = scenario.steps.len() + 1;
            let (action, expected) = split_arrow(text);
            scenario.steps.push(Step { number, action, expected });
            section = Section::Steps;
            continue;
        }

        // Rule 8 — a continuation of the step above.
        if indented && matches!(section, Section::Steps) {
            if let Some(step) = scenario.steps.last_mut() {
                append_continuation(step, trimmed);
                continue;
            }
        }

        if matches!(section, Section::Steps) {
            // Prose after the steps began — keep it out of the model rather
            // than guessing which step it belongs to.
            continue;
        }

        // Rules 4 and 5 — scenario metadata, then description.
        match split_meta(trimmed) {
            Some((key, value)) => match key.to_lowercase().as_str() {
                "tags" | "tag" => {
                    scenario.tags = value
                        .split(',')
                        .map(|t| t.trim().to_string())
                        .filter(|t| !t.is_empty())
                        .collect()
                }
                "pré" | "pre" | "precondição" | "precondicao" | "precondition" | "given" => {
                    scenario.precondition = value
                }
                _ => scenario.meta.push((key, value)),
            },
            None => {
                if !scenario.description.is_empty() {
                    scenario.description.push(' ');
                }
                scenario.description.push_str(trimmed);
            }
        }
    }

    if suite.title.is_empty() {
        suite.title = stem;
        diagnostics.push(Diagnostic {
            line: 0,
            severity: Severity::Warning,
            message: "suíte sem título '# '; usando o nome do arquivo".into(),
        });
    }
    if suite.scenarios.is_empty() {
        diagnostics.push(Diagnostic {
            line: 0,
            severity: Severity::Error,
            message: "nenhum cenário encontrado (esperado ao menos um '## ')".into(),
        });
    }
    for scenario in &suite.scenarios {
        if scenario.steps.is_empty() {
            diagnostics.push(Diagnostic {
                line: 0,
                severity: Severity::Error,
                message: format!("cenário {} não tem passos", scenario.id),
            });
        }
        for step in &scenario.steps {
            if step.expected.is_empty() {
                diagnostics.push(Diagnostic {
                    line: 0,
                    severity: Severity::Warning,
                    message: format!(
                        "{} passo {}: sem resultado esperado (use '→')",
                        scenario.id, step.number
                    ),
                });
            }
        }
    }

    ParseOutcome { suite, diagnostics }
}

/// Reads `path` from disk, then parses it.
pub fn parse_file(path: &Path) -> std::io::Result<ParseOutcome> {
    let text = std::fs::read_to_string(path)?;
    Ok(parse(path, &text))
}

/// Splits `SC-01 · Título` into its id and title (rule 3). Returns the
/// generated id and `false` when the heading carries none.
fn split_heading(heading: &str, position: usize) -> (String, String, bool) {
    let mut chars = heading.char_indices();
    let mut end = 0usize;
    let mut seen_dash = false;
    let mut digits_after_dash = 0usize;
    for (offset, ch) in &mut chars {
        if ch.is_ascii_uppercase() && !seen_dash {
            end = offset + ch.len_utf8();
        } else if ch == '-' && !seen_dash && end > 0 {
            seen_dash = true;
            end = offset + ch.len_utf8();
        } else if ch.is_ascii_digit() && seen_dash {
            digits_after_dash += 1;
            end = offset + ch.len_utf8();
        } else {
            break;
        }
    }
    // Rule 3 also requires a separator after the id. Without this check
    // `CT-500ms timeout test` silently splits into id `CT-500` and title
    // `ms timeout test` — a wrong answer with no diagnostic, which is exactly
    // what this parser's tolerance is not supposed to mean.
    let separated =
        end == heading.len() || heading[end..].starts_with([' ', '·', '-', '—', ':', '\t']);
    if seen_dash && digits_after_dash > 0 && separated {
        let id = heading[..end].to_string();
        let title = heading[end..]
            .trim_start_matches([' ', '·', '-', '—', ':', '\t'])
            .trim()
            .to_string();
        return (id, title, true);
    }
    (format!("SC-{position:02}"), heading.to_string(), false)
}

/// `key: value`, where the key has no spaces-then-colon ambiguity with prose.
/// A line is metadata only when the colon comes before any sentence-like
/// punctuation and the key is a single short word.
fn split_meta(line: &str) -> Option<(String, String)> {
    let (key, value) = line.split_once(':')?;
    let key = key.trim();
    if key.is_empty() || key.split_whitespace().count() > 1 || key.chars().count() > 20 {
        return None;
    }
    Some((key.to_string(), value.trim().to_string()))
}

/// Strips `1.`, `1)`, `-`, `*` or `+` from the front of a step (rule 6).
fn strip_list_marker(line: &str) -> Option<&str> {
    for marker in ['-', '*', '+'] {
        if let Some(rest) = line.strip_prefix(marker) {
            if rest.starts_with(' ') {
                return Some(rest.trim_start());
            }
        }
    }
    let digits: String = line.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return None;
    }
    let rest = &line[digits.len()..];
    let rest = rest.strip_prefix('.').or_else(|| rest.strip_prefix(')'))?;
    if rest.starts_with(' ') {
        Some(rest.trim_start())
    } else {
        None
    }
}

/// Splits a step on the first `→`, `->` or `=>` (rule 7).
fn split_arrow(text: &str) -> (String, String) {
    for arrow in ["→", "->", "=>"] {
        if let Some((action, expected)) = text.split_once(arrow) {
            return (action.trim().to_string(), expected.trim().to_string());
        }
    }
    (text.trim().to_string(), String::new())
}

/// Appends a continuation line to the expected result if the step already has
/// one, otherwise to the action — and honours an arrow that only appears on
/// the continuation line (rule 8).
fn append_continuation(step: &mut Step, text: &str) {
    if step.expected.is_empty() {
        let (action, expected) = split_arrow(text);
        if !expected.is_empty() {
            if !action.is_empty() {
                step.action.push(' ');
                step.action.push_str(&action);
            }
            step.expected = expected;
            return;
        }
        step.action.push(' ');
        step.action.push_str(text);
        return;
    }
    step.expected.push(' ');
    step.expected.push_str(text);
}
