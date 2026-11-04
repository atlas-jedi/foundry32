//! `daily-gui.exe` — the GUI front-end (GUI subsystem, launched by the
//! Foundry32 hub). Windows Classic chrome over the same engine the CLI uses.
//!
//! One day on screen at a time: a form to write down what was just done, and
//! the day's entries in a list under it. Every write goes through the engine's
//! `edit`, which reads the day back from disk first — so what Claude writes
//! through the CLI while the window is open is never written over. A watcher
//! thread notices when the day file changes and the list reloads on its own.
//!
//! The modal dialogs follow JAFIZ's pattern: their own thread, and the answer
//! comes back through `Shared` plus a `Notice`.
//!
//! This module is behaviour only. How the window is built, laid out and
//! captioned lives in `chrome`.

#![windows_subsystem = "windows"]

mod chrome;
mod entry_dialog;
mod i18n;
mod preferences_dialog;

use chrome::{COL_AUTHOR, COL_TASK, COL_TEXT, COL_TIME};
use i18n::{t, Lang, T};

use native_windows_gui as nwg;
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use daily::edit::{self, EntryChange, NewEntry};
use daily::model::{group_key, one_line, tally};
use daily::settings::AppSettings;
use daily::{report, store, Author, Date, Day, Entry};
use foundry_common::ui::{popup_menu, select_only, ensure_visible, set_list_view_sort_indicator, MenuEntry};

const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

const VK_F5: u32 = 0x74;
const VK_DELETE: u32 = 0x2E;

/// How often the watcher looks at the day file. Short enough that an entry
/// Claude adds shows up while the user is still looking.
const WATCH_INTERVAL: Duration = Duration::from_secs(2);

/// How many recent tasks the ▾ menu offers.
const RECENT_MENU_MAX: usize = 15;
const RECENT_TASK_DAYS: i64 = 14;

/// Mailbox the modal dialog threads fill; drained on the UI thread when the
/// Notice fires. Every slot spells "cancelled" as `Some(None)`.
#[derive(Default)]
pub(crate) struct Shared {
    pub edit: Option<Option<entry_dialog::EntryEdit>>,
    pub preferences: Option<Option<Lang>>,
}

/// Which `Shared` slot a dialog answers through.
#[derive(Clone, Copy)]
pub(crate) enum DialogSlot {
    Edit,
    Preferences,
}

impl DialogSlot {
    fn cancel(self, shared: &mut Shared) {
        match self {
            DialogSlot::Edit => shared.edit = Some(None),
            DialogSlot::Preferences => shared.preferences = Some(None),
        }
    }
}

/// Last-resort notifier for a dialog thread that unwinds before it answers —
/// without it, the main window, disabled while the dialog is up, would stay
/// disabled for good. Same guard as JAFIZ's; see its comment there.
pub(crate) struct DialogGuard {
    sent: Rc<Cell<bool>>,
    slot: DialogSlot,
    shared: Arc<Mutex<Shared>>,
    notify: nwg::NoticeSender,
}

impl DialogGuard {
    pub(crate) fn new(
        sent: Rc<Cell<bool>>,
        slot: DialogSlot,
        shared: Arc<Mutex<Shared>>,
        notify: nwg::NoticeSender,
    ) -> DialogGuard {
        DialogGuard { sent, slot, shared, notify }
    }
}

impl Drop for DialogGuard {
    fn drop(&mut self) {
        if self.sent.replace(true) {
            return; // the dialog answered for itself
        }
        let mut shared = match self.shared.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        self.slot.cancel(&mut shared);
        drop(shared);
        self.notify.notice();
    }
}

/// What the watcher thread looks at: the file of the day on screen, and when
/// it last saw it change.
pub(crate) struct WatchTarget {
    pub dir: PathBuf,
    pub date: Date,
    pub seen: Option<SystemTime>,
}

