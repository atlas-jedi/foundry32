//! Main hub window. A Windows-Classic card gallery: a welcome header with a
//! "check for updates" action, then tools laid out as raised 3D panels grouped
//! into "Installed" and "Discover more" sections. Each card is self-contained —
//! a colored glyph icon, name, version, description, a status chip, an "Open"/
//! "Install" primary button and a "…" overflow menu — over a classic bevel
//! drawn by owner-draw (DrawEdge). Modern accents: colored status glyphs,
//! DPI awareness, threaded install with live progress and cancel.

pub mod preferences_dialog;

use crate::i18n::{t, Lang, T};
use crate::installed::InstalledState;
use crate::model::{self, ToolStatus, ToolView};
use crate::registry::{self, Catalog, Source};
use crate::settings::AppSettings;
use crate::update_check::{self, UpdateInfo};
use crate::{download, engine, paths};
use foundry_common::theme::{apply_classic_button_theme, apply_classic_theme, create_glyph_icon};
use foundry_common::ui::{set_menu_item_text, set_submenu_text};
use native_windows_gui as nwg;
use std::cell::RefCell;
use std::collections::HashMap;
use std::os::windows::process::CommandExt;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use winapi::shared::windef::{HICON, HWND};

const REPO_URL: &str = "https://github.com/atlas-jedi/foundry32";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

// Layout metrics in logical pixels (nwg setters apply the DPI scale).
const MARGIN: i32 = 16;
const CARD_W: i32 = 250;
const CARD_H: i32 = 210;
const CARD_GAP: i32 = 14;
const CARD_PAD: i32 = 16;
const ICON_SIZE: i32 = 48;
const STATUS_ICON: i32 = 16;
const BTN_H: i32 = 28;
const MORE_W: i32 = 36;
const CHECK_W: i32 = 172;
const PROGRESS_H: i32 = 20;
const BOTTOM_BTN_W: i32 = 104;
const SECTION_H: i32 = 26;
const DIVIDER_Y: i32 = MARGIN + 60;
const STATUS_TOOLS_W: i32 = 120;
const STATUS_VERSION_W: i32 = 80;
const VK_F5: u32 = 0x74;

/// Segoe MDL2 Assets glyphs (the system icon font on Windows 10/11).
const GLYPH_CHECKMARK: u16 = 0xE73E;
const GLYPH_WARNING: u16 = 0xE7BA;
const GLYPH_ERROR: u16 = 0xE783;
/// Per-card status glyphs.
const GLYPH_INSTALLED: u16 = 0xE73E; // completed checkmark
const GLYPH_UPDATE: u16 = 0xE777; // update available
const GLYPH_AVAILABLE: u16 = 0xE896; // download
const GLYPH_UNAVAILABLE: u16 = 0xE785; // lock
/// Per-tool icon glyphs.
const GLYPH_CONSOLE: u16 = 0xE756; // command prompt
const GLYPH_TOOL: u16 = 0xE74C; // generic component

/// Fluent state colors as (r, g, b).
const COLOR_OK: (u8, u8, u8) = (0x10, 0x7C, 0x10);
const COLOR_WARNING: (u8, u8, u8) = (0x9D, 0x5D, 0x00);
const COLOR_ERROR: (u8, u8, u8) = (0xC4, 0x2B, 0x1C);
const COLOR_ACCENT: (u8, u8, u8) = (0x2B, 0x57, 0x9A);
const COLOR_TOOL_DEFAULT: (u8, u8, u8) = (0x40, 0x53, 0x6B);
/// COLORREF text tints for muted / body copy.
const TEXT_MUTED: u32 = rgb(0x66, 0x66, 0x66);
const TEXT_BODY: u32 = rgb(0x3A, 0x3A, 0x3A);

/// Popup ("…") command ids — returned synchronously by TrackPopupMenu.
const CMD_DETAILS: usize = 1;
const CMD_UPDATE: usize = 2;
const CMD_UNINSTALL: usize = 3;
const CMD_HOMEPAGE: usize = 4;
/// Raw handler id for the window owner-draw / text-color hook (must be > 0xFFFF).
const RAW_HANDLER_ID: usize = 0x1_0000;

/// Packs (r, g, b) into a Win32 COLORREF (0x00BBGGRR).
const fn rgb(r: u8, g: u8, b: u8) -> u32 {
    (r as u32) | ((g as u32) << 8) | ((b as u32) << 16)
}

#[derive(Clone, Copy)]
enum StatusTone {
    Busy,
    Ok,
    Warning,
    Error,
}

struct StatusIcons {
    ok: HICON,
    warning: HICON,
    error: HICON,
}

pub(crate) struct ProgressMsg {
    pub tool: String,
    pub done: u64,
    pub total: Option<u64>,
}

pub(crate) enum OpResult {
    Done,
    InUse,
    Failed { message: String },
    Cancelled,
}

/// Mailbox written by worker threads, drained on the UI thread when the Notice
/// fires.
#[derive(Default)]
pub(crate) struct Shared {
    pub catalog: Option<(Catalog, Source)>,
    pub progress: Option<ProgressMsg>,
    pub op_result: Option<OpResult>,
    pub update: Option<Result<Option<UpdateInfo>, String>>,
    pub preferences: Option<Option<Lang>>,
}

/// What the owner-draw hook needs: card frame rects (logical px), the header
/// divider y, and per-static text colors keyed by HWND.
#[derive(Default)]
struct PaintData {
    cards: Vec<(i32, i32, i32, i32)>,
    divider_y: i32,
    text_colors: HashMap<isize, u32>,
}

#[derive(Clone, Copy)]
enum CardKind {
    Installed { update: bool },
    Available { installable: bool },
}

/// The controls that make up one tool card. All are children of the main
/// window (so their WM_COMMAND reaches the window handler); the classic bevel
/// behind them is painted by the window owner-draw hook.
struct Card {
    view_index: usize,
    kind: CardKind,
    icon: nwg::ImageFrame,
    name: nwg::Label,
    version: nwg::Label,
    desc: nwg::Label,
    status_icon: nwg::ImageFrame,
    status_text: nwg::Label,
    btn_primary: nwg::Button,
    btn_more: nwg::Button,
    icon_hicon: HICON,
    status_hicon: HICON,
}

