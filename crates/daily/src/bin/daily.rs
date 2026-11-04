//! `daily.exe` — the console front-end (goes on PATH).
//!
//! What Claude runs: `report` to read a day back grouped by task, `style` to
//! see how the user writes before writing anything, `tasks` to reuse the
//! user's own task labels, and `add` / `edit` / `rm` to write. Everything it
//! writes goes through the engine's `edit`, the same path the window uses.
//! With no arguments it opens the GUI, like `jafiz` and `witn`.

use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::ExitCode;

use daily::edit::{self, EntryChange, NewEntry};
use daily::settings::AppSettings;
use daily::{report, store, Author, Date};

use foundry_common::lang::Lang;

/// Detaches a launched GUI from this console so the terminal returns at once.
const DETACHED_PROCESS: u32 = 0x0000_0008;

/// Flags that take a value, and flags that stand alone.
const VALUE_FLAGS: [&str; 8] = ["--date", "--task", "--time", "--by", "--text", "--days", "--examples", "--out"];
const SWITCHES: [&str; 3] = ["--ontem", "--yesterday", "--json"];

const DEFAULT_TASK_DAYS: i64 = 14;
const DEFAULT_EXAMPLES: usize = 25;

fn main() -> ExitCode {
    enable_utf8_console();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(command) = args.first().cloned() else {
        launch_gui();
        return ExitCode::SUCCESS;
    };
    if matches!(command.as_str(), "--help" | "-h" | "help") {
        print_help();
        return ExitCode::SUCCESS;
    }
    let parsed = match Parsed::from(&args[1..]) {
        Ok(parsed) => parsed,
        Err(message) => return usage_error(&message),
    };
    let lang = AppSettings::load().lang;
    let dir = store::data_dir();
    match command.as_str() {
        "add" => cmd_add(&dir, &parsed, lang),
        "list" => cmd_list(&dir, &parsed, lang),
        "report" => cmd_report(&dir, &parsed, lang),
        "edit" => cmd_edit(&dir, &parsed, lang),
        "rm" => cmd_rm(&dir, &parsed, lang),
        "tasks" => cmd_tasks(&dir, &parsed),
        "style" => cmd_style(&dir, &parsed, lang),
        "--dump" => cmd_dump(&dir, &parsed, lang),
        other => usage_error(&format!("comando desconhecido '{other}'")),
    }
}

/// Without this, `·` and accented text come out as mojibake in a console still
/// on a legacy code page (cmd.exe defaults to 850 in pt-BR Windows).
fn enable_utf8_console() {
    // SAFETY: SetConsoleOutputCP only sets the calling process's output page.
    unsafe { winapi::um::wincon::SetConsoleOutputCP(65001) };
}

/// The arguments after the command: positional words, `--flag value` pairs and
/// bare switches, in any order.
struct Parsed {
    positional: Vec<String>,
    values: Vec<(String, String)>,
    switches: Vec<String>,
}

impl Parsed {
    fn from(args: &[String]) -> Result<Parsed, String> {
        let mut parsed = Parsed { positional: Vec::new(), values: Vec::new(), switches: Vec::new() };
        let mut iter = args.iter();
        while let Some(arg) = iter.next() {
            if VALUE_FLAGS.contains(&arg.as_str()) {
                let value = iter.next().ok_or_else(|| format!("{arg} precisa de um valor"))?;
                parsed.values.push((arg.clone(), value.clone()));
            } else if SWITCHES.contains(&arg.as_str()) {
                parsed.switches.push(arg.clone());
            } else if arg.starts_with("--") {
                return Err(format!("opção desconhecida '{arg}'"));
            } else {
                parsed.positional.push(arg.clone());
            }
        }
        Ok(parsed)
    }

    fn value(&self, flag: &str) -> Option<&str> {
        self.values.iter().rev().find(|(name, _)| name == flag).map(|(_, value)| value.as_str())
    }

    fn has(&self, switch: &str) -> bool {
        self.switches.iter().any(|name| name == switch)
    }

    fn wants_yesterday(&self) -> bool {
        self.has("--ontem") || self.has("--yesterday")
    }

    /// The day a read command is about: `--date`, `--ontem` (the last day with
    /// entries before today), or today.
    fn day(&self, dir: &Path) -> Result<Date, String> {
        if let Some(raw) = self.value("--date") {
            return Date::parse(raw).ok_or_else(|| format!("data inválida: {raw} (use AAAA-MM-DD)"));
        }
        let today = Date::today();
        if !self.wants_yesterday() {
            return Ok(today);
        }
        store::previous_day_with_entries(dir, today)
            .ok_or_else(|| "nenhum dia anterior a hoje tem anotações".to_string())
    }

