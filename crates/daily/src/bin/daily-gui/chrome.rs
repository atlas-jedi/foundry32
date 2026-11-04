//! The window itself: what it is made of, where the pieces sit, and what they
//! are called. Everything here is chrome — construction, geometry and captions
//! — so `main.rs` is left holding behaviour alone. Same split as JAFIZ's GUI.

use crate::i18n::{t, Lang};
use crate::{DailyApp, Shared, UiState, WatchTarget};

use foundry_common::theme::{apply_classic_button_theme, apply_explorer_theme};
use foundry_common::ui::{
    apply_window_icon, insert_report_list_view_column, set_menu_item_text, set_submenu_text,
};
use native_windows_gui as nwg;
use std::cell::RefCell;
use std::sync::{Arc, Mutex};

use daily::settings::AppSettings;
use daily::{store, Date, Day};

// List columns.
pub(crate) const COL_TIME: usize = 0;
pub(crate) const COL_TASK: usize = 1;
pub(crate) const COL_TEXT: usize = 2;
pub(crate) const COL_AUTHOR: usize = 3;

const COL_TIME_W: i32 = 56;
const COL_TASK_W: i32 = 230;
const COL_AUTHOR_W: i32 = 70;
/// Room the list's own border and vertical scroll bar take from its columns.
const LIST_CHROME_W: i32 = 26;
const COL_TEXT_MIN_W: i32 = 140;

const MARGIN: i32 = 8;
const ROW_H: i32 = 24;
const ROW_GAP: i32 = 6;
const LABEL_W: i32 = 78;
const ARROW_W: i32 = 32;
const DATE_W: i32 = 280;
const TODAY_W: i32 = 80;
const RECENT_W: i32 = 28;
const ADD_W: i32 = 100;
const BUTTON_W: i32 = 100;
const COPY_W: i32 = 180;
const BUTTON_H: i32 = 26;
const BUTTON_GAP: i32 = 6;
const STATUS_H: i32 = 24;

// Below this there is no room left to lay anything out: `layout` bails.
const MIN_W: i32 = 520;
const MIN_H: i32 = 300;