impl Drop for Card {
    fn drop(&mut self) {
        use winapi::um::winuser::DestroyIcon;
        unsafe {
            if !self.icon_hicon.is_null() {
                DestroyIcon(self.icon_hicon);
            }
            if !self.status_hicon.is_null() {
                DestroyIcon(self.status_hicon);
            }
        }
    }
}

enum CardButton {
    Primary,
    More,
}

struct UiState {
    lang: Lang,
    catalog: Catalog,
    views: Vec<ToolView>,
    busy: bool,
    update_url: Option<String>,
}

pub struct HubApp {
    window: nwg::Window,
    lbl_title: nwg::Label,
    lbl_subtitle: nwg::Label,
    btn_check: nwg::Button,
    lbl_sec_installed: nwg::Label,
    lbl_sec_available: nwg::Label,
    lbl_sec_hint: nwg::Label,
    // Header / section fonts are held only to keep their HFONTs alive for the
    // fixed labels that use them (set once at build, never re-read).
    #[allow(dead_code)]
    font_title: nwg::Font,
    #[allow(dead_code)]
    font_section: nwg::Font,
    font_name: nwg::Font,
    font_small: nwg::Font,
    progress: nwg::ProgressBar,
    btn_cancel: nwg::Button,
    menu_file: nwg::Menu,
    mi_refresh: nwg::MenuItem,
    mi_prefs: nwg::MenuItem,
    mi_exit: nwg::MenuItem,
    menu_help: nwg::Menu,
    mi_site: nwg::MenuItem,
    mi_about: nwg::MenuItem,
    _menu_seps: Vec<nwg::MenuSeparator>,
    status_bar: nwg::StatusBar,
    status_icons: StatusIcons,
    notice: nwg::Notice,
    cards: RefCell<Vec<Card>>,
    paint: Rc<RefCell<PaintData>>,
    state: RefCell<UiState>,
    shared: Arc<Mutex<Shared>>,
    cancel: Arc<AtomicBool>,
}

pub fn run(settings: AppSettings) {
    nwg::init().expect("failed to init native-windows-gui");
    let _ = nwg::Font::set_global_family("Segoe UI");
    let app = Rc::new(build_app(settings));
    wire_events(&app);
    app.redraw_menu_bar();
    app.layout();
    app.set_version_text();
    app.set_status(app.tr().status_fetching, StatusTone::Busy);
    app.spawn_fetch_catalog();
    app.spawn_update_check();
    nwg::dispatch_thread_events();
}

fn build_app(settings: AppSettings) -> HubApp {
    let lang = settings.lang;
    let tr = t(lang);

    let mut window = nwg::Window::default();
    nwg::Window::builder()
        .size((1060, 700))
        .position((180, 90))
        .title(tr.app_title)
        .flags(nwg::WindowFlags::MAIN_WINDOW | nwg::WindowFlags::VISIBLE | nwg::WindowFlags::RESIZABLE)
        .build(&mut window)
        .expect("window");

    let mut menu_seps = Vec::new();
    let mut separator = |parent: &nwg::Menu| {
        let mut sep = nwg::MenuSeparator::default();
        nwg::MenuSeparator::builder().parent(parent).build(&mut sep).expect("menu separator");
        menu_seps.push(sep);
    };
    let menu = |text: &str, parent: &nwg::Window| {
        let mut control = nwg::Menu::default();
        nwg::Menu::builder().text(text).parent(parent).build(&mut control).expect("menu");
        control
    };
    let item = |text: &str, parent: &nwg::Menu| {
        let mut control = nwg::MenuItem::default();
        nwg::MenuItem::builder().text(text).parent(parent).build(&mut control).expect("menu item");
        control
    };

    let menu_file = menu(tr.menu_file, &window);
    let mi_refresh = item(tr.menu_file_refresh, &menu_file);
    let mi_prefs = item(tr.menu_file_prefs, &menu_file);
    separator(&menu_file);
    let mi_exit = item(tr.menu_file_exit, &menu_file);

    let menu_help = menu(tr.menu_help, &window);
    let mi_site = item(tr.menu_help_site, &menu_help);
    let mi_about = item(tr.menu_help_about, &menu_help);

    let font = |size: u32, weight: u32| {
        let mut f = nwg::Font::default();
        let _ = nwg::Font::builder().family("Segoe UI").size(size).weight(weight).build(&mut f);
        f
    };
    let font_title = font(24, 700);
    let font_section = font(15, 700);
    let font_name = font(14, 600);
    let font_small = font(12, 400);

    let label = |text: &str, parent: &nwg::Window| {
        let mut control = nwg::Label::default();
        nwg::Label::builder().parent(parent).text(text).build(&mut control).expect("label");
        control
    };
    let lbl_title = label(tr.header_title, &window);
    let lbl_subtitle = label(tr.header_subtitle, &window);
    let lbl_sec_installed = label(tr.sec_installed, &window);
    let lbl_sec_available = label(tr.sec_available, &window);
    let lbl_sec_hint = label(tr.sec_available_hint, &window);
    lbl_title.set_font(Some(&font_title));
    lbl_subtitle.set_font(Some(&font_small));
    lbl_sec_installed.set_font(Some(&font_section));
    lbl_sec_available.set_font(Some(&font_section));
    lbl_sec_hint.set_font(Some(&font_small));

    let mut btn_check = nwg::Button::default();
    nwg::Button::builder().parent(&window).text(tr.btn_check_updates).build(&mut btn_check).expect("btn_check");
    apply_classic_button_theme(&btn_check);

    let mut progress = nwg::ProgressBar::default();
    nwg::ProgressBar::builder().parent(&window).range(0..100).build(&mut progress).expect("progress");
    apply_classic_theme(&progress.handle); // classic segmented block style
    progress.set_visible(false);

    let mut btn_cancel = nwg::Button::default();
    nwg::Button::builder().parent(&window).text(tr.btn_cancel).build(&mut btn_cancel).expect("btn_cancel");
    apply_classic_button_theme(&btn_cancel);
    btn_cancel.set_visible(false);

    let mut status_bar = nwg::StatusBar::default();
    nwg::StatusBar::builder().parent(&window).text("").build(&mut status_bar).expect("status_bar");

    let mut notice = nwg::Notice::default();
    nwg::Notice::builder().parent(&window).build(&mut notice).expect("notice");

    if let Ok(embed) = nwg::EmbedResource::load(None) {
        if let Some(icon) = embed.icon(1, None) {
            window.set_icon(Some(&icon));
        }
    }

    HubApp {
        window,
        lbl_title,
        lbl_subtitle,
        btn_check,
        lbl_sec_installed,
        lbl_sec_available,
        lbl_sec_hint,
        font_title,
        font_section,
        font_name,
        font_small,
        progress,
        btn_cancel,
        menu_file,
        mi_refresh,
        mi_prefs,
        mi_exit,
        menu_help,
        mi_site,
        mi_about,
        _menu_seps: menu_seps,
        status_bar,
        status_icons: create_status_icons(),
        notice,
        cards: RefCell::new(Vec::new()),
        paint: Rc::new(RefCell::new(PaintData::default())),
        state: RefCell::new(UiState {
            lang,
            catalog: Catalog::default(),
            views: Vec::new(),
            busy: false,
            update_url: None,
        }),
        shared: Arc::new(Mutex::new(Shared::default())),
        cancel: Arc::new(AtomicBool::new(false)),
    }
}

