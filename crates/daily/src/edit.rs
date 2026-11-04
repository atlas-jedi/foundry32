//! The only way a day changes. Every operation is one short cycle — read the
//! day back from disk, apply exactly one change, save it atomically — and never
//! starts from a copy held in memory. That is what lets the window and the CLI
//! write the same day at once: each change lands on top of the other's, instead
//! of one saving an old copy over the other's entry.

use crate::clock::{self, Date};
use crate::model::{one_line, tidy_description, Author, Day, Entry};
use crate::store;
use foundry_common::lang::Lang;
use std::path::Path;

/// Why an edit did not happen.
#[derive(Debug)]
pub enum EditError {
    /// The day file is there but unreadable; writing would replace what it holds.
    Damaged(String),
    /// No entry with this id on its day.
    NotFound(String),
    /// The id does not start with a day (`YYYYMMDD-`), so there is no file to look in.
    BadId(String),
    /// The entry would have no text.
    EmptyText,
    /// The time is not `HH:MM`.
    BadTime(String),
    /// Saving failed.
    Io(String),
}

impl EditError {
    pub fn message(&self, lang: Lang) -> String {
        match (self, lang) {
            (EditError::Damaged(reason), Lang::PtBr) => {
                format!("o arquivo do dia está danificado e não foi alterado ({reason})")
            }
            (EditError::Damaged(reason), Lang::En) => {
                format!("the day file is damaged and was left untouched ({reason})")
            }
            (EditError::NotFound(id), Lang::PtBr) => format!("nenhuma anotação com o id {id}"),
            (EditError::NotFound(id), Lang::En) => format!("no entry with id {id}"),
            (EditError::BadId(id), Lang::PtBr) => {
                format!("id inválido: {id} (esperado AAAAMMDD-HHMMSS, veja daily list)")
            }
            (EditError::BadId(id), Lang::En) => {
                format!("invalid id: {id} (expected YYYYMMDD-HHMMSS, see daily list)")
            }
            (EditError::EmptyText, Lang::PtBr) => "a anotação ficaria sem texto".into(),
            (EditError::EmptyText, Lang::En) => "the entry would have no text".into(),
            (EditError::BadTime(time), Lang::PtBr) => format!("hora inválida: {time} (use HH:MM)"),
            (EditError::BadTime(time), Lang::En) => format!("invalid time: {time} (use HH:MM)"),
            (EditError::Io(reason), _) => reason.clone(),
        }
    }
}

/// A new entry, before it has an id.
pub struct NewEntry {
    pub date: Date,
    /// `None` = now.
    pub time: Option<String>,
    pub task: String,
    pub text: String,
    /// Empty = no description.
    pub description: String,
    pub author: Author,
}

/// What to change on an existing entry. `None` fields are left as they are;
/// `author` is always written, because whoever edits the text now owns it.
pub struct EntryChange {
    pub text: Option<String>,
    pub task: Option<String>,
    pub time: Option<String>,
    /// `Some("")` clears the description.
    pub description: Option<String>,
    pub author: Author,
}

/// The day an id was filed under: the id's first eight digits.
pub fn date_of_id(id: &str) -> Option<Date> {
    let prefix = id.get(..8)?;
    if !prefix.chars().all(|c| c.is_ascii_digit()) || id.as_bytes().get(8) != Some(&b'-') {
        return None;
    }
    Date::parse(&format!("{}-{}-{}", &prefix[..4], &prefix[4..6], &prefix[6..8]))
}

pub fn add_entry(dir: &Path, new: NewEntry) -> Result<Entry, EditError> {
    let text = one_line(&new.text);
    if text.is_empty() {
        return Err(EditError::EmptyText);
    }
    let time = match new.time.as_deref() {
        Some(raw) => clock::parse_hm(raw).ok_or_else(|| EditError::BadTime(raw.to_string()))?,
        None => clock::now_hm(),
    };
    let mut day = day_for_write(dir, new.date)?;
    let now = clock::timestamp();
    let entry = Entry {
        id: unique_id(&day, new.date),
        time,
        task: one_line(&new.task),
        text,
        description: tidy_description(&new.description),
        author: new.author,
        created_at: now.clone(),
        updated_at: now,
    };
    day.entries.push(entry.clone());
    store::save_day(dir, new.date, &day).map_err(EditError::Io)?;
    Ok(entry)
}

pub fn update_entry(dir: &Path, id: &str, change: EntryChange) -> Result<Entry, EditError> {
    let date = date_of_id(id).ok_or_else(|| EditError::BadId(id.to_string()))?;
    let time = match change.time.as_deref() {
        Some(raw) => Some(clock::parse_hm(raw).ok_or_else(|| EditError::BadTime(raw.to_string()))?),
        None => None,
    };
    let text = change.text.as_deref().map(one_line);
    if text.as_deref() == Some("") {
        return Err(EditError::EmptyText);
    }
    let mut day = day_for_write(dir, date)?;
    let entry = day
        .entries
        .iter_mut()
        .find(|entry| entry.id == id)
        .ok_or_else(|| EditError::NotFound(id.to_string()))?;
    if let Some(text) = text {
        entry.text = text;
    }
    if let Some(task) = change.task.as_deref() {
        entry.task = one_line(task);
    }
    if let Some(time) = time {
        entry.time = time;
    }
    if let Some(description) = change.description.as_deref() {
        entry.description = tidy_description(description);
    }
    entry.author = change.author;
    entry.updated_at = clock::timestamp();
    let updated = entry.clone();
    store::save_day(dir, date, &day).map_err(EditError::Io)?;
    Ok(updated)
}

pub fn remove_entry(dir: &Path, id: &str) -> Result<Entry, EditError> {
    let date = date_of_id(id).ok_or_else(|| EditError::BadId(id.to_string()))?;
    let mut day = day_for_write(dir, date)?;
    let position = day
        .entries
        .iter()
        .position(|entry| entry.id == id)
        .ok_or_else(|| EditError::NotFound(id.to_string()))?;
    let removed = day.entries.remove(position);
    store::save_day(dir, date, &day).map_err(EditError::Io)?;
    Ok(removed)
}

/// The day exactly as it is on disk right now — refused when the file is there
/// but unreadable, because saving would replace whatever it still holds.
fn day_for_write(dir: &Path, date: Date) -> Result<Day, EditError> {
    let loaded = store::load_day(dir, date);
    match loaded.damage {
        Some(reason) => Err(EditError::Damaged(reason)),
        None => Ok(loaded.day),
    }
}

/// `<day>-<HHMMSS now>`, with `-2`, `-3`… when two entries land in the same
/// second. The day prefix is the entry's day, not today: an entry added to
/// yesterday is filed under yesterday, and `date_of_id` finds it there.
fn unique_id(day: &Day, date: Date) -> String {
    let base = format!("{}-{}", date.compact(), clock::now_hms_compact());
    if day.entry(&base).is_none() {
        return base;
    }
    (2..)
        .map(|n| format!("{base}-{n}"))
        .find(|candidate| day.entry(candidate).is_none())
        .unwrap_or(base)
}
