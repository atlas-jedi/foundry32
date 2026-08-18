//! `jafiz.exe` — the console front-end (goes on PATH).
//!
//! Read-mostly by design: the GUI records verdicts, this reads them back.
//! `report` is the command Claude runs to learn what failed; `check` is what
//! it runs to validate a suite it just wrote; `format` prints the contract.
//!
//! Commands: `list`, `show <suite>`, `status`, `report [suite]`,
//! `check <file>`, `format`, `new <name>`, plus `--dir`, `--dump`, `--help`.
//! With no arguments it opens the GUI, like `witn`.

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use jafiz::model::{Run, Suite};
// `Severity` is reached through `d.severity.label()`, never named — importing
// it would be an unused import, which `-D warnings` turns into an error.
use jafiz::parser::{EXAMPLE, GUIDE_EN, GUIDE_PT};
use jafiz::settings::AppSettings;
use jafiz::store::LoadedRuns;
use jafiz::{report, store};

use foundry_common::lang::Lang;

/// Detaches a launched GUI from this console so the terminal returns at once.
const DETACHED_PROCESS: u32 = 0x0000_0008;

fn main() -> ExitCode {
    enable_utf8_console();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (dir, dir_seen, rest) = take_flag(&args, "--dir");
    if dir_seen && dir.is_none() {
        eprintln!("jafiz: --dir precisa de um caminho (ex.: jafiz --dir C:\\projeto\\tests\\manual list)");
        return ExitCode::from(2);
    }
    let (run_id, run_seen, rest) = take_flag(&rest, "--run");
    if run_seen && run_id.is_none() {
        eprintln!("jafiz: --run precisa de um id de execução (veja jafiz list)");
        return ExitCode::from(2);
    }
    let dir = dir.map(PathBuf::from);
    let settings = AppSettings::load();
    let lang = settings.lang;

    match rest.first().map(String::as_str) {
        Some("list") => cmd_list(dir.as_deref(), lang),
        Some("show") => return cmd_show(dir.as_deref(), rest.get(1).map(String::as_str), lang),
        Some("status") => cmd_status(dir.as_deref(), lang),
        Some("report") => {
            return cmd_report(dir.as_deref(), rest.get(1).map(String::as_str), run_id.as_deref(), lang)
        }
        Some("check") => return cmd_check(rest.get(1).map(String::as_str)),
        Some("format") => cmd_format(lang),
        Some("new") => return cmd_new(dir.as_deref(), rest.get(1).map(String::as_str)),
        Some("--dump") => cmd_dump(dir.as_deref(), rest.get(1).map(String::as_str), lang),
        Some("--help") | Some("-h") => print_help(),
        None => launch_gui(),
        Some(other) => {
            eprintln!("jafiz: comando desconhecido '{other}'\n");
            print_help();
            return ExitCode::from(2);
        }
    }
    ExitCode::SUCCESS
}

/// Without this, `→` and the status symbols come out as mojibake in a console
/// still on a legacy code page (cmd.exe defaults to 850 in pt-BR Windows).
fn enable_utf8_console() {
    // SAFETY: SetConsoleOutputCP only sets the calling process's output page.
    unsafe { winapi::um::wincon::SetConsoleOutputCP(65001) };
}

/// Pulls a `--flag <value>` pair out of the argument list wherever it appears,
/// so a flag can precede or follow the command. Reports whether the flag was
/// seen at all, separately from whether it had a value — a flag written with
/// no value is a mistake worth naming, not something to swallow.
fn take_flag(args: &[String], flag: &str) -> (Option<String>, bool, Vec<String>) {
    let mut value = None;
    let mut seen = false;
    let mut rest = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == flag {
            seen = true;
            value = iter.next().cloned();
        } else {
            rest.push(arg.clone());
        }
    }
    (value, seen, rest)
}

/// Loads a suite by name, or the only one present when no name is given.
/// Prints the reason and returns None when it cannot.
fn open_suite(dir: Option<&Path>, name: Option<&str>) -> Option<(PathBuf, Suite, LoadedRuns)> {
    let location = store::resolve_location(dir);
    let path = match name {
        Some(name) => {
            let matches = store::match_suites(&location.dir, name);
            if matches.len() > 1 {
                eprintln!("jafiz: '{name}' casa com {} suítes:", matches.len());
                for path in &matches {
                    eprintln!("  {}", path.file_stem().unwrap_or_default().to_string_lossy());
                }
                eprintln!("informe o nome completo.");
                return None;
            }
            if matches.is_empty() {
                eprintln!(
                    "jafiz: nenhuma suíte chamada '{name}' em {} (veja jafiz list)",
                    location.dir.display()
                );
                return None;
            }
            matches.into_iter().next()
        }
        None => {
            let suites = store::list_suites(&location.dir);
            match suites.len() {
                1 => suites.into_iter().next(),
                0 => None,
                _ => {
                    eprintln!(
                        "jafiz: há {} suítes em {} — informe qual (jafiz list)",
                        suites.len(),
                        location.dir.display()
                    );
                    return None;
                }
            }
        }
    };
    let Some(path) = path else {
        eprintln!("jafiz: nenhuma suíte encontrada em {}", location.dir.display());
        return None;
    };
    let outcome = match store::load_suite(&path) {
        Ok(outcome) => outcome,
        Err(e) => {
            eprintln!("jafiz: falha ao ler {}: {e}", path.display());
            return None;
        }
    };
    let runs = store::load_runs(&location.dir, &outcome.suite.stem);
    Some((location.dir, outcome.suite, runs))
}

