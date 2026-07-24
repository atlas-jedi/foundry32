//! `witn.exe` — the console front-end (goes on PATH).
//!
//! Commands: `list`, `port <porta>`, `kill <porta | --pid PID> [-y]`, plus
//! `--dump` / `--help`. Running with no args will open the GUI once
//! `witn-gui.exe` exists (Phase 3); for now it prints help.

use std::io::Write;
use std::os::windows::process::CommandExt;

use witn::model::{format_uptime, NodeProc};
use witn::{proctree, tree, Scanner};

/// Detaches a launched GUI from this console so the terminal returns immediately.
const DETACHED_PROCESS: u32 = 0x0000_0008;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("list") => cmd_list(),
        Some("port") => cmd_port(&args[2..]),
        Some("kill") => cmd_kill(&args[2..]),
        Some("--dump") => cmd_dump(args.get(2).map(String::as_str)),
        Some("--help") | Some("-h") => print_help(),
        None => launch_gui(),
        Some(other) => {
            eprintln!("witn: comando desconhecido '{other}'\n");
            print_help();
        }
    }
}

/// Two samples ~400 ms apart so the second carries a real CPU% reading.
fn scan_twice() -> Vec<NodeProc> {
    let mut scanner = Scanner::new();
    let _ = scanner.sample();
    std::thread::sleep(std::time::Duration::from_millis(400));
    scanner.sample()
}

fn cmd_list() {
    let forest = tree::build(scan_twice());
    if forest.is_empty() {
        println!("Nenhum processo node.exe em execução.");
        return;
    }
    println!(
        "{:<32} {:>6} {:>6} {:<16} {:>5} {:>7} {:>8}  CAMINHO",
        "APP", "PID", "PPID", "PORTA(S)", "CPU", "RAM", "UPTIME"
    );
    for p in &forest {
        let name = format!("{}{}", "  ".repeat(p.depth), p.app_name);
        println!(
            "{:<32} {:>6} {:>6} {:<16} {:>4.0}% {:>5}MB {:>8}  {}",
            truncate(&name, 32),
            p.pid,
            p.ppid,
            truncate(&p.ports_label(), 16),
            p.cpu_percent,
            p.mem_mib(),
            format_uptime(p.uptime_secs),
            app_path(p)
        );
    }
}

fn cmd_port(rest: &[String]) {
    let Some(port) = first_number(rest) else {
        eprintln!("uso: witn port <porta>");
        return;
    };
    let forest = tree::build(scan_twice());
    let matches: Vec<&NodeProc> = forest.iter().filter(|p| p.ports.contains(&port)).collect();
    if matches.is_empty() {
        println!("Nenhum processo node escutando na porta {port}.");
        return;
    }
    for p in matches {
        print_detail(p);
    }
}

fn print_detail(p: &NodeProc) {
    println!("{}  (PID {})", p.app_name, p.pid);
    println!("  Porta(s): {}", p.ports_label());
    println!("  Caminho:  {}", app_path(p));
    println!(
        "  RAM: {} MB    CPU: {:.0}%    Uptime: {}    PID pai: {}",
        p.mem_mib(),
        p.cpu_percent,
        format_uptime(p.uptime_secs),
        p.ppid
    );
}

enum Target {
    Pid(u32),
    Port(u16),
}

fn cmd_kill(rest: &[String]) {
    let yes = rest.iter().any(|a| a == "-y" || a == "--yes");
    let target = if let Some(i) = rest.iter().position(|a| a == "--pid") {
        rest.get(i + 1)
            .and_then(|s| s.parse::<u32>().ok())
            .map(Target::Pid)
    } else {
        first_number(rest).map(Target::Port)
    };
    let Some(target) = target else {
        eprintln!("uso: witn kill <porta> | witn kill --pid <PID>   [-y|--yes]");
        return;
    };

    let target_pid = match target {
        Target::Pid(pid) => pid,
        Target::Port(port) => match resolve_port(port) {
            Some(pid) => pid,
            None => {
                println!("Nenhum processo escutando na porta {port}.");
                return;
            }
        },
    };

    let all = proctree::all_processes();
    let subtree = proctree::subtree(&all, target_pid);
    if subtree.is_empty() {
        println!("Processo {target_pid} não encontrado.");
        return;
    }

    println!(
        "Encerrar {} e sua árvore — {} processo(s):",
        target_label(target_pid),
        subtree.len()
    );
    for entry in &subtree {
        println!("  PID {:>6}  {}", entry.pid, entry.exe_name);
    }

    if !yes && !confirm() {
        println!("Cancelado.");
        return;
    }

    // Children-first (reverse of the breadth-first subtree) so a parent can't
    // re-spawn a worker we already listed.
    let (mut killed, mut failed) = (0u32, 0u32);
    for entry in subtree.iter().rev() {
        match proctree::terminate(entry.pid) {
            Ok(()) => killed += 1,
            Err(_) => failed += 1,
        }
    }
    println!("Encerrados {killed} processo(s).");
    if failed > 0 {
        println!("{failed} não puderam ser encerrados (acesso negado / já saíram).");
    }
}

