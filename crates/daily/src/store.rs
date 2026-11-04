//! Everything that touches the filesystem: where the day files live, which
//! days have one, and the JSON each day is stored as.
//!
//! One file per calendar day, `<data dir>\2026-11-04.json`. A day is small —
//! a few dozen lines at most — so it is always read and written whole, and a
//! per-day file keeps "what did I do yesterday" a single read.

use crate::clock::Date;
use crate::model::{group_key, Author, Day, Entry};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Environment variable that points the whole tool at another folder — for
/// trying it out, or testing, without touching the real days.
pub const DIR_ENV: &str = "DAILY_DIR";

/// Where the day files live: `DAILY_DIR` when set, else
/// `%LOCALAPPDATA%\Software Imperial\Foundry32\daily\days`. Shares the hub's
/// root, like JAFIZ's library, so uninstalling Foundry32 takes it along.
pub fn data_dir() -> PathBuf {
    if let Ok(dir) = std::env::var(DIR_ENV) {
        if !dir.trim().is_empty() {
            return PathBuf::from(dir);
        }
    }
    let local = std::env::var("LOCALAPPDATA")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| {
            let home = std::env::var("USERPROFILE").unwrap_or_else(|_| ".".into());
            format!("{home}\\AppData\\Local")
        });
    PathBuf::from(local).join("Software Imperial").join("Foundry32").join("daily").join("days")
}

pub fn day_path(dir: &Path, date: Date) -> PathBuf {
    dir.join(format!("{}.json", date.iso()))
}

/// A day as read from disk, plus why it came back empty when it did.
///
/// A missing file is an ordinary empty day. A file that is there and cannot be
/// used is something else: it holds entries the user wrote, and treating it as
/// empty would let the next save replace them. `damage` says so, and `edit`
/// refuses to write over it.
pub struct LoadedDay {
    pub day: Day,
    /// `Some(reason)` when a day file exists but could not be read or parsed —
    /// the path and the underlying error, for each front-end to phrase.
    pub damage: Option<String>,
}

pub fn load_day(dir: &Path, date: Date) -> LoadedDay {
    let path = day_path(dir, date);
    let empty = || Day { date: date.iso(), entries: Vec::new() };
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return LoadedDay { day: empty(), damage: None }
        }
        Err(e) => return LoadedDay { day: empty(), damage: Some(format!("{}: {e}", path.display())) },
    };
    // A map, not a bare `Value`: valid JSON that is not an object is still not
    // a day, and serde says why in its own words.
    match serde_json::from_str::<serde_json::Map<String, Value>>(text.trim_start_matches('\u{feff}')) {
        Ok(map) => {
            let value = Value::Object(map);
            let entries = value["entries"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .enumerate()
                        .map(|(index, item)| entry_from_json(item, date, index))
                        .collect()
                })
                .unwrap_or_default();
            // The filename is the identity, not the JSON's own `date`: a file
            // copied to another day's name belongs to that day now.
            LoadedDay { day: Day { date: date.iso(), entries }, damage: None }
        }
        Err(e) => LoadedDay { day: empty(), damage: Some(format!("{}: {e}", path.display())) },
    }
}

/// Writes a day as pretty-printed JSON, beside the target first and then
/// renamed over it — a crash mid-write leaves the previous good file, never a
/// half one. Same staging as `jafiz::store::save_runs`.
pub fn save_day(dir: &Path, date: Date, day: &Day) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    let path = day_path(dir, date);
    let body = serde_json::to_string_pretty(&json!({
        "schema": 1,
        "date": date.iso(),
        "entries": day.entries.iter().map(entry_to_json).collect::<Vec<_>>(),
    }))
    .map_err(|e| e.to_string())?;
    let tmp = dir.join(format!(".save.{}.json.tmp", date.iso()));
    std::fs::write(&tmp, body).map_err(|e| format!("write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("write {}: {e}", path.display())
    })
}

/// When a day file last changed, or `None` when it does not exist. The window
/// polls this to notice what the CLI wrote.
pub fn modified_at(dir: &Path, date: Date) -> Option<SystemTime> {
    std::fs::metadata(day_path(dir, date)).and_then(|meta| meta.modified()).ok()
}

