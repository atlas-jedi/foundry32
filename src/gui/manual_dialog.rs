//! Manual window: topic sidebar on the left (Scopes, Types), rich-text
//! explanation pane on the right. Runs on its own thread with its own event
//! loop (nwg multithread dialog pattern) and reports closure through
//! `Shared.manual` + a Notice back to the main window.

use super::{apply_classic_button_theme, Shared};
use crate::i18n::{t, Lang, T};
use native_windows_gui as nwg;
use std::sync::{Arc, Mutex};

const MARGIN: i32 = 12;
const SIDEBAR_W: i32 = 170;
const CONTENT_X: i32 = MARGIN + SIDEBAR_W + 14;
const BUTTON_W: i32 = 95;
const BUTTON_H: i32 = 26;

/// Body text: 10pt in twips (1/20 pt).
const BODY_TWIPS: i32 = 200;
const H1_TWIPS: i32 = 340;
const H2_TWIPS: i32 = 250;
const EXAMPLE_TWIPS: i32 = 190;
const WARNING_COLOR: [u8; 3] = [0xC4, 0x2B, 0x1C];
const H2_COLOR: [u8; 3] = [0x1B, 0x3D, 0x6E];
const EXAMPLE_COLOR: [u8; 3] = [0x40, 0x40, 0x40];

pub struct ManualParams {
    pub lang: Lang,
    pub shared: Arc<Mutex<Shared>>,
    pub notify: nwg::NoticeSender,
}

pub fn spawn(params: ManualParams) {
    std::thread::spawn(move || run_manual(params));
}

fn run_manual(params: ManualParams) {
    let tr = t(params.lang);

    let mut window = nwg::Window::default();
    nwg::Window::builder()
        .size((860, 620))
        .position((280, 90))
        .title(tr.manual_title)
        .flags(nwg::WindowFlags::WINDOW | nwg::WindowFlags::VISIBLE)
        .build(&mut window)
        .expect("manual window");
    let (client_w, client_h) = window.size();
    let (client_w, client_h) = (client_w as i32, client_h as i32);
    let button_y = client_h - MARGIN - BUTTON_H;
    let content_h = button_y - 2 * MARGIN;

    let mut topics = nwg::ListBox::default();
    nwg::ListBox::builder()
        .parent(&window)
        .position((MARGIN, MARGIN))
        .size((SIDEBAR_W, content_h))
        .collection(vec![tr.manual_nav_scopes.to_string(), tr.manual_nav_types.to_string()])
        .build(&mut topics)
        .expect("topics");
    topics.set_selection(Some(0));

    let mut content = nwg::RichTextBox::default();
    nwg::RichTextBox::builder()
        .parent(&window)
        .position((CONTENT_X, MARGIN))
        .size((client_w - CONTENT_X - MARGIN, content_h))
        .readonly(true)
        .flags(
            nwg::RichTextBoxFlags::VISIBLE
                | nwg::RichTextBoxFlags::VSCROLL
                | nwg::RichTextBoxFlags::AUTOVSCROLL
                | nwg::RichTextBoxFlags::TAB_STOP,
        )
        .build(&mut content)
        .expect("content");
    set_text_margins(&content, 14);

    let mut close_btn = nwg::Button::default();
    nwg::Button::builder()
        .parent(&window)
        .text(tr.manual_close)
        .position((client_w - MARGIN - BUTTON_W, button_y))
        .size((BUTTON_W, BUTTON_H))
        .build(&mut close_btn)
        .expect("close_btn");
    apply_classic_button_theme(&close_btn);

    render_topic(&content, tr, 0);

    let window_handle = window.handle;
    let topics_handle = topics.handle;
    let close_handle = close_btn.handle;
    let shared = Arc::clone(&params.shared);
    let notify = params.notify;
    let handler = nwg::full_bind_event_handler(&window_handle, move |evt, _evt_data, handle| {
        use nwg::Event as E;
        match evt {
            E::OnWindowClose if handle == window_handle => {
                send_closed(&shared, &notify);
            }
            E::OnButtonClick if handle == close_handle => {
                send_closed(&shared, &notify);
                window.close();
            }
            E::OnListBoxSelect if handle == topics_handle => {
                let index = topics.selection().unwrap_or(0);
                render_topic(&content, tr, index);
            }
            _ => {}
        }
    });

    nwg::dispatch_thread_events();
    nwg::unbind_event_handler(&handler);
}

