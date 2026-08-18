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

use jafiz::model::{Run, RunFile, Suite};
// `Severity` is reached through `d.severity.label()`, never named — importing
// it would be an unused import, which `-D warnings` turns into an error.
use jafiz::parser::{EXAMPLE, GUIDE_EN, GUIDE_PT};
use jafiz::settings::AppSettings;
use jafiz::{report, store};

use foundry_common::lang::Lang;

/// Detaches a launched GUI from this console so the terminal returns at once.
const DETACHED_PROCESS: u32 = 0x0000_0008;

fn main() -> ExitCode {
    enable_utf8_console();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (dir, rest) = take_dir_flag(&args);
    let settings = AppSettings::load();
    let lang = settings.lang;

    match rest.first().map(String::as_str) {
        Some("list") => cmd_list(dir.as_deref(), lang),
        Some("show") => cmd_show(dir.as_deref(), rest.get(1).map(String::as_str), lang),
        Some("status") => cmd_status(dir.as_deref(), lang),
        Some("report") => cmd_report(dir.as_deref(), rest.get(1).map(String::as_str), &rest, lang),
        Some("check") => return cmd_check(rest.get(1).map(String::as_str)),
        Some("format") => cmd_format(lang),
        Some("new") => cmd_new(dir.as_deref(), rest.get(1).map(String::as_str)),
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

/// Pulls `--dir <path>` out of the argument list wherever it appears, so it
/// can precede or follow the command.
fn take_dir_flag(args: &[String]) -> (Option<PathBuf>, Vec<String>) {
    let mut dir = None;
    let mut rest = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--dir" {
            dir = iter.next().map(PathBuf::from);
        } else {
            rest.push(arg.clone());
        }
    }
    (dir, rest)
}

/// Loads a suite by name, or the only one present when no name is given.
/// Prints the reason and returns None when it cannot.
fn open_suite(dir: Option<&Path>, name: Option<&str>) -> Option<(PathBuf, Suite, RunFile)> {
    let location = store::resolve_location(dir);
    let path = match name {
        Some(name) => store::find_suite(&location.dir, name),
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
        println!("{}", report::list_row(&outcome.suite, runs.latest(), lang));
    }
}

fn cmd_show(dir: Option<&Path>, name: Option<&str>, lang: Lang) {
    let Some((_, suite, runs)) = open_suite(dir, name) else { return };
    print!("{}", report::show(&suite, runs.latest(), lang));
}

fn cmd_status(dir: Option<&Path>, lang: Lang) {
    let location = store::resolve_location(dir);
    let mut any = false;
    for path in store::list_suites(&location.dir) {
        let Ok(outcome) = store::load_suite(&path) else { continue };
        let runs = store::load_runs(&location.dir, &outcome.suite.stem);
        if runs.active().is_some() {
            println!("{}", report::status_line(&outcome.suite, runs.active(), lang));
            any = true;
        }
    }
    if !any {
        println!("Nenhuma execução em andamento em {}.", location.dir.display());
    }
}

fn cmd_report(dir: Option<&Path>, name: Option<&str>, args: &[String], lang: Lang) {
    // `--run <id>` is optional; without it the active or most recent run wins.
    let wanted = args.iter().position(|a| a == "--run").and_then(|i| args.get(i + 1));
    let name = name.filter(|n| !n.starts_with("--"));
    let Some((_, suite, runs)) = open_suite(dir, name) else { return };
    let run: Option<&Run> = match wanted {
        Some(id) => runs.run(id),
        None => runs.latest(),
    };
    print!("{}", report::report(&suite, run, lang));
}

/// Validates a suite file. Exit code 1 on error so a caller can gate on it.
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

fn cmd_new(dir: Option<&Path>, name: Option<&str>) {
    let Some(name) = name else {
        eprintln!("uso: jafiz new <nome>");
        return;
    };
    let location = store::resolve_location(dir);
    if let Err(e) = std::fs::create_dir_all(&location.dir) {
        eprintln!("jafiz: falha ao criar {}: {e}", location.dir.display());
        return;
    }
    let stem = name.trim().trim_end_matches(".md");
    let path = location.dir.join(format!("{stem}.md"));
    if path.exists() {
        eprintln!("jafiz: {} já existe", path.display());
        return;
    }
    // The example doubles as the skeleton: a new suite starts as a valid one.
    let body = EXAMPLE.replacen(
        "# Exemplo JAFIZ — todas as regras do formato",
        &format!("# {stem}"),
        1,
    );
    match std::fs::write(&path, body) {
        Ok(()) => println!("jafiz: criado {}", path.display()),
        Err(e) => eprintln!("jafiz: falha ao gravar {}: {e}", path.display()),
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
                for d in &outcome.diagnostics {
                    text.push_str(&format!("{}: {}\n", d.severity.label(), d.message));
                }
                text.push_str(&report::show(&outcome.suite, runs.latest(), lang));
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