    fn author(&self) -> Result<Author, String> {
        match self.value("--by") {
            None => Ok(Author::Claude),
            Some(raw) => Author::from_code(raw).ok_or_else(|| format!("--by aceita user ou claude, não '{raw}'")),
        }
    }

    fn number(&self, flag: &str, default: usize) -> Result<usize, String> {
        match self.value(flag) {
            None => Ok(default),
            Some(raw) => raw.parse::<usize>().map_err(|_| format!("{flag} precisa de um número, não '{raw}'")),
        }
    }
}

/// A command written wrong. One line and a pointer to the help, not the whole
/// help: the reader is often Claude, and the mistake is what it needs to see.
fn usage_error(message: &str) -> ExitCode {
    eprintln!("daily: {message}");
    eprintln!("(daily --help mostra todos os comandos)");
    ExitCode::from(2)
}

fn failure(message: &str) -> ExitCode {
    eprintln!("daily: {message}");
    ExitCode::from(1)
}

fn cmd_add(dir: &Path, parsed: &Parsed, lang: Lang) -> ExitCode {
    let text = parsed.positional.join(" ");
    if text.trim().is_empty() {
        return usage_error("daily add precisa do texto: daily add \"o que foi feito\" [--task <rótulo>]");
    }
    let date = match parsed.value("--date") {
        Some(raw) => match Date::parse(raw) {
            Some(date) => date,
            None => return usage_error(&format!("data inválida: {raw} (use AAAA-MM-DD)")),
        },
        None => Date::today(),
    };
    let author = match parsed.author() {
        Ok(author) => author,
        Err(message) => return usage_error(&message),
    };
    let new = NewEntry {
        date,
        time: parsed.value("--time").map(str::to_string),
        task: parsed.value("--task").unwrap_or_default().to_string(),
        text,
        author,
    };
    match edit::add_entry(dir, new) {
        Ok(entry) => {
            println!("anotado: {} · {} {}", entry.id, date.iso(), entry.time);
            ExitCode::SUCCESS
        }
        Err(error) => failure(&error.message(lang)),
    }
}

fn cmd_list(dir: &Path, parsed: &Parsed, lang: Lang) -> ExitCode {
    read_day(dir, parsed, lang, |day, date| {
        if parsed.has("--json") {
            pretty(&report::list_json(day, date))
        } else {
            report::render_list(day, date, lang)
        }
    })
}

fn cmd_report(dir: &Path, parsed: &Parsed, lang: Lang) -> ExitCode {
    read_day(dir, parsed, lang, |day, date| {
        if parsed.has("--json") {
            pretty(&report::report_json(day, date))
        } else {
            report::render_day(day, date, lang)
        }
    })
}

/// Shared by the read commands: resolve the day, load it, print what `render`
/// makes of it. A damaged file still prints (as an empty day) but exits 1, so
/// whoever reads the output — usually Claude — cannot take it for an empty day.
fn read_day(dir: &Path, parsed: &Parsed, lang: Lang, render: impl Fn(&daily::Day, Date) -> String) -> ExitCode {
    let date = match parsed.day(dir) {
        Ok(date) => date,
        Err(message) => return failure(&message),
    };
    let loaded = store::load_day(dir, date);
    print!("{}", ensure_newline(render(&loaded.day, date)));
    match loaded.damage {
        Some(reason) => failure(&report::damaged_line(&reason, lang)),
        None => ExitCode::SUCCESS,
    }
}

fn cmd_edit(dir: &Path, parsed: &Parsed, lang: Lang) -> ExitCode {
    let Some(id) = parsed.positional.first() else {
        return usage_error("daily edit precisa do id (veja daily list)");
    };
    let author = match parsed.author() {
        Ok(author) => author,
        Err(message) => return usage_error(&message),
    };
    let change = EntryChange {
        text: parsed.value("--text").map(str::to_string),
        task: parsed.value("--task").map(str::to_string),
        time: parsed.value("--time").map(str::to_string),
        author,
    };
    if change.text.is_none() && change.task.is_none() && change.time.is_none() {
        return usage_error("daily edit precisa de --text, --task ou --time");
    }
    match edit::update_entry(dir, id, change) {
        Ok(entry) => {
            println!("editado: {} · {} [{}] {}", entry.id, entry.time, entry.task, entry.text);
            ExitCode::SUCCESS
        }
        Err(error) => failure(&error.message(lang)),
    }
}

fn cmd_rm(dir: &Path, parsed: &Parsed, lang: Lang) -> ExitCode {
    let Some(id) = parsed.positional.first() else {
        return usage_error("daily rm precisa do id (veja daily list)");
    };
    match edit::remove_entry(dir, id) {
        Ok(entry) => {
            println!("removido: {} · {}", entry.id, entry.text);
            ExitCode::SUCCESS
        }
        Err(error) => failure(&error.message(lang)),
    }
}