/// Builds every control the window owns and hands back the assembled app.
pub(crate) fn build_app(settings: AppSettings) -> DailyApp {
    let lang = settings.lang;
    let tr = t(lang);

    let mut window = nwg::Window::default();
    nwg::Window::builder()
        .size((900, 600))
        .position((180, 110))
        .title(tr.app_title)
        .flags(nwg::WindowFlags::MAIN_WINDOW | nwg::WindowFlags::VISIBLE | nwg::WindowFlags::RESIZABLE)
        .build(&mut window)
        .expect("window");

    let submenu = |text: &str, window: &nwg::Window| {
        let mut menu = nwg::Menu::default();
        nwg::Menu::builder().parent(window).text(text).build(&mut menu).expect("menu");
        menu
    };
    let item = |text: &str, parent: &nwg::Menu| {
        let mut menu_item = nwg::MenuItem::default();
        nwg::MenuItem::builder().parent(parent).text(text).build(&mut menu_item).expect("item");
        menu_item
    };
    let menu_file = submenu(tr.menu_file, &window);
    let menu_open_folder = item(tr.menu_open_folder, &menu_file);
    let menu_reload = item(tr.menu_reload, &menu_file);
    let menu_exit = item(tr.menu_exit, &menu_file);
    let menu_report = submenu(tr.menu_report, &window);
    let menu_copy = item(tr.menu_copy, &menu_report);
    let menu_tools = submenu(tr.menu_tools, &window);
    let menu_prefs = item(tr.menu_prefs, &menu_tools);
    let menu_help = submenu(tr.menu_help, &window);
    let menu_about = item(tr.menu_about, &menu_help);

    let button = |text: &str, parent: &nwg::Window| {
        let mut control = nwg::Button::default();
        nwg::Button::builder().parent(parent).text(text).build(&mut control).expect("button");
        apply_classic_button_theme(&control);
        control
    };
    let label = |text: &str, parent: &nwg::Window| {
        let mut control = nwg::Label::default();
        nwg::Label::builder().parent(parent).text(text).build(&mut control).expect("label");
        control
    };
    let input = |hint: &str, parent: &nwg::Window| {
        let mut control = nwg::TextInput::default();
        nwg::TextInput::builder()
            .parent(parent)
            .placeholder_text(Some(hint))
            .build(&mut control)
            .expect("input");
        control
    };

    // Day row: step back, the day on screen, step forward, back to today.
    let btn_prev = button("◄", &window);
    let mut date_font = nwg::Font::default();
    let _ = nwg::Font::builder().family("Segoe UI").size(18).weight(700).build(&mut date_font);
    let mut date_label = nwg::Label::default();
    nwg::Label::builder()
        .parent(&window)
        .text("")
        .h_align(nwg::HTextAlign::Center)
        .build(&mut date_label)
        .expect("date_label");
    date_label.set_font(Some(&date_font));
    let btn_next = button("►", &window);
    let btn_today = button(tr.btn_today, &window);

    // The form: which task, what was done.
    let task_label = label(tr.lbl_task, &window);
    let task_input = input(tr.task_hint, &window);
    let btn_recent = button("▾", &window);
    let text_label = label(tr.lbl_text, &window);
    let text_input = input(tr.text_hint, &window);
    let btn_add = button(tr.btn_add, &window);

    let mut list = nwg::ListView::default();
    nwg::ListView::builder()
        .parent(&window)
        .list_style(nwg::ListViewStyle::Detailed)
        .ex_flags(nwg::ListViewExFlags::FULL_ROW_SELECT | nwg::ListViewExFlags::GRID)
        .build(&mut list)
        .expect("list");
    // nwg forces LVS_NOCOLUMNHEADER at creation for backward compatibility.
    list.set_headers_enabled(true);
    apply_explorer_theme(&list.handle);
    for (index, width, title) in columns(lang) {
        insert_report_list_view_column(&list, index as i32, width, title);
    }

    let btn_edit = button(tr.btn_edit, &window);
    let btn_delete = button(tr.btn_delete, &window);
    let btn_copy = button(tr.btn_copy, &window);
    btn_edit.set_enabled(false);
    btn_delete.set_enabled(false);

    let mut status_bar = nwg::StatusBar::default();
    nwg::StatusBar::builder().parent(&window).text("").build(&mut status_bar).expect("status_bar");

    let mut notice = nwg::Notice::default();
    nwg::Notice::builder().parent(&window).build(&mut notice).expect("notice");
    let mut watch_notice = nwg::Notice::default();
    nwg::Notice::builder().parent(&window).build(&mut watch_notice).expect("watch_notice");

    apply_window_icon(&window.handle);

    let dir = store::data_dir();
    let today = Date::today();
    DailyApp {
        window,
        btn_prev,
        date_label,
        date_font,
        btn_next,
        btn_today,
        task_label,
        task_input,
        btn_recent,
        text_label,
        text_input,
        btn_add,
        list,
        btn_edit,
        btn_delete,
        btn_copy,
        status_bar,
        notice,
        watch_notice,
        menu_open_folder,
        menu_reload,
        menu_exit,
        menu_copy,
        menu_prefs,
        menu_about,
        menu_file,
        menu_report,
        menu_tools,
        menu_help,
        watch: Arc::new(Mutex::new(WatchTarget { dir: dir.clone(), date: today, seen: None })),
        state: RefCell::new(UiState {
            lang,
            settings,
            dir,
            date: today,
            day: Day::default(),
            damage: None,
            rows: Vec::new(),
            sort: None,
            recent_tasks: Vec::new(),
            edit_target: None,
        }),
        shared: Arc::new(Mutex::new(Shared::default())),
    }
}

