#![windows_subsystem = "windows"]

mod download;
mod engine;
mod gui;
mod i18n;
mod installed;
mod model;
mod pathenv;
mod paths;
mod registry;
mod settings;
mod single_instance;
mod update_check;

use std::fs;
use std::path::PathBuf;
use std::process::exit;
use std::sync::atomic::AtomicBool;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let flag = |name: &str| args.iter().position(|a| a == name);

    // Verification/data flags write a file and exit 0.
    if let Some(pos) = flag("--dump-catalog") {
        let out = args.get(pos + 1).cloned().unwrap_or_else(|| "foundry32-catalog.txt".into());
        run_dump_catalog(&out);
        return;
    }
    if let Some(pos) = flag("--check-update") {
        let out = args.get(pos + 1).cloned().unwrap_or_else(|| "foundry32-update.txt".into());
        run_check_update(&out);
        return;
    }
    if let Some(pos) = flag("--dump-path") {
        let out = args.get(pos + 1).cloned().unwrap_or_else(|| "foundry32-path.txt".into());
        run_dump_path(&out);
        return;
    }
    // Action flags exit 0 on success, non-zero on failure (no console output in
    // the windows subsystem — the exit code is the signal for verification).
    if let Some(pos) = flag("--install") {
        run_install(args.get(pos + 1).cloned());
    }
    if let Some(pos) = flag("--install-local") {
        run_install_local(args.get(pos + 1).cloned(), args.get(pos + 2).cloned());
    }
    if let Some(pos) = flag("--uninstall") {
        run_uninstall(args.get(pos + 1).cloned());
    }
    if let Some(pos) = flag("--run-tool") {
        run_tool(args.get(pos + 1).cloned());
    }
    if let Some(pos) = flag("--set-lang") {
        run_set_lang(args.get(pos + 1).cloned());
    }

    // GUI mode: enforce a single instance, then open the hub.
    let _guard = match single_instance::acquire() {
        Some(guard) => guard,
        None => {
            single_instance::notify_already_running();
            return;
        }
    };
    paths::sweep_stale_tmp();
    gui::run(settings::AppSettings::load());
}

fn catalog_entry(id: &str) -> Option<registry::ToolEntry> {
    let (catalog, _) = registry::load();
    catalog.tools.into_iter().find(|tool| tool.id == id)
}

/// Full, field-by-field dump of the merged catalog+installed view.
fn run_dump_catalog(out_path: &str) {
    let (catalog, source) = registry::load();
    let mut installed = installed::InstalledState::load();
    installed.reconcile();
    let views = model::merge(&catalog, &installed);

    let mut report = format!(
        "source: {source:?}  registry_version: {}  tools: {}\n\n",
        catalog.registry_version,
        views.len()
    );
    for view in &views {
        let e = &view.entry;
        let inst = view.installed.as_ref().map(|t| t.version.as_str()).unwrap_or("-");
        report.push_str(&format!(
            "id: {}\nname: {}\npublisher: {}\nversion: {} (installed: {})  status: {:?}\nexe: {}  size: {}\nurl: {}\nsha256: {}\nhomepage: {}\ndesc_en: {}\ndesc_pt: {}\nexpose_on_path: {}\ninstallable: {}\n",
            e.id, e.name, e.publisher, e.version, inst, view.status, e.exe, e.size_bytes,
            e.download_url, e.sha256, e.homepage, e.description_en, e.description_pt,
            e.expose_on_path, e.is_installable(),
        ));
        for companion in &e.companions {
            report.push_str(&format!(
                "companion: {}  size: {}\n  url: {}\n  sha256: {}\n",
                companion.exe, companion.size_bytes, companion.download_url, companion.sha256,
            ));
        }
        report.push('\n');
    }
    let _ = fs::write(out_path, report);
}

/// The user's PATH exactly as stored, plus which tool directories are on it —
/// the headless counterpart to the PATH exposure the installer performs.
fn run_dump_path(out_path: &str) {
    let report = match pathenv::read_raw() {
        Err(error) => format!("error {error}\n"),
        Ok(None) => "value: <absent>\n".to_string(),
        Ok(Some(value)) => {
            let text = String::from_utf16_lossy(
                &value
                    .bytes
                    .chunks_exact(2)
                    .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                    .take_while(|unit| *unit != 0)
                    .collect::<Vec<u16>>(),
            );
            let mut installed = installed::InstalledState::load();
            installed.reconcile();
            let exposed: Vec<String> = registry::load()
                .0
                .tools
                .iter()
                .filter(|tool| tool.expose_on_path && installed.get(&tool.id).is_some())
                .map(|tool| paths::tool_dir(&tool.id).display().to_string())
                .collect();
            format!("type: {:?}\nexposed: {}\nvalue: {}\n", value.vtype, exposed.join(", "), text)
        }
    };
    let _ = fs::write(out_path, report);
}

fn run_check_update(out_path: &str) {
    let line = match update_check::check_for_update() {
        Ok(Some(info)) => format!("update-available {} {}", info.latest_version, info.html_url),
        Ok(None) => format!("up-to-date {}", update_check::CURRENT_VERSION),
        Err(error) => format!("error {error}"),
    };
    let _ = fs::write(out_path, line);
}

fn run_install(id: Option<String>) {
    let Some(id) = id else { exit(2) };
    let Some(entry) = catalog_entry(&id) else { exit(3) };
    let cancel = AtomicBool::new(false);
    match engine::install(&entry, &cancel, |p| {
        // Consume progress so the fields are exercised even headless.
        let _ = (p.done, p.total);
    }) {
        Ok(()) => exit(0),
        Err(_) => exit(1),
    }
}

fn run_install_local(exe: Option<String>, id: Option<String>) {
    let (Some(exe), Some(id)) = (exe, id) else { exit(2) };
    let src = PathBuf::from(&exe);
    let (version, exe_name, expose_path) = match catalog_entry(&id) {
        Some(entry) => (entry.version, entry.exe, entry.expose_on_path),
        None => (
            "0.0.0-local".to_string(),
            src.file_name().and_then(|n| n.to_str()).unwrap_or("tool.exe").to_string(),
            false,
        ),
    };
    match engine::install_from_file(&id, &src, &version, &exe_name, expose_path) {
        Ok(()) => exit(0),
        Err(_) => exit(1),
    }
}

fn run_uninstall(id: Option<String>) {
    let Some(id) = id else { exit(2) };
    match engine::uninstall(&id) {
        Ok(()) => exit(0),
        Err(_) => exit(1),
    }
}

fn run_tool(id: Option<String>) {
    let Some(id) = id else { exit(2) };
    match engine::launch(&id) {
        Ok(()) => exit(0),
        Err(_) => exit(1),
    }
}

fn run_set_lang(code: Option<String>) {
    let lang = code.as_deref().and_then(foundry_common::Lang::from_code).unwrap_or(foundry_common::Lang::En);
    match (settings::AppSettings { lang }).save() {
        Ok(()) => exit(0),
        Err(_) => exit(1),
    }
}