fn wire_events(app: &Rc<HubApp>) {
    // Owner-draw + text-color hook on the window (raised card bevels, muted /
    // status-tinted static text). Kept for the lifetime of the process.
    let paint = Rc::clone(&app.paint);
    // RawEventHandler has no Drop, so dropping the Result leaves the subclass
    // installed for the lifetime of the window.
    let _ = nwg::bind_raw_event_handler(&app.window.handle, RAW_HANDLER_ID, move |hwnd, msg, w, l| {
        use winapi::um::winuser::{GetSysColorBrush, COLOR_BTNFACE, WM_CTLCOLORSTATIC, WM_ERASEBKGND};
        match msg {
            WM_ERASEBKGND => {
                unsafe { paint_background(hwnd, w as winapi::shared::windef::HDC, &paint.borrow()) };
                Some(1)
            }
            WM_CTLCOLORSTATIC => {
                let child = l;
                let color = paint.borrow().text_colors.get(&child).copied();
                color.map(|c| {
                    use winapi::um::wingdi::{SetBkMode, SetTextColor, TRANSPARENT};
                    let hdc = w as winapi::shared::windef::HDC;
                    unsafe {
                        SetTextColor(hdc, c);
                        SetBkMode(hdc, TRANSPARENT as i32);
                        GetSysColorBrush(COLOR_BTNFACE) as isize
                    }
                })
            }
            _ => None,
        }
    });

    let evt_app = Rc::downgrade(app);
    let handler = nwg::full_bind_event_handler(&app.window.handle, move |evt, evt_data, handle| {
        let Some(app) = evt_app.upgrade() else { return };
        use nwg::Event as E;
        match evt {
            E::OnWindowClose if handle == app.window.handle => nwg::stop_thread_dispatch(),
            E::OnResize | E::OnResizeEnd | E::OnWindowMaximize if handle == app.window.handle => app.layout(),
            E::OnButtonClick => {
                if handle == app.btn_check.handle {
                    app.on_check_updates();
                } else if handle == app.btn_cancel.handle {
                    app.cancel.store(true, Ordering::Relaxed);
                } else if let Some((index, which)) = app.locate_card_button(&handle) {
                    match which {
                        CardButton::Primary => app.card_primary(index),
                        CardButton::More => app.card_more(index),
                    }
                }
            }
            E::OnMenuItemSelected => {
                if handle == app.mi_exit.handle {
                    nwg::stop_thread_dispatch();
                } else if handle == app.mi_refresh.handle {
                    app.refresh();
                } else if handle == app.mi_prefs.handle {
                    app.open_preferences();
                } else if handle == app.mi_site.handle {
                    open_in_browser(REPO_URL);
                } else if handle == app.mi_about.handle {
                    app.show_about();
                }
            }
            E::OnKeyRelease => {
                if let nwg::EventData::OnKey(VK_F5) = evt_data {
                    if !app.state.borrow().busy {
                        app.refresh();
                    }
                }
            }
            E::OnNotice if handle == app.notice.handle => app.drain_shared(),
            _ => {}
        }
    });
    std::mem::forget(handler);
}

/// Paints the window background: flat classic face, a raised bevel around each
/// card rect, and an etched divider under the header. Runs on WM_ERASEBKGND so
/// child controls paint on top.
unsafe fn paint_background(hwnd: HWND, hdc: winapi::shared::windef::HDC, paint: &PaintData) {
    use winapi::shared::windef::RECT;
    use winapi::um::winuser::{
        DrawEdge, FillRect, GetClientRect, GetSysColorBrush, BF_RECT, BF_TOP, COLOR_BTNFACE, EDGE_ETCHED, EDGE_RAISED,
    };

    let mut client: RECT = std::mem::zeroed();
    GetClientRect(hwnd, &mut client);
    FillRect(hdc, &client, GetSysColorBrush(COLOR_BTNFACE));

    let scale = nwg::scale_factor();
    let dev = |v: i32| (v as f64 * scale) as i32;

    for &(x, y, w, h) in &paint.cards {
        let mut r = RECT { left: dev(x), top: dev(y), right: dev(x + w), bottom: dev(y + h) };
        DrawEdge(hdc, &mut r, EDGE_RAISED, BF_RECT);
    }

    if paint.divider_y > 0 {
        let mut r = RECT {
            left: dev(MARGIN),
            top: dev(paint.divider_y),
            right: client.right - dev(MARGIN),
            bottom: dev(paint.divider_y) + 2,
        };
        DrawEdge(hdc, &mut r, EDGE_ETCHED, BF_TOP);
    }
}