pub(crate) struct UiState {
    lang: Lang,
    settings: AppSettings,
    dir: PathBuf,
    /// The day on screen.
    date: Date,
    /// That day, as last read from disk. Only ever read from: every write goes
    /// through `edit`, which reads the file again itself.
    day: Day,
    /// Why the day file could not be used, when it is there and damaged.
    damage: Option<String>,
    /// The entry id behind each list row, in row order.
    rows: Vec<String>,
    /// The column the list is ordered by and whether descending; `None` is
    /// time order.
    sort: Option<(usize, bool)>,
    /// What the ▾ menu offers.
    recent_tasks: Vec<String>,
    /// The entry the open edit dialog is about, captured when it was opened.
    edit_target: Option<String>,
}

pub(crate) struct DailyApp {
    window: nwg::Window,
    btn_prev: nwg::Button,
    date_label: nwg::Label,
    /// Held so the label's font outlives the label (nwg does not own it).
    #[allow(dead_code)]
    date_font: nwg::Font,
    btn_next: nwg::Button,
    btn_today: nwg::Button,
    task_label: nwg::Label,
    task_input: nwg::TextInput,
    btn_recent: nwg::Button,
    text_label: nwg::Label,
    text_input: nwg::TextInput,
    btn_add: nwg::Button,
    list: nwg::ListView,
    btn_edit: nwg::Button,
    btn_delete: nwg::Button,
    btn_copy: nwg::Button,
    status_bar: nwg::StatusBar,
    /// Wakes the UI thread when a dialog answered.
    notice: nwg::Notice,
    /// Wakes the UI thread when the day file changed on disk.
    watch_notice: nwg::Notice,
    menu_open_folder: nwg::MenuItem,
    menu_reload: nwg::MenuItem,
    menu_exit: nwg::MenuItem,
    menu_copy: nwg::MenuItem,
    menu_prefs: nwg::MenuItem,
    menu_about: nwg::MenuItem,
    // Top-level menus, declared after the items on purpose: fields drop in
    // declaration order, and an item's `Drop` needs its parent menu alive.
    // They must be fields at all because `nwg::Menu`'s `Drop` destroys the menu.
    menu_file: nwg::Menu,
    menu_report: nwg::Menu,
    menu_tools: nwg::Menu,
    menu_help: nwg::Menu,
    watch: Arc<Mutex<WatchTarget>>,
    state: RefCell<UiState>,
    shared: Arc<Mutex<Shared>>,
}

fn main() {
    nwg::init().expect("failed to init native-windows-gui");
    let _ = nwg::Font::set_global_family("Segoe UI");

    let app = Rc::new(chrome::build_app(AppSettings::load()));
    wire_events(&app);
    spawn_watcher(Arc::clone(&app.watch), app.watch_notice.sender());
    app.layout();
    app.reload();
    app.text_input.set_focus();
    nwg::dispatch_thread_events();
}