fn cmd_list(dir: Option<&Path>, lang: Lang) {
    let location = store::resolve_location(dir);
    println!("{} ({})", location.dir.display(), location.kind.label());
    let suites = store::list_suites(&location.dir);
    if suites.is_empty() {
        println!("Nenhuma suíte aqui. Crie uma com: jafiz new <nome>");
        return;
    }
    for path in suites {
        let Ok(outcome) = store::load_suite(&path) else { continue };
        let runs = store::load_runs(&location.dir, &outcome.suite.stem);
        // Listing every suite is not a verdict on any one of them, so a damaged
        // history is named on stderr and the row still prints — now next to the
        // reason it says no run was recorded.
        report_damage(&runs, lang);
        println!("{}", report::list_row(&outcome.suite, runs.file.latest(), lang));
    }
}

fn cmd_show(dir: Option<&Path>, name: Option<&str>, lang: Lang) -> ExitCode {
    let Some((_, suite, runs)) = open_suite(dir, name) else { return ExitCode::from(2) };
    if report_damage(&runs, lang) {
        return ExitCode::from(2);
    }
    print!("{}", report::show(&suite, runs.file.latest(), lang));
    ExitCode::SUCCESS
}

/// Says on stderr that a suite's run history exists but could not be read, and
/// reports whether that happened. The reading commands refuse rather than
/// print a report over an empty history: "nenhuma execução registrada" for a
/// damaged file is byte-identical to a suite nobody ever ran, and this is the
/// output Claude believes. The same reasoning already refuses an unknown
/// `--run` id.
fn report_damage(runs: &LoadedRuns, lang: Lang) -> bool {
    let Some(reason) = runs.damage.as_deref() else { return false };
    eprintln!("jafiz: {}", report::damaged_runs_line(reason, lang));
    true
}

fn cmd_status(dir: Option<&Path>, lang: Lang) {
    let location = store::resolve_location(dir);
    let mut any = false;
    for path in store::list_suites(&location.dir) {
        let Ok(outcome) = store::load_suite(&path) else { continue };
        let runs = store::load_runs(&location.dir, &outcome.suite.stem);
        report_damage(&runs, lang);
        if runs.file.active().is_some() {
            println!("{}", report::status_line(&outcome.suite, runs.file.active(), lang));
            any = true;
        }
    }
    if !any {
        println!("Nenhuma execução em andamento em {}.", location.dir.display());
    }
}

fn cmd_report(dir: Option<&Path>, name: Option<&str>, run_id: Option<&str>, lang: Lang) -> ExitCode {
    let Some((_, suite, runs)) = open_suite(dir, name) else { return ExitCode::from(2) };
    if report_damage(&runs, lang) {
        return ExitCode::from(2);
    }
    let runs = &runs.file;
    let run: Option<&Run> = match run_id {
        Some(id) => match runs.run(id) {
            Some(run) => Some(run),
            None => {
                // Rendering "nenhuma execução registrada" for a typo'd id would
                // assert something false about the user's testing — and this is
                // the command Claude reads to decide what to fix.
                eprintln!("jafiz: a suíte '{}' não tem execução com id '{id}'", suite.stem);
                if runs.runs.is_empty() {
                    eprintln!("  (nenhuma execução registrada)");
                } else {
                    let ids: Vec<&str> = runs.runs.iter().map(|r| r.id.as_str()).collect();
                    eprintln!("  execuções: {}", ids.join(", "));
                }
                return ExitCode::from(2);
            }
        },
        None => runs.latest(),
    };
    print!("{}", report::report(&suite, run, lang));
    ExitCode::SUCCESS
}