impl HubApp {
    fn tr(&self) -> &'static T {
        t(self.state.borrow().lang)
    }

    fn layout(&self) {
        let (width, height) = self.window.size();
        let (width, height) = (width as i32, height as i32);
        if width < 420 || height < 320 {
            return;
        }
        let status_h = self.status_bar_height();

        // Header row: title + subtitle on the left, check-updates on the right.
        self.lbl_title.set_position(MARGIN, MARGIN);
        self.lbl_title.set_size((width - 2 * MARGIN - CHECK_W - 8).max(80) as u32, 30);
        self.lbl_subtitle.set_position(MARGIN, MARGIN + 34);
        self.lbl_subtitle.set_size((width - 2 * MARGIN).max(80) as u32, 20);
        self.btn_check.set_position(width - MARGIN - CHECK_W, MARGIN + 2);
        self.btn_check.set_size(CHECK_W as u32, BTN_H as u32);

        // Bottom band: progress + cancel while an operation runs.
        let busy = self.state.borrow().busy;
        if busy {
            let band_y = height - status_h - MARGIN - BTN_H;
            self.progress.set_position(MARGIN, band_y + (BTN_H - PROGRESS_H) / 2);
            self.progress.set_size((width - 2 * MARGIN - BOTTOM_BTN_W - 8).max(60) as u32, PROGRESS_H as u32);
            self.btn_cancel.set_position(width - MARGIN - BOTTOM_BTN_W, band_y);
            self.btn_cancel.set_size(BOTTOM_BTN_W as u32, BTN_H as u32);
        }

        // Card grid, installed section first.
        let columns = ((width - 2 * MARGIN + CARD_GAP) / (CARD_W + CARD_GAP)).max(1);
        let cards = self.cards.borrow();
        let n_installed = cards.iter().filter(|c| matches!(c.kind, CardKind::Installed { .. })).count();
        let mut rects = Vec::with_capacity(cards.len());
        let mut y = DIVIDER_Y + 14;

        if n_installed > 0 {
            self.lbl_sec_installed.set_visible(true);
            self.lbl_sec_installed.set_position(MARGIN, y);
            self.lbl_sec_installed.set_size((width - 2 * MARGIN).max(80) as u32, 22);
            y += SECTION_H;
            y = self.place_grid(&cards[..n_installed], y, columns, &mut rects) + 12;
        } else {
            self.lbl_sec_installed.set_visible(false);
        }

        if cards.len() > n_installed {
            self.lbl_sec_available.set_visible(true);
            self.lbl_sec_hint.set_visible(true);
            self.lbl_sec_available.set_position(MARGIN, y);
            self.lbl_sec_available.set_size((width - 2 * MARGIN).max(80) as u32, 22);
            self.lbl_sec_hint.set_position(MARGIN, y + 22);
            self.lbl_sec_hint.set_size((width - 2 * MARGIN).max(80) as u32, 18);
            y += SECTION_H + 20;
            self.place_grid(&cards[n_installed..], y, columns, &mut rects);
        } else {
            self.lbl_sec_available.set_visible(false);
            self.lbl_sec_hint.set_visible(false);
        }
        drop(cards);

        {
            let mut paint = self.paint.borrow_mut();
            paint.cards = rects;
            paint.divider_y = DIVIDER_Y;
            paint.text_colors = self.compute_text_colors();
        }
        self.set_status_parts();
        self.invalidate();
    }

    /// Positions the inner controls of every card in `slice` into a wrapping
    /// grid starting at `y0`, recording each frame rect. Returns the y just
    /// below the last row.
    fn place_grid(&self, slice: &[Card], y0: i32, columns: i32, rects: &mut Vec<(i32, i32, i32, i32)>) -> i32 {
        let mut max_y = y0;
        for (k, card) in slice.iter().enumerate() {
            let col = (k as i32) % columns;
            let row = (k as i32) / columns;
            let cx = MARGIN + col * (CARD_W + CARD_GAP);
            let cy = y0 + row * (CARD_H + CARD_GAP);
            self.place_card(card, cx, cy);
            rects.push((cx, cy, CARD_W, CARD_H));
            max_y = max_y.max(cy + CARD_H);
        }
        max_y
    }

    fn place_card(&self, card: &Card, cx: i32, cy: i32) {
        let ix = cx + CARD_PAD;
        let inner_w = (CARD_W - 2 * CARD_PAD).max(40) as u32;

        card.icon.set_position(ix, cy + 16);
        card.icon.set_size(ICON_SIZE as u32, ICON_SIZE as u32);
        card.name.set_position(ix, cy + 70);
        card.name.set_size(inner_w, 20);
        card.version.set_position(ix, cy + 92);
        card.version.set_size(inner_w, 16);
        card.desc.set_position(ix, cy + 112);
        card.desc.set_size(inner_w, 34);
        card.status_icon.set_position(ix, cy + 150);
        card.status_icon.set_size(STATUS_ICON as u32, STATUS_ICON as u32);
        card.status_text.set_position(ix + STATUS_ICON + 6, cy + 150);
        card.status_text.set_size((inner_w as i32 - STATUS_ICON - 6).max(20) as u32, 18);

        let btn_y = cy + CARD_H - 14 - BTN_H;
        let more_x = cx + CARD_W - CARD_PAD - MORE_W;
        card.btn_more.set_position(more_x, btn_y);
        card.btn_more.set_size(MORE_W as u32, BTN_H as u32);
        card.btn_primary.set_position(ix, btn_y);
        card.btn_primary.set_size((more_x - 8 - ix).max(40) as u32, BTN_H as u32);
    }

    /// Rebuilds the per-static text color map from the current cards + header.
    fn compute_text_colors(&self) -> HashMap<isize, u32> {
        let mut map = HashMap::new();
        let mut insert = |label: &nwg::Label, color: u32| {
            if let Some(hwnd) = label.handle.hwnd() {
                map.insert(hwnd as isize, color);
            }
        };
        insert(&self.lbl_subtitle, TEXT_MUTED);
        insert(&self.lbl_sec_hint, TEXT_MUTED);
        for card in self.cards.borrow().iter() {
            insert(&card.version, TEXT_MUTED);
            insert(&card.desc, TEXT_BODY);
            let tone = match card.kind {
                CardKind::Installed { update: false } => rgb(COLOR_OK.0, COLOR_OK.1, COLOR_OK.2),
                CardKind::Installed { update: true } => rgb(COLOR_WARNING.0, COLOR_WARNING.1, COLOR_WARNING.2),
                CardKind::Available { installable: true } => rgb(COLOR_ACCENT.0, COLOR_ACCENT.1, COLOR_ACCENT.2),
                CardKind::Available { installable: false } => TEXT_MUTED,
            };
            insert(&card.status_text, tone);
        }
        map
    }

    fn invalidate(&self) {
        use winapi::um::winuser::InvalidateRect;
        if let Some(hwnd) = self.window.handle.hwnd() {
            unsafe { InvalidateRect(hwnd, std::ptr::null(), 1) };
        }
    }

    fn status_bar_height(&self) -> i32 {
        use winapi::um::winuser::GetWindowRect;
        let Some(hwnd) = self.status_bar.handle.hwnd() else { return 23 };
        let mut rect: winapi::shared::windef::RECT = unsafe { std::mem::zeroed() };
        unsafe { GetWindowRect(hwnd, &mut rect) };
        ((rect.bottom - rect.top) as f64 / nwg::scale_factor()) as i32
    }

    fn set_status_parts(&self) {
        use winapi::um::commctrl::SB_SETPARTS;
        use winapi::um::winuser::SendMessageW;
        let Some(hwnd) = self.status_bar.handle.hwnd() else { return };
        let scale = nwg::scale_factor();
        let px = |v: i32| (v as f64 * scale) as i32;
        let width = px(self.window.size().0 as i32);
        let right = px(STATUS_VERSION_W);
        let edges = [width - px(STATUS_TOOLS_W) - right, width - right, -1i32];
        unsafe {
            SendMessageW(hwnd, SB_SETPARTS, edges.len(), edges.as_ptr() as isize);
        }
    }

    fn refresh(&self) {
        if self.state.borrow().busy {
            return;
        }
        self.set_status(self.tr().status_fetching, StatusTone::Busy);
        self.spawn_fetch_catalog();
    }

    /// "Check for Updates": if a newer Foundry32 is already known, open its
    /// release page; otherwise re-fetch the catalog and re-run the update check.
    fn on_check_updates(&self) {
        if self.state.borrow().busy {
            return;
        }
        if let Some(url) = self.state.borrow().update_url.clone() {
            open_in_browser(&url);
            return;
        }
        self.refresh();
        self.spawn_update_check();
    }

    fn spawn_fetch_catalog(&self) {
        self.state.borrow_mut().busy = true;
        let shared = Arc::clone(&self.shared);
        let sender = self.notice.sender();
        std::thread::spawn(move || {
            let loaded = registry::load();
            shared.lock().unwrap().catalog = Some(loaded);
            sender.notice();
        });
    }

    fn spawn_update_check(&self) {
        let shared = Arc::clone(&self.shared);
        let sender = self.notice.sender();
        std::thread::spawn(move || {
            let result = update_check::check_for_update();
            shared.lock().unwrap().update = Some(result);
            sender.notice();
        });
    }

    fn view_at(&self, index: usize) -> Option<ToolView> {
        self.state.borrow().views.get(index).cloned()
    }

    fn locate_card_button(&self, handle: &nwg::ControlHandle) -> Option<(usize, CardButton)> {
        self.cards.borrow().iter().find_map(|card| {
            if *handle == card.btn_primary.handle {
                Some((card.view_index, CardButton::Primary))
            } else if *handle == card.btn_more.handle {
                Some((card.view_index, CardButton::More))
            } else {
                None
            }
        })
    }

    fn card_primary(&self, index: usize) {
        let Some(view) = self.view_at(index) else { return };
        match view.status {
            ToolStatus::Installed | ToolStatus::UpdateAvailable => self.do_run(index),
            ToolStatus::NotInstalled => self.do_install_or_update(index, false),
        }
    }

    /// Opens the "…" overflow menu for a card and acts on the chosen command.
    fn card_more(&self, index: usize) {
        use winapi::um::winuser::{
            AppendMenuW, CreatePopupMenu, DestroyMenu, GetCursorPos, TrackPopupMenu, MF_SEPARATOR, MF_STRING,
            TPM_LEFTALIGN, TPM_RETURNCMD, TPM_TOPALIGN,
        };
        let Some(view) = self.view_at(index) else { return };
        let Some(hwnd) = self.window.handle.hwnd() else { return };
        let tr = self.tr();
        let wide = |s: &str| -> Vec<u16> { s.encode_utf16().chain(std::iter::once(0)).collect() };

        let cmd = unsafe {
            let menu = CreatePopupMenu();
            AppendMenuW(menu, MF_STRING, CMD_DETAILS, wide(tr.more_details).as_ptr());
            match view.status {
                ToolStatus::UpdateAvailable => {
                    AppendMenuW(menu, MF_STRING, CMD_UPDATE, wide(tr.more_update).as_ptr());
                    AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
                    AppendMenuW(menu, MF_STRING, CMD_UNINSTALL, wide(tr.more_uninstall).as_ptr());
                }
                ToolStatus::Installed => {
                    AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
                    AppendMenuW(menu, MF_STRING, CMD_UNINSTALL, wide(tr.more_uninstall).as_ptr());
                }
                ToolStatus::NotInstalled => {
                    if !view.entry.homepage.is_empty() {
                        AppendMenuW(menu, MF_STRING, CMD_HOMEPAGE, wide(tr.more_homepage).as_ptr());
                    }
                }
            }
            let mut pt = winapi::shared::windef::POINT { x: 0, y: 0 };
            GetCursorPos(&mut pt);
            let cmd = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_LEFTALIGN | TPM_TOPALIGN, pt.x, pt.y, 0, hwnd, std::ptr::null_mut());
            DestroyMenu(menu);
            cmd as usize
        };

        match cmd {
            CMD_DETAILS => self.show_details(index),
            CMD_UPDATE => self.do_install_or_update(index, true),
            CMD_UNINSTALL => self.do_uninstall(index),
            CMD_HOMEPAGE => open_in_browser(&view.entry.homepage),
            _ => {}
        }
    }

    fn do_install_or_update(&self, index: usize, is_update: bool) {
        let Some(view) = self.view_at(index) else { return };
        if self.state.borrow().busy {
            return;
        }
        if !view.entry.is_installable() {
            nwg::modal_error_message(&self.window.handle, self.tr().op_error_title, self.tr().err_not_installable);
            return;
        }
        self.begin_op(true);
        let status = if is_update { self.tr().status_updating } else { self.tr().status_installing };
        self.set_status(&status.replace("%S", &view.entry.name), StatusTone::Busy);

        self.cancel.store(false, Ordering::Relaxed);
        let shared = Arc::clone(&self.shared);
        let sender = self.notice.sender();
        let cancel = Arc::clone(&self.cancel);
        let entry = view.entry.clone();
        let tool = view.entry.name.clone();
        std::thread::spawn(move || {
            let mut last = Instant::now();
            let mut first = true;
            let result = engine::install(&entry, &cancel, |p| {
                if first || last.elapsed() >= Duration::from_millis(150) {
                    first = false;
                    last = Instant::now();
                    shared.lock().unwrap().progress =
                        Some(ProgressMsg { tool: tool.clone(), done: p.done, total: p.total });
                    sender.notice();
                }
            });
            shared.lock().unwrap().op_result = Some(op_outcome(result));
            sender.notice();
        });
    }

    fn do_uninstall(&self, index: usize) {
        let Some(view) = self.view_at(index) else { return };
        if self.state.borrow().busy {
            return;
        }
        let tr = self.tr();
        let choice = nwg::modal_message(&self.window.handle, &nwg::MessageParams {
            title: tr.confirm_uninstall_title,
            content: &tr.confirm_uninstall_body.replace("%S", &view.entry.name),
            buttons: nwg::MessageButtons::YesNo,
            icons: nwg::MessageIcons::Warning,
        });
        if choice != nwg::MessageChoice::Yes {
            return;
        }
        self.begin_op(false);
        self.set_status(&tr.status_uninstalling.replace("%S", &view.entry.name), StatusTone::Busy);
        let shared = Arc::clone(&self.shared);
        let sender = self.notice.sender();
        let id = view.entry.id.clone();
        std::thread::spawn(move || {
            shared.lock().unwrap().op_result = Some(op_outcome(engine::uninstall(&id)));
            sender.notice();
        });
    }

    fn do_run(&self, index: usize) {
        let Some(view) = self.view_at(index) else { return };
        if let Err(error) = engine::launch(&view.entry.id) {
            nwg::modal_error_message(&self.window.handle, self.tr().op_error_title, &error.to_string());
        }
    }

    fn show_details(&self, index: usize) {
        let Some(view) = self.view_at(index) else { return };
        let lang = self.state.borrow().lang;
        let tr = t(lang);
        let title = tr.details_title.replace("%S", &view.entry.name);
        nwg::modal_info_message(&self.window.handle, &title, &details_text(&view, lang, tr));
    }

    /// Enters the busy state: disable actions, show the progress bar (for
    /// downloads) and the cancel button.
    fn begin_op(&self, cancellable: bool) {
        self.state.borrow_mut().busy = true;
        self.refresh_enabled();
        if cancellable {
            self.progress.set_visible(true);
            self.progress.set_pos(0);
            self.btn_cancel.set_visible(true);
            self.btn_cancel.set_enabled(true);
        }
        self.layout();
    }

    fn end_op(&self) {
        self.state.borrow_mut().busy = false;
        self.progress.set_visible(false);
        self.btn_cancel.set_visible(false);
        self.reload_installed();
        self.populate();
    }

    /// Enables/disables the per-card buttons and the check-updates action to
    /// match the busy state and each tool's installability.
    fn refresh_enabled(&self) {
        let busy = self.state.borrow().busy;
        self.btn_check.set_enabled(!busy);
        for card in self.cards.borrow().iter() {
            let primary = match card.kind {
                CardKind::Installed { .. } => !busy,
                CardKind::Available { installable } => !busy && installable,
            };
            card.btn_primary.set_enabled(primary);
            card.btn_more.set_enabled(!busy);
        }
    }

    fn drain_shared(&self) {
        let (catalog, progress, op_result, update, preferences) = {
            let mut shared = self.shared.lock().unwrap();
            (
                shared.catalog.take(),
                shared.progress.take(),
                shared.op_result.take(),
                shared.update.take(),
                shared.preferences.take(),
            )
        };

        if let Some((catalog, source)) = catalog {
            {
                let mut state = self.state.borrow_mut();
                state.catalog = catalog;
                state.busy = false;
            }
            self.reload_installed();
            self.populate();
            match source {
                Source::Embedded => self.set_status(self.tr().status_offline, StatusTone::Warning),
                _ => self.set_status(self.tr().status_ready, StatusTone::Ok),
            }
        }

        if let Some(p) = progress {
            self.apply_progress(&p);
        }

        if let Some(result) = op_result {
            self.end_op();
            match result {
                OpResult::Done => self.set_status(self.tr().status_done, StatusTone::Ok),
                OpResult::Cancelled => self.set_status(self.tr().status_ready, StatusTone::Ok),
                OpResult::InUse => {
                    nwg::modal_error_message(&self.window.handle, self.tr().op_error_title, self.tr().err_in_use);
                    self.set_status(self.tr().status_ready, StatusTone::Warning);
                }
                OpResult::Failed { message } => {
                    nwg::modal_error_message(&self.window.handle, self.tr().op_error_title, &message);
                    self.set_status(self.tr().status_ready, StatusTone::Error);
                }
            }
        }

        if let Some(Ok(Some(info))) = update {
            self.state.borrow_mut().update_url = Some(info.html_url);
        }

        if let Some(outcome) = preferences {
            self.window.set_enabled(true);
            if let Some(lang) = outcome {
                let changed = {
                    let mut state = self.state.borrow_mut();
                    let changed = state.lang != lang;
                    state.lang = lang;
                    changed
                };
                if changed {
                    let _ = AppSettings { lang }.save();
                    self.relabel_all();
                }
            }
        }
    }

    fn apply_progress(&self, p: &ProgressMsg) {
        match p.total {
            Some(total) if total > 0 => {
                let pct = ((p.done.min(total) as f64 / total as f64) * 100.0) as u32;
                self.progress.set_pos(pct);
                let text = self.tr().status_downloading.replace("%S", &p.tool).replace("%P", &pct.to_string());
                self.set_status(&text, StatusTone::Busy);
            }
            _ => {
                self.progress.set_pos((p.done / 65536 % 100) as u32);
                let text = self.tr().status_installing.replace("%S", &p.tool);
                self.set_status(&text, StatusTone::Busy);
            }
        }
    }

    fn reload_installed(&self) {
        let mut installed = InstalledState::load();
        installed.reconcile();
        let mut state = self.state.borrow_mut();
        state.views = model::merge(&state.catalog, &installed);
    }

    /// Rebuilds the card controls from the current views (installed first),
    /// then re-runs layout.
    fn populate(&self) {
        let (order, lang) = {
            let state = self.state.borrow();
            let mut installed = Vec::new();
            let mut available = Vec::new();
            for (i, v) in state.views.iter().enumerate() {
                match v.status {
                    ToolStatus::Installed | ToolStatus::UpdateAvailable => installed.push(i),
                    ToolStatus::NotInstalled => available.push(i),
                }
            }
            installed.extend(available);
            (installed, state.lang)
        };
        let tr = t(lang);
        let mut new_cards = Vec::with_capacity(order.len());
        for &index in &order {
            let view = self.state.borrow().views[index].clone();
            new_cards.push(self.build_card(index, &view, lang, tr));
        }
        *self.cards.borrow_mut() = new_cards;
        self.set_tools_count_text();
        self.refresh_enabled();
        self.layout();
    }

    fn build_card(&self, index: usize, view: &ToolView, lang: Lang, tr: &T) -> Card {
        let dev = |v: i32| (v as f64 * nwg::scale_factor()) as i32;
        let entry = &view.entry;

        let kind = match view.status {
            ToolStatus::Installed => CardKind::Installed { update: false },
            ToolStatus::UpdateAvailable => CardKind::Installed { update: true },
            ToolStatus::NotInstalled => CardKind::Available { installable: entry.is_installable() },
        };

        // Icon — a placeholder Icon forces the SS_ICON style, then the colored
        // glyph is set directly via STM_SETIMAGE.
        let placeholder = nwg::Icon::default();
        let mut icon = nwg::ImageFrame::default();
        nwg::ImageFrame::builder()
            .parent(&self.window)
            .icon(Some(&placeholder))
            .size((ICON_SIZE, ICON_SIZE))
            .build(&mut icon)
            .expect("card icon");
        let (glyph, glyph_color) = tool_glyph(&entry.id);
        let icon_hicon = create_glyph_icon(glyph, glyph_color, dev(ICON_SIZE));
        set_image_icon(&icon, icon_hicon);

        let label = |text: &str, font: &nwg::Font| {
            let mut control = nwg::Label::default();
            nwg::Label::builder().parent(&self.window).text(text).build(&mut control).expect("card label");
            control.set_font(Some(font));
            control
        };
        let name = label(&entry.name, &self.font_name);
        let version = label(&format!("v{}", entry.version), &self.font_small);
        let desc = label(&short_desc(view, lang), &self.font_small);

        let (status_glyph, status_color, status_label) = card_status(view, tr);
        let placeholder2 = nwg::Icon::default();
        let mut status_icon = nwg::ImageFrame::default();
        nwg::ImageFrame::builder()
            .parent(&self.window)
            .icon(Some(&placeholder2))
            .size((STATUS_ICON, STATUS_ICON))
            .build(&mut status_icon)
            .expect("card status icon");
        let status_hicon = create_glyph_icon(status_glyph, status_color, dev(STATUS_ICON));
        set_image_icon(&status_icon, status_hicon);
        let status_text = label(status_label, &self.font_small);

        let primary_text = match kind {
            CardKind::Installed { .. } => tr.btn_open,
            CardKind::Available { .. } => tr.btn_install,
        };
        let button = |text: &str| {
            let mut control = nwg::Button::default();
            nwg::Button::builder().parent(&self.window).text(text).build(&mut control).expect("card button");
            apply_classic_button_theme(&control);
            control
        };
        let btn_primary = button(primary_text);
        let btn_more = button("…");

        Card {
            view_index: index,
            kind,
            icon,
            name,
            version,
            desc,
            status_icon,
            status_text,
            btn_primary,
            btn_more,
            icon_hicon,
            status_hicon,
        }
    }

    fn open_preferences(&self) {
        let params = preferences_dialog::PreferencesParams {
            lang: self.state.borrow().lang,
            shared: Arc::clone(&self.shared),
            notify: self.notice.sender(),
        };
        self.window.set_enabled(false);
        preferences_dialog::spawn(params);
    }

    fn show_about(&self) {
        let tr = self.tr();
        let body = tr.about_body.replace("%V", CURRENT_VERSION);
        nwg::modal_info_message(&self.window.handle, tr.about_title, &body);
    }

    fn relabel_all(&self) {
        let tr = self.tr();
        self.window.set_text(tr.app_title);
        self.lbl_title.set_text(tr.header_title);
        self.lbl_subtitle.set_text(tr.header_subtitle);
        self.btn_check.set_text(tr.btn_check_updates);
        self.lbl_sec_installed.set_text(tr.sec_installed);
        self.lbl_sec_available.set_text(tr.sec_available);
        self.lbl_sec_hint.set_text(tr.sec_available_hint);
        self.btn_cancel.set_text(tr.btn_cancel);
        set_submenu_text(&self.menu_file, tr.menu_file);
        set_submenu_text(&self.menu_help, tr.menu_help);
        set_menu_item_text(&self.mi_refresh, tr.menu_file_refresh);
        set_menu_item_text(&self.mi_prefs, tr.menu_file_prefs);
        set_menu_item_text(&self.mi_exit, tr.menu_file_exit);
        set_menu_item_text(&self.mi_site, tr.menu_help_site);
        set_menu_item_text(&self.mi_about, tr.menu_help_about);
        self.redraw_menu_bar();
        self.set_version_text();
        self.populate();
    }

    fn redraw_menu_bar(&self) {
        if let Some(hwnd) = self.window.handle.hwnd() {
            unsafe { winapi::um::winuser::DrawMenuBar(hwnd) };
        }
    }

    fn set_status(&self, text: &str, tone: StatusTone) {
        use winapi::um::commctrl::SB_SETICON;
        use winapi::um::winuser::SendMessageW;
        self.status_bar.set_text(0, text);
        let Some(hwnd) = self.status_bar.handle.hwnd() else { return };
        let icon = match tone {
            StatusTone::Busy => std::ptr::null_mut(),
            StatusTone::Ok => self.status_icons.ok,
            StatusTone::Warning => self.status_icons.warning,
            StatusTone::Error => self.status_icons.error,
        };
        unsafe {
            SendMessageW(hwnd, SB_SETICON, 0, icon as isize);
        }
    }

    fn set_tools_count_text(&self) {
        let count = self.state.borrow().views.len().to_string();
        let text = self.tr().status_tools.replace("%N", &count);
        self.status_bar.set_text(1, &text);
    }

    fn set_version_text(&self) {
        self.status_bar.set_text(2, &format!("v{CURRENT_VERSION}"));
    }
}