fn wire_events(app: &Rc<DailyApp>) {
    let weak = Rc::downgrade(app);
    let handler = nwg::full_bind_event_handler(&app.window.handle, move |evt, evt_data, handle| {
        let Some(app) = weak.upgrade() else { return };
        use nwg::Event as E;
        match evt {
            E::OnWindowClose if handle == app.window.handle => nwg::stop_thread_dispatch(),
            E::OnResize | E::OnResizeEnd | E::OnWindowMaximize if handle == app.window.handle => app.layout(),
            E::OnButtonClick if handle == app.btn_prev.handle => app.step_day(-1),
            E::OnButtonClick if handle == app.btn_next.handle => app.step_day(1),
            E::OnButtonClick if handle == app.btn_today.handle => app.show_day(Date::today()),
            E::OnButtonClick if handle == app.btn_recent.handle => app.pick_recent_task(),
            E::OnButtonClick if handle == app.btn_add.handle => app.add_from_form(),
            E::OnButtonClick if handle == app.btn_edit.handle => app.edit_selected(),
            E::OnButtonClick if handle == app.btn_delete.handle => app.delete_selected(),
            E::OnButtonClick if handle == app.btn_copy.handle => app.copy_report(),
            E::OnMenuItemSelected => {
                if handle == app.menu_open_folder.handle {
                    app.open_folder();
                } else if handle == app.menu_reload.handle {
                    app.reload();
                } else if handle == app.menu_exit.handle {
                    nwg::stop_thread_dispatch();
                } else if handle == app.menu_copy.handle {
                    app.copy_report();
                } else if handle == app.menu_prefs.handle {
                    app.open_preferences();
                } else if handle == app.menu_about.handle {
                    app.show_about();
                }
            }
            // nwg's message loop runs IsDialogMessage, which turns Enter into
            // IDOK on the window — so Enter arrives here, whichever control has
            // the focus. In the form it adds; on the list it edits.
            E::OnKeyEnter => {
                if has_focus(&app.text_input.handle) || has_focus(&app.task_input.handle) {
                    app.add_from_form();
                } else if has_focus(&app.list.handle) {
                    app.edit_selected();
                }
            }
            E::OnKeyRelease => match evt_data {
                nwg::EventData::OnKey(VK_F5) => app.reload(),
                nwg::EventData::OnKey(VK_DELETE) if has_focus(&app.list.handle) => app.delete_selected(),
                _ => {}
            },
            E::OnListViewItemActivated if handle == app.list.handle => app.edit_selected(),
            E::OnListViewColumnClick if handle == app.list.handle => {
                if let nwg::EventData::OnListViewItemIndex { column_index, .. } = evt_data {
                    app.cycle_sort(column_index);
                }
            }
            E::OnListViewItemChanged | E::OnListViewClick if handle == app.list.handle => app.update_buttons(),
            E::OnNotice if handle == app.notice.handle => app.drain_dialog(),
            E::OnNotice if handle == app.watch_notice.handle => app.reload(),
            _ => {}
        }
    });
    std::mem::forget(handler); // lives for the whole process (single window)
}

/// Polls the modification time of the day on screen and wakes the window when
/// it changes. Runs for as long as the process does.
fn spawn_watcher(target: Arc<Mutex<WatchTarget>>, sender: nwg::NoticeSender) {
    std::thread::spawn(move || loop {
        std::thread::sleep(WATCH_INTERVAL);
        let changed = {
            let mut target = match target.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            let now = store::modified_at(&target.dir, target.date);
            let changed = now != target.seen;
            target.seen = now;
            changed
        };
        if changed {
            sender.notice();
        }
    });
}

fn has_focus(handle: &nwg::ControlHandle) -> bool {
    use winapi::um::winuser::GetFocus;
    // SAFETY: GetFocus takes no arguments and returns this thread's focus HWND.
    handle.hwnd().is_some_and(|hwnd| unsafe { GetFocus() } == hwnd)
}

/// Line breaks the way Windows' clipboard expects them, so the text pastes as
/// separate lines into every app, old editors included.
fn crlf(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\n', "\r\n")
}

/// The entries in the order the list shows them: by the chosen column, or by
/// time when none is chosen. Ties fall back to time order.
fn ordered(day: &Day, sort: Option<(usize, bool)>) -> Vec<&Entry> {
    let mut entries = day.sorted();
    let Some((column, descending)) = sort else { return entries };
    entries.sort_by(|a, b| {
        let order = match column {
            COL_TASK => group_key(&a.task).cmp(&group_key(&b.task)),
            COL_TEXT => a.text.to_lowercase().cmp(&b.text.to_lowercase()),
            COL_AUTHOR => a.author.code().cmp(b.author.code()),
            _ => std::cmp::Ordering::Equal,
        };
        let order = order.then_with(|| a.time.cmp(&b.time)).then_with(|| a.id.cmp(&b.id));
        if descending {
            order.reverse()
        } else {
            order
        }
    });
    entries
}