fn send_closed(shared: &Arc<Mutex<Shared>>, notify: &nwg::NoticeSender) {
    let mut guard = shared.lock().unwrap();
    if guard.manual.is_none() {
        guard.manual = Some(());
        drop(guard);
        notify.notice();
        nwg::stop_thread_dispatch();
    }
}

fn render_topic(content: &nwg::RichTextBox, tr: &T, index: usize) {
    let doc = match index {
        1 => tr.manual_doc_types,
        _ => tr.manual_doc_scopes,
    };
    render_document(content, doc);
}

#[derive(Clone, Copy, PartialEq)]
enum LineStyle {
    H1,
    H2,
    Warning,
    Example,
    Body,
}

/// Line-based markup: `# ` heading, `## ` subheading, `!! ` warning,
/// two-space indent example line, anything else body text.
fn classify(line: &str) -> (LineStyle, &str) {
    if let Some(rest) = line.strip_prefix("## ") {
        (LineStyle::H2, rest)
    } else if let Some(rest) = line.strip_prefix("# ") {
        (LineStyle::H1, rest)
    } else if let Some(rest) = line.strip_prefix("!! ") {
        (LineStyle::Warning, rest)
    } else if line.starts_with("  ") && !line.trim().is_empty() {
        (LineStyle::Example, line)
    } else {
        (LineStyle::Body, line)
    }
}

/// Renders the markup into the rich text box. Styling works on character
/// positions, where RichEdit counts every line break as exactly one position —
/// hence the `\n`-only source text and the manual utf16 length bookkeeping.
fn render_document(content: &nwg::RichTextBox, doc: &str) {
    let mut display = String::new();
    let mut spans: Vec<(u32, u32, LineStyle)> = Vec::new();
    let mut position: u32 = 0;
    for line in doc.split('\n') {
        let (style, text) = classify(line);
        let length = text.encode_utf16().count() as u32;
        if style != LineStyle::Body && length > 0 {
            spans.push((position, length, style));
        }
        display.push_str(text);
        display.push('\n');
        position += length + 1;
    }

    content.set_text(&display);
    content.set_selection(0..position);
    content.set_char_format(&nwg::CharFormat {
        effects: Some(nwg::CharEffects::empty()),
        height: Some(BODY_TWIPS),
        text_color: Some([0x20, 0x20, 0x20]),
        font_face_name: Some("Segoe UI".to_string()),
        ..Default::default()
    });
    for (start, length, style) in spans {
        content.set_selection(start..start + length);
        content.set_char_format(&span_format(style));
    }
    content.set_selection(0..0);
    content.scroll(-100_000);
}

fn span_format(style: LineStyle) -> nwg::CharFormat {
    match style {
        LineStyle::H1 => nwg::CharFormat {
            effects: Some(nwg::CharEffects::BOLD),
            height: Some(H1_TWIPS),
            ..Default::default()
        },
        LineStyle::H2 => nwg::CharFormat {
            effects: Some(nwg::CharEffects::BOLD),
            height: Some(H2_TWIPS),
            text_color: Some(H2_COLOR),
            ..Default::default()
        },
        LineStyle::Warning => nwg::CharFormat {
            text_color: Some(WARNING_COLOR),
            ..Default::default()
        },
        LineStyle::Example => nwg::CharFormat {
            height: Some(EXAMPLE_TWIPS),
            text_color: Some(EXAMPLE_COLOR),
            font_face_name: Some("Consolas".to_string()),
            ..Default::default()
        },
        LineStyle::Body => nwg::CharFormat::default(),
    }
}

/// Inner left/right padding so the text does not touch the control frame.
fn set_text_margins(content: &nwg::RichTextBox, margin: i32) {
    use winapi::um::winuser::{SendMessageW, EM_SETMARGINS, EC_LEFTMARGIN, EC_RIGHTMARGIN};
    let Some(hwnd) = content.handle.hwnd() else { return };
    let scaled = (margin as f64 * nwg::scale_factor()) as isize;
    let packed = (scaled & 0xFFFF) | ((scaled & 0xFFFF) << 16);
    unsafe {
        SendMessageW(hwnd, EM_SETMARGINS as u32, (EC_LEFTMARGIN | EC_RIGHTMARGIN) as usize, packed);
    }
}