/// Sets a raw HICON on an SS_ICON image frame (nwg's `Icon` can't wrap a bare
/// handle, so we send STM_SETIMAGE ourselves).
fn set_image_icon(frame: &nwg::ImageFrame, hicon: HICON) {
    use winapi::um::winuser::{SendMessageW, IMAGE_ICON, STM_SETIMAGE};
    if let Some(hwnd) = frame.handle.hwnd() {
        unsafe {
            SendMessageW(hwnd, STM_SETIMAGE, IMAGE_ICON as usize, hicon as isize);
        }
    }
}

/// Maps an engine result to the UI outcome, keeping the "in use" case typed so
/// the UI can show a localized message instead of the engine's English string.
fn op_outcome(result: Result<(), engine::EngineError>) -> OpResult {
    match result {
        Ok(()) => OpResult::Done,
        Err(engine::EngineError::InUse) => OpResult::InUse,
        Err(engine::EngineError::Download(download::DlError::Cancelled)) => OpResult::Cancelled,
        Err(error) => OpResult::Failed { message: error.to_string() },
    }
}

/// The status glyph, its color, and the chip label for a tool.
fn card_status(view: &ToolView, tr: &T) -> (u16, (u8, u8, u8), &'static str) {
    match view.status {
        ToolStatus::Installed => (GLYPH_INSTALLED, COLOR_OK, tr.st_installed),
        ToolStatus::UpdateAvailable => (GLYPH_UPDATE, COLOR_WARNING, tr.st_update),
        ToolStatus::NotInstalled if view.entry.is_installable() => (GLYPH_AVAILABLE, COLOR_ACCENT, tr.st_available),
        ToolStatus::NotInstalled => (GLYPH_UNAVAILABLE, COLOR_TOOL_DEFAULT, tr.st_unavailable),
    }
}