impl DailyApp {
    fn tr(&self) -> &'static T {
        t(self.state.borrow().lang)
    }

    fn show_error(&self, message: &str) {
        nwg::modal_error_message(&self.window.handle, self.tr().error_title, message);
    }

    /// Puts another day on screen.
    fn show_day(&self, date: Date) {
        {
            let mut state = self.state.borrow_mut();
            if state.date != date {
                state.date = date;
                state.sort = None;
            }
        }
        self.reload();
    }

    fn step_day(&self, days: i64) {
        let date = self.state.borrow().date.shift(days);
        self.show_day(date);
    }

    /// Reads the day on screen back from disk and redraws everything that
    /// depends on it, keeping the selected entry selected.
    fn reload(&self) {
        let keep = self.selected_id();
        let (dir, date) = {
            let state = self.state.borrow();
            (state.dir.clone(), state.date)
        };
        let loaded = store::load_day(&dir, date);
        let recent = store::recent_tasks(&dir, Date::today(), RECENT_TASK_DAYS);
        {
            let mut state = self.state.borrow_mut();
            state.day = loaded.day;
            state.damage = loaded.damage;
            state.recent_tasks = recent;
        }
        // Tell the watcher what is on screen now, so it neither misses the
        // next change nor reports the one just read.
        if let Ok(mut watch) = self.watch.lock() {
            watch.dir = dir.clone();
            watch.date = date;
            watch.seen = store::modified_at(&dir, date);
        }
        self.set_date_label();
        self.populate_list(keep.as_deref());
        self.update_status();
        self.update_buttons();
    }

    fn set_date_label(&self) {
        let state = self.state.borrow();
        let tr = t(state.lang);
        let mut text = state.date.display(state.lang);
        if state.date == Date::today() {
            text.push_str(tr.today_suffix);
        }
        self.date_label.set_text(&text);
    }

    fn populate_list(&self, keep: Option<&str>) {
        let rows: Vec<String> = {
            let state = self.state.borrow();
            let entries = ordered(&state.day, state.sort);
            self.list.set_redraw(false);
            self.list.clear();
            for (row, entry) in entries.iter().enumerate() {
                self.list.insert_items_row(
                    Some(row as i32),
                    &[
                        entry.time.as_str(),
                        entry.task.as_str(),
                        entry.text.as_str(),
                        report::author_label(entry.author, state.lang),
                    ],
                );
            }
            self.list.set_redraw(true);
            entries.iter().map(|entry| entry.id.clone()).collect()
        };
        let keep_row = keep.and_then(|id| rows.iter().position(|row_id| row_id == id));
        self.state.borrow_mut().rows = rows;
        if let Some(row) = keep_row {
            self.select_row(row);
        }
    }

    fn select_row(&self, row: usize) {
        select_only(&self.list, row);
        ensure_visible(&self.list, row);
    }

    fn selected_id(&self) -> Option<String> {
        let row = self.list.selected_item()?;
        self.state.borrow().rows.get(row).cloned()
    }

    fn update_status(&self) {
        let state = self.state.borrow();
        let text = match state.damage.as_deref() {
            Some(reason) => report::damaged_line(reason, state.lang),
            None => {
                let counts = tally(&state.day);
                let mut parts = vec![report::entry_count(counts.entries, state.lang)];
                if counts.tasks > 0 {
                    parts.push(i18n::tasks_count(counts.tasks));
                }
                if counts.by_claude > 0 {
                    parts.push(i18n::by_claude(counts.by_claude, state.lang));
                }
                parts.join(" · ")
            }
        };
        self.status_bar.set_text(0, &format!("{text}    —    DAILY {CURRENT_VERSION}"));
    }

    fn update_buttons(&self) {
        let selected = self.list.selected_item().is_some();
        self.btn_edit.set_enabled(selected);
        self.btn_delete.set_enabled(selected);
    }

    /// Writes down what is in the form. The text field empties for the next
    /// entry; the task stays, since the next thing done is often the same task.
    fn add_from_form(&self) {
        let text = self.text_input.text();
        if text.trim().is_empty() {
            self.status_bar.set_text(0, self.tr().empty_text);
            self.text_input.set_focus();
            return;
        }
        let (dir, date, lang) = {
            let state = self.state.borrow();
            (state.dir.clone(), state.date, state.lang)
        };
        let new = NewEntry { date, time: None, task: self.task_input.text(), text, author: Author::User };
        match edit::add_entry(&dir, new) {
            Ok(entry) => {
                self.text_input.set_text("");
                self.text_input.set_focus();
                self.reload();
                self.select_id(&entry.id);
            }
            Err(error) => self.show_error(&error.message(lang)),
        }
    }

    fn select_id(&self, id: &str) {
        let row = self.state.borrow().rows.iter().position(|row_id| row_id == id);
        if let Some(row) = row {
            self.select_row(row);
            self.update_buttons();
        }
    }

    /// The ▾ next to the task field: the tasks of the last days, newest first.
    fn pick_recent_task(&self) {
        let (labels, tr) = {
            let state = self.state.borrow();
            let labels: Vec<String> = state.recent_tasks.iter().take(RECENT_MENU_MAX).cloned().collect();
            (labels, t(state.lang))
        };
        let entries: Vec<MenuEntry> = if labels.is_empty() {
            vec![MenuEntry::Item { id: 1, text: tr.no_recent_tasks, enabled: false, default: false }]
        } else {
            labels
                .iter()
                .enumerate()
                .map(|(index, label)| MenuEntry::Item { id: index + 1, text: label, enabled: true, default: false })
                .collect()
        };
        let Some(choice) = popup_menu(&self.window.handle, &entries) else { return };
        if let Some(label) = labels.get(choice - 1) {
            self.task_input.set_text(label);
            self.text_input.set_focus();
        }
    }

    fn selected_entry(&self) -> Option<Entry> {
        let id = self.selected_id()?;
        self.state.borrow().day.entry(&id).cloned()
    }

    fn edit_selected(&self) {
        let Some(entry) = self.selected_entry() else { return };
        let lang = self.state.borrow().lang;
        self.state.borrow_mut().edit_target = Some(entry.id.clone());
        self.window.set_enabled(false);
        entry_dialog::spawn(entry_dialog::EntryParams {
            lang,
            task: entry.task,
            text: entry.text,
            time: entry.time,
            shared: Arc::clone(&self.shared),
            notify: self.notice.sender(),
        });
    }

    /// Saves what the edit dialog came back with. The entry becomes the user's
    /// own: whatever its text was, it now reads the way they left it. An OK
    /// that changed nothing saves nothing — otherwise glancing at one of
    /// Claude's entries would turn it into a style example of the user's.
    fn apply_edit(&self, answer: entry_dialog::EntryEdit) {
        let Some(id) = self.state.borrow_mut().edit_target.take() else { return };
        let (dir, lang, unchanged) = {
            let state = self.state.borrow();
            let unchanged = state.day.entry(&id).is_some_and(|entry| {
                entry.text == one_line(&answer.text)
                    && entry.task == one_line(&answer.task)
                    && entry.time == answer.time.trim()
            });
            (state.dir.clone(), state.lang, unchanged)
        };
        if unchanged {
            return;
        }
        let change = EntryChange {
            text: Some(answer.text),
            task: Some(answer.task),
            time: Some(answer.time),
            author: Author::User,
        };
        match edit::update_entry(&dir, &id, change) {
            Ok(_) => {
                self.reload();
                self.select_id(&id);
            }
            Err(error) => self.show_error(&error.message(lang)),
        }
    }

    fn delete_selected(&self) {
        let Some(entry) = self.selected_entry() else { return };
        let (dir, lang) = {
            let state = self.state.borrow();
            (state.dir.clone(), state.lang)
        };
        let tr = t(lang);
        let params = nwg::MessageParams {
            title: tr.delete_title,
            content: &tr.delete_body.replace("%S", &entry.text),
            buttons: nwg::MessageButtons::YesNo,
            icons: nwg::MessageIcons::Question,
        };
        if nwg::modal_message(&self.window.handle, &params) != nwg::MessageChoice::Yes {
            return;
        }
        match edit::remove_entry(&dir, &entry.id) {
            Ok(_) => self.reload(),
            Err(error) => self.show_error(&error.message(lang)),
        }
    }

    /// The day grouped by task, on the clipboard — the text the stand-up needs.
    fn copy_report(&self) {
        let (text, tr) = {
            let state = self.state.borrow();
            if state.day.entries.is_empty() {
                (None, t(state.lang))
            } else {
                (Some(report::render_day(&state.day, state.date, state.lang)), t(state.lang))
            }
        };
        match text {
            Some(text) => {
                nwg::Clipboard::set_data_text(&self.window, &crlf(&text));
                // Transient by design: the next reload puts the counts back.
                self.status_bar.set_text(0, tr.copied);
            }
            None => self.status_bar.set_text(0, tr.nothing_to_copy),
        }
    }

    /// Clicking a header cycles it: ascending, descending, back to time order.
    fn cycle_sort(&self, column: usize) {
        let next = match self.state.borrow().sort {
            _ if column == COL_TIME => None,
            Some((current, false)) if current == column => Some((column, true)),
            Some((current, true)) if current == column => None,
            _ => Some((column, false)),
        };
        self.state.borrow_mut().sort = next;
        set_list_view_sort_indicator(&self.list, next.map(|(column, descending)| (column as i32, descending)));
        let keep = self.selected_id();
        self.populate_list(keep.as_deref());
    }

    fn open_folder(&self) {
        let dir = self.state.borrow().dir.clone();
        if let Err(error) = std::fs::create_dir_all(&dir) {
            self.show_error(&format!("{}: {error}", dir.display()));
            return;
        }
        // Nothing is done with the exit status: explorer routinely returns
        // non-zero after opening the folder successfully.
        let _ = std::process::Command::new("explorer.exe").arg(&dir).spawn();
    }

    fn show_about(&self) {
        let tr = self.tr();
        let body = tr.about_body.replace("%V", CURRENT_VERSION);
        nwg::modal_info_message(&self.window.handle, tr.about_title, &body);
    }

    fn open_preferences(&self) {
        let lang = self.state.borrow().lang;
        self.window.set_enabled(false);
        preferences_dialog::spawn(preferences_dialog::PreferencesParams {
            lang,
            shared: Arc::clone(&self.shared),
            notify: self.notice.sender(),
        });
    }

    fn apply_language(&self, lang: Lang) {
        let changed = {
            let mut state = self.state.borrow_mut();
            let changed = state.lang != lang;
            state.lang = lang;
            state.settings.lang = lang;
            changed
        };
        if !changed {
            return;
        }
        // The CLI reads the same setting, so `daily report` in a terminal
        // comes back in the language just chosen here.
        let outcome = self.state.borrow().settings.save();
        if let Err(error) = outcome {
            self.show_error(&error);
        }
        self.relabel_all();
    }

    fn drain_dialog(&self) {
        // Unconditionally, first: a dialog that exits without answering would
        // otherwise leave a disabled window the user cannot even close.
        self.window.set_enabled(true);
        let (edit, preferences) = {
            let mut shared = self.shared.lock().unwrap();
            (shared.edit.take(), shared.preferences.take())
        };
        match edit {
            Some(Some(answer)) => self.apply_edit(answer),
            Some(None) => self.state.borrow_mut().edit_target = None,
            None => {}
        }
        if let Some(Some(lang)) = preferences {
            self.apply_language(lang);
        }
    }
}