/// Index, width and caption of each list column, in one place so building the
/// list and re-captioning it after a language change cannot disagree.
fn columns(lang: Lang) -> [(usize, i32, &'static str); 4] {
    let tr = t(lang);
    [
        (COL_TIME, COL_TIME_W, tr.col_time),
        (COL_TASK, COL_TASK_W, tr.col_task),
        (COL_TEXT, 380, tr.col_text),
        (COL_AUTHOR, COL_AUTHOR_W, tr.col_author),
    ]
}

impl DailyApp {
    /// The day row on top, the form under it, the list taking the rest, the
    /// buttons under the list, the status bar at the bottom.
    pub(crate) fn layout(&self) {
        let (width, height) = self.window.size();
        let (width, height) = (width as i32, height as i32);
        if width < MIN_W || height < MIN_H {
            return;
        }
        let day_y = MARGIN;
        let task_y = day_y + BUTTON_H + ROW_GAP + 2;
        let text_y = task_y + ROW_H + ROW_GAP;
        let list_y = text_y + ROW_H + MARGIN;
        let button_y = height - STATUS_H - BUTTON_H - MARGIN;
        let list_h = (button_y - list_y - MARGIN).max(60);

        // Day row, centred: ◄  quarta, 04/11/2026  ►  [Hoje]
        let row_w = ARROW_W * 2 + DATE_W + TODAY_W + BUTTON_GAP * 3;
        let mut x = ((width - row_w) / 2).max(MARGIN);
        self.btn_prev.set_position(x, day_y);
        self.btn_prev.set_size(ARROW_W as u32, BUTTON_H as u32);
        x += ARROW_W + BUTTON_GAP;
        self.date_label.set_position(x, day_y + 2);
        self.date_label.set_size(DATE_W as u32, (BUTTON_H - 2) as u32);
        x += DATE_W + BUTTON_GAP;
        self.btn_next.set_position(x, day_y);
        self.btn_next.set_size(ARROW_W as u32, BUTTON_H as u32);
        x += ARROW_W + BUTTON_GAP;
        self.btn_today.set_position(x, day_y);
        self.btn_today.set_size(TODAY_W as u32, BUTTON_H as u32);

        // Task row: label, the field, and ▾ for the recent tasks.
        let field_x = MARGIN + LABEL_W;
        self.task_label.set_position(MARGIN, task_y + 4);
        self.task_label.set_size(LABEL_W as u32, 20);
        let recent_x = width - MARGIN - ADD_W - BUTTON_GAP - RECENT_W;
        self.task_input.set_position(field_x, task_y);
        self.task_input.set_size((recent_x - BUTTON_GAP - field_x).max(80) as u32, ROW_H as u32);
        self.btn_recent.set_position(recent_x, task_y);
        self.btn_recent.set_size(RECENT_W as u32, ROW_H as u32);

        // Text row: label, the field (lined up with the task field above), Anotar.
        let add_x = width - MARGIN - ADD_W;
        self.text_label.set_position(MARGIN, text_y + 4);
        self.text_label.set_size(LABEL_W as u32, 20);
        self.text_input.set_position(field_x, text_y);
        self.text_input.set_size((add_x - BUTTON_GAP - field_x).max(80) as u32, ROW_H as u32);
        self.btn_add.set_position(add_x, text_y - 1);
        self.btn_add.set_size(ADD_W as u32, BUTTON_H as u32);

        let list_w = width - 2 * MARGIN;
        self.list.set_position(MARGIN, list_y);
        self.list.set_size(list_w as u32, list_h as u32);
        // "O que fiz" is the column with no length limit, so it takes the
        // width the others leave.
        let text_w = (list_w - COL_TIME_W - COL_TASK_W - COL_AUTHOR_W - LIST_CHROME_W).max(COL_TEXT_MIN_W);
        self.list.set_column_width(COL_TEXT, text_w as isize);

        let mut x = MARGIN;
        for control in [&self.btn_edit, &self.btn_delete] {
            control.set_position(x, button_y);
            control.set_size(BUTTON_W as u32, BUTTON_H as u32);
            x += BUTTON_W + BUTTON_GAP;
        }
        self.btn_copy.set_position(width - MARGIN - COPY_W, button_y);
        self.btn_copy.set_size(COPY_W as u32, BUTTON_H as u32);
    }

    /// Re-captions every visible string after the language changed, in place —
    /// the window keeps the day on screen, what is typed in the form and the
    /// selection.
    pub(crate) fn relabel_all(&self) {
        let lang = self.state.borrow().lang;
        let tr = t(lang);
        self.window.set_text(tr.app_title);
        self.btn_today.set_text(tr.btn_today);
        self.task_label.set_text(tr.lbl_task);
        self.text_label.set_text(tr.lbl_text);
        self.task_input.set_placeholder_text(Some(tr.task_hint));
        self.text_input.set_placeholder_text(Some(tr.text_hint));
        self.btn_add.set_text(tr.btn_add);
        self.btn_edit.set_text(tr.btn_edit);
        self.btn_delete.set_text(tr.btn_delete);
        self.btn_copy.set_text(tr.btn_copy);
        for (index, _, title) in columns(lang) {
            relabel_column(&self.list, index, title);
        }
        self.relabel_menus(lang);
        // The date, the author column and the status line are translated too.
        self.reload();
    }

    /// Menu captions, which nwg cannot set after creation — the two Win32
    /// shims in `foundry_common::ui` do it in place.
    fn relabel_menus(&self, lang: Lang) {
        let tr = t(lang);
        set_submenu_text(&self.menu_file, tr.menu_file);
        set_submenu_text(&self.menu_report, tr.menu_report);
        set_submenu_text(&self.menu_tools, tr.menu_tools);
        set_submenu_text(&self.menu_help, tr.menu_help);
        set_menu_item_text(&self.menu_open_folder, tr.menu_open_folder);
        set_menu_item_text(&self.menu_reload, tr.menu_reload);
        set_menu_item_text(&self.menu_exit, tr.menu_exit);
        set_menu_item_text(&self.menu_copy, tr.menu_copy);
        set_menu_item_text(&self.menu_prefs, tr.menu_prefs);
        set_menu_item_text(&self.menu_about, tr.menu_about);
        if let Some(hwnd) = self.window.handle.hwnd() {
            // SAFETY: a live top-level HWND; DrawMenuBar takes nothing else.
            unsafe { winapi::um::winuser::DrawMenuBar(hwnd) };
        }
    }
}

/// Rewrites one column header, leaving its width alone — the user may have
/// dragged it, and a language change is no reason to undo that.
fn relabel_column(list: &nwg::ListView, index: usize, title: &str) {
    list.update_column(
        index,
        nwg::InsertListViewColumn { index: Some(index as i32), fmt: None, width: None, text: Some(title.into()) },
    );
}