/// Every day that has a file, oldest first. Files whose name is not a date
/// are not days and are skipped.
pub fn list_days(dir: &Path) -> Vec<Date> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut days: Vec<Date> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            name.strip_suffix(".json").and_then(Date::parse)
        })
        .collect();
    days.sort();
    days
}

/// The last day before `before` that has at least one entry — "yesterday" as
/// a stand-up means it: on a Monday, that is Friday.
pub fn previous_day_with_entries(dir: &Path, before: Date) -> Option<Date> {
    list_days(dir)
        .into_iter()
        .rev()
        .filter(|date| *date < before)
        .find(|date| !load_day(dir, *date).day.entries.is_empty())
}

/// Task labels used from `days` days before `today` up to `today`, one per
/// task (`group_key`), most recently used first. Each task is named by the
/// longest label it was given — `ABC-123 · login fix` over a bare `ABC-123` —
/// which is the one worth reusing.
pub fn recent_tasks(dir: &Path, today: Date, days: i64) -> Vec<String> {
    let since = today.shift(-days.max(0));
    let mut tasks: Vec<(String, String)> = Vec::new(); // (group key, label)
    for date in list_days(dir).into_iter().rev().filter(|date| *date >= since && *date <= today) {
        let day = load_day(dir, date).day;
        for entry in day.sorted().into_iter().rev() {
            let label = entry.task.trim();
            if label.is_empty() {
                continue;
            }
            let key = group_key(label);
            match tasks.iter_mut().find(|(known, _)| *known == key) {
                Some((_, known_label)) if label.chars().count() > known_label.chars().count() => {
                    *known_label = label.to_string();
                }
                Some(_) => {}
                None => tasks.push((key, label.to_string())),
            }
        }
    }
    tasks.into_iter().map(|(_, label)| label).collect()
}

/// The user's own latest entries — written or last edited in the window —
/// newest first, up to `limit`. What `daily style` shows Claude.
pub fn recent_user_entries(dir: &Path, limit: usize) -> Vec<(Date, Entry)> {
    let mut found = Vec::new();
    for date in list_days(dir).into_iter().rev() {
        let day = load_day(dir, date).day;
        for entry in day.sorted().into_iter().rev() {
            if entry.author == Author::User {
                found.push((date, entry.clone()));
                if found.len() >= limit {
                    return found;
                }
            }
        }
    }
    found
}

fn entry_to_json(entry: &Entry) -> Value {
    json!({
        "id": entry.id,
        "time": entry.time,
        "task": entry.task,
        "text": entry.text,
        "description": entry.description,
        "author": entry.author.code(),
        "created_at": entry.created_at,
        "updated_at": entry.updated_at,
    })
}

/// An entry written by hand without an id still gets one — `<day>-x<position>`
/// — rather than being dropped: dropping it here would delete it from disk on
/// the next save, and an entry with no id could never be edited or removed.
fn entry_from_json(value: &Value, date: Date, index: usize) -> Entry {
    let text_of = |key: &str| value[key].as_str().unwrap_or_default().to_string();
    let id = Some(text_of("id"))
        .filter(|id| !id.trim().is_empty())
        .unwrap_or_else(|| format!("{}-x{}", date.compact(), index + 1));
    Entry {
        id,
        time: text_of("time"),
        task: text_of("task"),
        text: text_of("text"),
        // Missing in files written before descriptions existed: none.
        description: text_of("description"),
        author: value["author"].as_str().and_then(Author::from_code).unwrap_or(Author::User),
        created_at: text_of("created_at"),
        updated_at: text_of("updated_at"),
    }
}

#[cfg(test)]
mod tests {
    use super::{entry_from_json, entry_to_json};
    use crate::clock::Date;
    use serde_json::json;

    #[test]
    fn a_day_written_before_descriptions_loads_with_none() {
        let date = Date::parse("2026-11-03").unwrap();
        let old = json!({ "id": "20261103-090000", "time": "09:00", "task": "", "text": "x", "author": "user" });
        assert_eq!(entry_from_json(&old, date, 0).description, "");
    }

    #[test]
    fn description_survives_a_save_and_load() {
        let date = Date::parse("2026-11-04").unwrap();
        let old = json!({ "id": "20261104-090000", "text": "x", "description": "a\n\nb" });
        let entry = entry_from_json(&old, date, 0);
        assert_eq!(entry_from_json(&entry_to_json(&entry), date, 0).description, "a\n\nb");
    }
}
