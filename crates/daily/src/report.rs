//! Day → text. The grouped report is the one text the stand-up needs, and it
//! is the same whether it reaches the clipboard from the window or Claude
//! from `daily report`. The JSON shapes are what the CLI hands scripts.

use crate::clock::Date;
use crate::model::{group_by_task, Author, Day, Entry};
use foundry_common::lang::Lang;
use serde_json::{json, Value};

fn others_label(lang: Lang) -> &'static str {
    match lang {
        Lang::PtBr => "Outros",
        Lang::En => "Other",
    }
}

/// `Daily · qua, 04/11/2026`.
pub fn heading(date: Date, lang: Lang) -> String {
    format!("Daily · {}", date.display(lang))
}

/// The day grouped by task, ready to read out at the stand-up:
///
/// ```text
/// Daily · qua, 04/11/2026
///
/// ABC-123 · login fix
/// - Reviewed and approved the PR
///
/// Outros
/// - Sprint planning
/// ```
pub fn render_day(day: &Day, date: Date, lang: Lang) -> String {
    let mut out = heading(date, lang);
    out.push('\n');
    let groups = group_by_task(day);
    if groups.is_empty() {
        out.push('\n');
        out.push_str(match lang {
            Lang::PtBr => "(nenhuma anotação neste dia)",
            Lang::En => "(nothing written down on this day)",
        });
        out.push('\n');
        return out;
    }
    for group in groups {
        out.push('\n');
        let label = if group.key.is_empty() { others_label(lang) } else { group.label.as_str() };
        out.push_str(label);
        out.push('\n');
        for entry in group.entries {
            out.push_str("- ");
            out.push_str(&entry.text);
            out.push('\n');
        }
    }
    out
}

/// Who wrote it, as a list column or a CLI line says it.
pub fn author_label(author: Author, lang: Lang) -> &'static str {
    match (author, lang) {
        (Author::User, Lang::PtBr) => "você",
        (Author::User, Lang::En) => "you",
        (Author::Claude, _) => "Claude",
    }
}

/// One line per entry, with what `daily edit` / `daily rm` need (the id):
/// `09:12  20261104-091233  [ABC-123 · login fix]  Reviewed the PR  (Claude)`.
pub fn render_list(day: &Day, date: Date, lang: Lang) -> String {
    let entries = day.sorted();
    let mut out = format!("{} · {}\n", date.display(lang), entry_count(entries.len(), lang));
    for entry in entries {
        out.push_str(&list_line(entry, lang));
        out.push('\n');
    }
    out
}

/// `1 anotação` / `3 anotações` — also what the window's status bar says.
pub fn entry_count(count: usize, lang: Lang) -> String {
    match (lang, count) {
        (Lang::PtBr, 1) => "1 anotação".into(),
        (Lang::PtBr, n) => format!("{n} anotações"),
        (Lang::En, 1) => "1 entry".into(),
        (Lang::En, n) => format!("{n} entries"),
    }
}

fn list_line(entry: &Entry, lang: Lang) -> String {
    let task = if entry.task.is_empty() { String::new() } else { format!("[{}]  ", entry.task) };
    let author = match entry.author {
        Author::Claude => format!("  ({})", author_label(Author::Claude, lang)),
        Author::User => String::new(),
    };
    format!("{}  {}  {task}{}{author}", entry.time, entry.id, entry.text)
}

fn entry_json(entry: &Entry) -> Value {
    json!({
        "id": entry.id,
        "time": entry.time,
        "task": entry.task,
        "text": entry.text,
        "author": entry.author.code(),
    })
}

/// `daily list --json`: every entry of the day, in time order.
pub fn list_json(day: &Day, date: Date) -> Value {
    json!({
        "date": date.iso(),
        "entries": day.sorted().into_iter().map(entry_json).collect::<Vec<_>>(),
    })
}

/// `daily report --json`: the day grouped by task. `key` is the grouping key
/// (`ABC-123`, a lower-cased label, or "" for the entries with no task).
pub fn report_json(day: &Day, date: Date) -> Value {
    let groups: Vec<Value> = group_by_task(day)
        .into_iter()
        .map(|group| {
            json!({
                "key": group.key,
                "label": group.label,
                "entries": group.entries.into_iter().map(entry_json).collect::<Vec<_>>(),
            })
        })
        .collect();
    json!({ "date": date.iso(), "groups": groups })
}

/// `daily style`: the user's own latest entries, for Claude to write new ones
/// the same way — length, tense, level of detail, how tasks are labelled.
pub fn render_style(examples: &[(Date, Entry)], lang: Lang) -> String {
    let mut out = String::from(match lang {
        Lang::PtBr => "# Como o usuário escreve (últimas anotações feitas na janela, mais recentes primeiro)\n\
                       Escreva anotações novas no mesmo jeito: mesmo tamanho, mesmo tempo verbal, mesmo nível de detalhe.\n\n",
        Lang::En => "# How the user writes (latest entries typed in the window, newest first)\n\
                     Write new entries the same way: same length, same tense, same level of detail.\n\n",
    });
    if examples.is_empty() {
        out.push_str(match lang {
            Lang::PtBr => "(nenhum exemplo ainda: o usuário não escreveu nada pela janela)\n",
            Lang::En => "(no examples yet: nothing has been typed in the window)\n",
        });
        return out;
    }
    for (date, entry) in examples {
        let task = if entry.task.is_empty() { "-".to_string() } else { entry.task.clone() };
        out.push_str(&format!("- {} · [{task}] {}\n", date.iso(), entry.text));
    }
    out
}

/// The line a front-end shows when a day file could not be read.
pub fn damaged_line(reason: &str, lang: Lang) -> String {
    match lang {
        Lang::PtBr => format!("⚠ arquivo do dia danificado, nada foi gravado por cima: {reason}"),
        Lang::En => format!("⚠ day file damaged, nothing was written over it: {reason}"),
    }
}