fn confirm() -> bool {
    print!("Confirmar encerramento? [s/N] ");
    let _ = std::io::stdout().flush();
    let mut input = String::new();
    if std::io::stdin().read_line(&mut input).is_err() {
        return false;
    }
    matches!(input.trim().to_lowercase().as_str(), "s" | "sim" | "y" | "yes")
}

fn cmd_dump(path: Option<&str>) {
    let out_path = path.unwrap_or("witn-dump.txt");
    let forest = tree::build(scan_twice());
    let mut report = String::new();
    for p in &forest {
        report.push_str(&format!(
            "{}{} | pid={} ppid={} ports={} cpu={:.0}% mem={}MB up={} path={}\n",
            "  ".repeat(p.depth),
            p.app_name,
            p.pid,
            p.ppid,
            p.ports_label(),
            p.cpu_percent,
            p.mem_mib(),
            format_uptime(p.uptime_secs),
            app_path(p)
        ));
    }
    match std::fs::write(out_path, &report) {
        Ok(()) => println!("witn: dump gravado em {out_path} ({} processos)", forest.len()),
        Err(e) => eprintln!("witn: falha ao gravar {out_path}: {e}"),
    }
}

/// `witn` with no arguments opens the GUI. `witn-gui.exe` lives next to this
/// exe (same tool dir); launch it detached so the terminal isn't held, and fall
/// back to help if it isn't there (e.g. running the console exe in isolation).
fn launch_gui() {
    let gui = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("witn-gui.exe")));
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
    println!("WITN — Where Is The Node?  (v{})", env!("CARGO_PKG_VERSION"));
    println!();
    println!("USO:");
    println!("  witn list                    lista os node.exe (árvore, portas, CPU, RAM, uptime)");
    println!("  witn port <porta>            mostra quem está escutando a porta");
    println!("  witn kill <porta> [-y]       encerra quem escuta a porta, e sua árvore (confirma; -y pula)");
    println!("  witn kill --pid <PID> [-y]   encerra por PID, e sua árvore");
    println!("  witn --dump [arq]            grava a lista num arquivo (verificação headless)");
    println!("  witn --help | -h             esta ajuda");
    println!();
    println!("Sem argumentos, o witn abrirá a interface gráfica (em breve).");
}

/// The app's on-disk location (script/project), never the shared node.exe path.
fn app_path(p: &NodeProc) -> String {
    witn::appname::app_location(p.cmdline.as_deref(), p.cwd.as_deref())
        .or_else(|| p.exe_path.clone())
        .map(|x| x.display().to_string())
        .unwrap_or_else(|| "—".to_string())
}

/// First non-flag argument parsed as a port number.
fn first_number(rest: &[String]) -> Option<u16> {
    rest.iter()
        .find(|a| !a.starts_with('-'))
        .and_then(|s| s.parse::<u16>().ok())
}

fn resolve_port(port: u16) -> Option<u32> {
    witn::ports::listening_by_pid()
        .into_iter()
        .find(|(_, ports)| ports.contains(&port))
        .map(|(pid, _)| pid)
}

fn target_label(pid: u32) -> String {
    let mut scanner = Scanner::new();
    match scanner.sample().into_iter().find(|p| p.pid == pid) {
        Some(p) => format!("{} (PID {pid})", p.app_name),
        None => format!("PID {pid}"),
    }
}

/// Truncates by character count (not bytes — names carry `·`/`…`), adding `…`.
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut t: String = s.chars().take(max.saturating_sub(1)).collect();
    t.push('…');
    t
}