/// A per-tool icon glyph + accent color, keyed by catalog id.
fn tool_glyph(id: &str) -> (u16, (u8, u8, u8)) {
    match id {
        "mcp-console" => (GLYPH_CONSOLE, COLOR_OK),
        _ => (GLYPH_TOOL, COLOR_ACCENT),
    }
}

/// One-line, whitespace-collapsed, length-capped description for the card.
fn short_desc(view: &ToolView, lang: Lang) -> String {
    let raw = match lang {
        Lang::PtBr => &view.entry.description_pt,
        Lang::En => &view.entry.description_en,
    };
    let text: String = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() > 96 {
        let clipped: String = text.chars().take(95).collect();
        format!("{}…", clipped.trim_end())
    } else {
        text
    }
}

fn details_text(view: &ToolView, lang: Lang, tr: &T) -> String {
    let description = match lang {
        Lang::PtBr => &view.entry.description_pt,
        Lang::En => &view.entry.description_en,
    };
    let installed = view
        .installed
        .as_ref()
        .map(|t| t.version.clone())
        .unwrap_or_else(|| tr.d_none.to_string());
    let path = view
        .installed
        .as_ref()
        .map(|t| paths::tool_exe(&view.entry.id, &t.exe).display().to_string())
        .unwrap_or_else(|| tr.d_none.to_string());
    let mut lines = vec![
        view.entry.name.clone(),
        String::new(),
        format!("{}: {}", tr.d_publisher, view.entry.publisher),
        format!("{}: {}", tr.d_version, view.entry.version),
        format!("{}: {}", tr.d_installed, installed),
        format!("{}: {}", tr.d_homepage, view.entry.homepage),
        format!("{}: {}", tr.d_path, path),
        String::new(),
        description.clone(),
    ];
    if description.is_empty() {
        lines.pop();
    }
    lines.join("\r\n")
}

fn create_status_icons() -> StatusIcons {
    let size = (16.0 * nwg::scale_factor()) as i32;
    StatusIcons {
        ok: create_glyph_icon(GLYPH_CHECKMARK, COLOR_OK, size),
        warning: create_glyph_icon(GLYPH_WARNING, COLOR_WARNING, size),
        error: create_glyph_icon(GLYPH_ERROR, COLOR_ERROR, size),
    }
}

fn open_in_browser(url: &str) {
    let _ = std::process::Command::new("cmd")
        .args(["/C", "start", "", url])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
}