fn cmd_tasks(dir: &Path, parsed: &Parsed) -> ExitCode {
    let days = match parsed.number("--days", DEFAULT_TASK_DAYS as usize) {
        Ok(days) => days as i64,
        Err(message) => return usage_error(&message),
    };
    for label in store::recent_tasks(dir, Date::today(), days) {
        println!("{label}");
    }
    ExitCode::SUCCESS
}

fn cmd_style(dir: &Path, parsed: &Parsed, lang: Lang) -> ExitCode {
    let limit = match parsed.number("--examples", DEFAULT_EXAMPLES) {
        Ok(limit) => limit,
        Err(message) => return usage_error(&message),
    };
    print!("{}", report::render_style(&store::recent_user_entries(dir, limit), lang));
    ExitCode::SUCCESS
}

/// Headless verification dump (project convention): everything the tool would
/// show about today and the day before, written to a file.
fn cmd_dump(dir: &Path, parsed: &Parsed, lang: Lang) -> ExitCode {
    let out_path = parsed.value("--out").or(parsed.positional.first().map(String::as_str)).unwrap_or("daily-dump.txt");
    let today = Date::today();
    let source = if std::env::var(store::DIR_ENV).is_ok_and(|v| !v.trim().is_empty()) { store::DIR_ENV } else { "padrão" };
    let mut text = format!("DAILY {}\ndata dir: {} ({source})\n", env!("CARGO_PKG_VERSION"), dir.display());
    text.push_str(&format!("dias com arquivo: {}\n", store::list_days(dir).len()));
    let mut days = vec![today];
    if let Some(previous) = store::previous_day_with_entries(dir, today) {
        text.push_str(&format!("ontem (último dia com anotações): {}\n", previous.iso()));
        days.push(previous);
    }
    for date in days {
        let loaded = store::load_day(dir, date);
        text.push_str(&format!("\n=== {} ===\n", date.iso()));
        if let Some(reason) = loaded.damage.as_deref() {
            text.push_str(&report::damaged_line(reason, lang));
            text.push('\n');
        }
        text.push_str(&report::render_list(&loaded.day, date, lang));
        text.push('\n');
        text.push_str(&report::render_day(&loaded.day, date, lang));
    }
    text.push_str("\n=== tasks recentes ===\n");
    for label in store::recent_tasks(dir, today, DEFAULT_TASK_DAYS) {
        text.push_str(&format!("{label}\n"));
    }
    text.push_str("\n=== style ===\n");
    text.push_str(&report::render_style(&store::recent_user_entries(dir, 5), lang));
    match std::fs::write(out_path, &text) {
        Ok(()) => {
            println!("daily: dump gravado em {out_path}");
            ExitCode::SUCCESS
        }
        Err(e) => failure(&format!("falha ao gravar {out_path}: {e}")),
    }
}

fn pretty(value: &serde_json::Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_default()
}

fn ensure_newline(mut text: String) -> String {
    if !text.ends_with('\n') {
        text.push('\n');
    }
    text
}

/// `daily` with no arguments opens the GUI, which lives next to this exe.
fn launch_gui() {
    let gui = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("daily-gui.exe")));
    match gui {
        Some(path) if path.exists() => {
            let _ = std::process::Command::new(&path).creation_flags(DETACHED_PROCESS).spawn();
        }
        _ => print_help(),
    }
}

fn print_help() {
    println!("DAILY — o que fiz hoje  (v{})", env!("CARGO_PKG_VERSION"));
    println!();
    println!("USO:");
    println!("  daily add \"<texto>\" [--task <rótulo>] [--date AAAA-MM-DD] [--time HH:MM] [--by user|claude]");
    println!("                                 grava uma anotação (padrão --by claude)");
    println!("  daily list [--date D | --ontem] [--json]     anotações do dia, com o id de cada uma");
    println!("  daily report [--date D | --ontem] [--json]   o dia agrupado por task (o texto da daily)");
    println!("  daily edit <id> [--text …] [--task …] [--time HH:MM] [--by user|claude]");
    println!("  daily rm <id>                  apaga uma anotação");
    println!("  daily tasks [--days 14]        rótulos de task usados nos últimos dias");
    println!("  daily style [--examples 25]    as últimas anotações escritas na janela (o jeito de escrever)");
    println!("  daily --dump [arq]             verificação headless");
    println!("  daily --help | -h              esta ajuda");
    println!();
    println!("Sem argumentos, o daily abre a janela. --ontem é o último dia com anotações antes de hoje.");
    println!("Dados: DAILY_DIR, ou %LOCALAPPDATA%\\Software Imperial\\Foundry32\\daily\\days.");
}
