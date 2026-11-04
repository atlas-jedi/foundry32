//! What a day is made of: entries, who wrote them, and how they fall into
//! tasks. Pure data and pure functions — nothing here touches the disk.

/// Who wrote an entry's current text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Author {
    /// Typed (or last edited) in the window — the user's own words, which is
    /// what makes an entry a style example for `daily style`.
    User,
    /// Written through the CLI, normally by Claude.
    Claude,
}

impl Author {
    pub fn code(&self) -> &'static str {
        match self {
            Author::User => "user",
            Author::Claude => "claude",
        }
    }

    pub fn from_code(code: &str) -> Option<Author> {
        match code.trim().to_ascii_lowercase().as_str() {
            "user" | "voce" | "você" | "you" => Some(Author::User),
            "claude" => Some(Author::Claude),
            _ => None,
        }
    }
}

/// One thing the user did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// `<day YYYYMMDD>-<HHMMSS of creation>[-n]`. The prefix is always the day
    /// the entry belongs to, so an id alone is enough to find its file.
    pub id: String,
    /// `HH:MM` — when it was done, as the user tells it (defaults to when it
    /// was written down).
    pub time: String,
    /// Free task label; empty means "no task" (the "Others" group).
    pub task: String,
    /// One line of text.
    pub text: String,
    pub author: Author,
    pub created_at: String,
    pub updated_at: String,
}

/// Every entry of one calendar day.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Day {
    /// `2026-11-04`.
    pub date: String,
    pub entries: Vec<Entry>,
}

impl Day {
    /// The entries in the order a day is read: by time, then by when each was
    /// written down (ids sort chronologically within a day).
    pub fn sorted(&self) -> Vec<&Entry> {
        let mut entries: Vec<&Entry> = self.entries.iter().collect();
        entries.sort_by(|a, b| a.time.cmp(&b.time).then_with(|| a.id.cmp(&b.id)));
        entries
    }

    pub fn entry(&self, id: &str) -> Option<&Entry> {
        self.entries.iter().find(|entry| entry.id == id)
    }
}

/// The entries of one task, in reading order.
pub struct Group<'a> {
    /// `group_key` of every entry in it; empty for the "Others" group.
    pub key: String,
    /// What the group is called: the longest label used for it that day, so
    /// `ABC-123` and `ABC-123 · login fix` come out as the latter.
    pub label: String,
    pub entries: Vec<&'a Entry>,
}

/// A leading issue key — `LETTERS-DIGITS`, as in `ABC-123` — when the label
/// starts with one followed by nothing or a non-alphanumeric character.
pub fn issue_key(task: &str) -> Option<String> {
    let task = task.trim();
    let letters = task.chars().take_while(|c| c.is_ascii_alphabetic()).count();
    if letters == 0 || !task[letters..].starts_with('-') {
        return None;
    }
    let rest = &task[letters + 1..];
    let digits = rest.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 {
        return None;
    }
    let after = rest[digits..].chars().next();
    if after.is_some_and(|c| c.is_alphanumeric()) {
        return None;
    }
    Some(task[..letters + 1 + digits].to_ascii_uppercase())
}

/// What decides which group an entry falls in. A label that starts with an
/// issue key groups by the key, so wording the rest of the label differently
/// does not split a task in two; any other label groups by itself, ignoring
/// case and surrounding spaces. No label groups as "" — the "Others" group.
pub fn group_key(task: &str) -> String {
    issue_key(task).unwrap_or_else(|| task.trim().to_lowercase())
}

/// The day grouped by task: groups in the order their first entry happened,
/// entries in time order inside each, "Others" last.
pub fn group_by_task(day: &Day) -> Vec<Group<'_>> {
    let mut groups: Vec<Group> = Vec::new();
    for entry in day.sorted() {
        let key = group_key(&entry.task);
        let label = entry.task.trim();
        match groups.iter_mut().find(|group| group.key == key) {
            Some(group) => {
                if label.chars().count() > group.label.chars().count() {
                    group.label = label.to_string();
                }
                group.entries.push(entry);
            }
            None => groups.push(Group { key, label: label.to_string(), entries: vec![entry] }),
        }
    }
    // Stable: every task keeps its order, only "Others" moves to the end.
    groups.sort_by_key(|group| group.key.is_empty());
    groups
}

/// Counts for a status line: entries, distinct tasks (not counting "Others"),
/// and how many entries Claude wrote.
pub struct Tally {
    pub entries: usize,
    pub tasks: usize,
    pub by_claude: usize,
}

pub fn tally(day: &Day) -> Tally {
    let groups = group_by_task(day);
    Tally {
        entries: day.entries.len(),
        tasks: groups.iter().filter(|group| !group.key.is_empty()).count(),
        by_claude: day.entries.iter().filter(|entry| entry.author == Author::Claude).count(),
    }
}

/// Folds typed text to the single line an entry is: line breaks become
/// spaces, runs of blanks collapse, the ends are trimmed.
pub fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}