/// Validates a suite file. Exit 1 means it parsed but has diagnostic errors;
/// exit 2 covers bad usage and a file that could not be read at all — a
/// caller gating on "did the suite pass" can treat anything nonzero as no.
fn cmd_check(path: Option<&str>) -> ExitCode {
    let Some(path) = path else {
        eprintln!("uso: jafiz check <arquivo.md>");
        return ExitCode::from(2);
    };
    let path = Path::new(path);
    let outcome = match store::load_suite(path) {
        Ok(outcome) => outcome,
        Err(e) => {
            eprintln!("jafiz: falha ao ler {}: {e}", path.display());
            return ExitCode::from(2);
        }
    };
    for d in &outcome.diagnostics {
        let where_at = if d.line > 0 { format!("linha {}", d.line) } else { "arquivo".into() };
        println!("{}: {where_at}: {}", d.severity.label(), d.message);
    }
    let scenarios = outcome.suite.scenarios.len();
    let steps = outcome.suite.total_steps();
    println!("{}: {scenarios} cenários, {steps} passos", outcome.suite.title);
    if outcome.has_errors() {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

fn cmd_format(lang: Lang) {
    print!("{}", if matches!(lang, Lang::En) { GUIDE_EN } else { GUIDE_PT });
}

fn cmd_new(dir: Option<&Path>, name: Option<&str>) -> ExitCode {
    let Some(name) = name else {
        eprintln!("uso: jafiz new <nome>");
        return ExitCode::from(2);
    };
    let location = store::resolve_location(dir);
    if let Err(e) = std::fs::create_dir_all(&location.dir) {
        eprintln!("jafiz: falha ao criar {}: {e}", location.dir.display());
        return ExitCode::from(2);
    }
    let stem = name.trim().trim_end_matches(".md").trim();
    // `new` is the CLI's only mutating command, and this is its only argument
    // that becomes a path — a separator or `..` would place the file outside
    // the resolved location entirely.
    if stem.is_empty() {
        eprintln!("jafiz: o nome da suíte não pode ser vazio");
        return ExitCode::from(2);
    }
    if stem.contains(['/', '\\', ':']) || stem.contains("..") {
        eprintln!("jafiz: o nome da suíte não pode conter caminho ('{stem}')");
        return ExitCode::from(2);
    }
    let path = location.dir.join(format!("{stem}.md"));
    if path.exists() {
        eprintln!("jafiz: {} já existe", path.display());
        return ExitCode::from(2);
    }
    // The example doubles as the skeleton: a new suite starts as a valid one.
    let body = EXAMPLE.replacen(
        "# Exemplo JAFIZ — todas as regras do formato",
        &format!("# {stem}"),
        1,
    );
    match std::fs::write(&path, body) {
        Ok(()) => {
            println!("jafiz: criado {}", path.display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("jafiz: falha ao gravar {}: {e}", path.display());
            ExitCode::from(2)
        }
    }
}

/// Headless verification dump (project convention): the resolved location,
/// every suite, and each one's latest run rendered as a report.
fn cmd_dump(dir: Option<&Path>, out: Option<&str>, lang: Lang) {
    let out_path = out.unwrap_or("jafiz-dump.txt");
    let location = store::resolve_location(dir);
    let mut text = format!("location: {} ({})\n", location.dir.display(), location.kind.label());
    for path in store::list_suites(&location.dir) {
        match store::load_suite(&path) {
            Ok(outcome) => {
                let runs = store::load_runs(&location.dir, &outcome.suite.stem);
                text.push_str(&format!("\n=== {} ===\n", path.display()));
                if let Some(reason) = runs.damage.as_deref() {
                    text.push_str(&report::damaged_runs_line(reason, lang));
                    text.push('\n');
                }
                for d in &outcome.diagnostics {
                    let where_at = if d.line > 0 { format!("linha {}", d.line) } else { "arquivo".into() };
                    text.push_str(&format!("{}: {where_at}: {}\n", d.severity.label(), d.message));
                }
                text.push_str(&report::show(&outcome.suite, runs.file.latest(), lang));
            }
            Err(e) => text.push_str(&format!("\n=== {} === ERRO: {e}\n", path.display())),
        }
    }
    match std::fs::write(out_path, &text) {
        Ok(()) => println!("jafiz: dump gravado em {out_path}"),
        Err(e) => eprintln!("jafiz: falha ao gravar {out_path}: {e}"),
    }
}

/// `jafiz` with no arguments opens the GUI, which lives next to this exe.
fn launch_gui() {
    let gui = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("jafiz-gui.exe")));
    match gui {
        Some(path) if path.exists() => {
            let _ = std::process::Command::new(&path)
                .creation_flags(DETACHED_PROCESS)
                .spawn();
        }
        _ => print_help(),
    }
}

fn print_help() {
    println!("JAFIZ — já fiz?  (v{})", env!("CARGO_PKG_VERSION"));
    println!();
    println!("USO:");
    println!("  jafiz list                    suítes do local atual, com o estado da última execução");
    println!("  jafiz show <suíte>            a suíte inteira, cada passo com seu status");
    println!("  jafiz status                  o que está sendo testado agora");
    println!("  jafiz report [suíte] [--run <id>]   relatório compacto (o que o Claude lê)");
    println!("  jafiz check <arquivo.md>      valida o formato (sai 1 se houver erro)");
    println!("  jafiz format                  imprime o contrato do formato");
    println!("  jafiz new <nome>              cria uma suíte esqueleto");
    println!("  jafiz --dir <pasta>           usa esta pasta em vez do local resolvido");
    println!("  jafiz --dump [arq]            verificação headless");
    println!("  jafiz --help | -h             esta ajuda");
    println!();
    println!("Sem argumentos, o jafiz abre a interface gráfica.");
    println!("Local: --dir > JAFIZ_DIR > tests\\manual (subindo do diretório atual) > biblioteca.");
}
