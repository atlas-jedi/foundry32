#![windows_subsystem = "windows"]

mod discovery;
mod gui;
mod i18n;
mod model;
mod mutation;
mod settings;
mod update_check;

use std::fs;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if let Some(position) = args.iter().position(|a| a == "--dump") {
        let out = args
            .get(position + 1)
            .cloned()
            .unwrap_or_else(|| "mcp-hangar-dump.txt".into());
        run_dump(&out);
        return;
    }
    if let Some(position) = args.iter().position(|a| a == "--check-update") {
        let out = args
            .get(position + 1)
            .cloned()
            .unwrap_or_else(|| "mcp-hangar-update.txt".into());
        run_check_update(&out);
        return;
    }
    gui::run();
}

/// Headless report of every discovered server — used for automated verification.
fn run_dump(out_path: &str) {
    let mut discovery = discovery::discover_file_servers();
    match discovery::cli::locate_claude_binary() {
        Some(claude) => match discovery::cli::run_mcp_list(&claude) {
            Ok(entries) => discovery::merge_cli_entries(&mut discovery, entries),
            Err(warning) => discovery.warnings.push(warning),
        },
        None => discovery.warnings.push("claude CLI not found".into()),
    }

    let mut report = String::new();
    for server in &discovery.servers {
        let scope = match &server.scope {
            model::Scope::Account => "ACCOUNT".to_string(),
            model::Scope::Plugin => "PLUGIN".to_string(),
            model::Scope::User => "USER".to_string(),
            model::Scope::Project { project_dir } => format!("PROJECT({project_dir})"),
            model::Scope::Local { project_dir } => format!("LOCAL({project_dir})"),
            model::Scope::Unknown => "UNKNOWN".to_string(),
        };
        let reach = match &server.scope {
            model::Scope::Account => "account-wide",
            model::Scope::Project { .. } => "repo-shared",
            model::Scope::Unknown => "unknown",
            _ => "machine-local",
        };
        report.push_str(&format!(
            "{} | {} | {} | {} | {} | {}\n",
            server.name,
            scope,
            reach,
            server.transport.label(),
            server.target,
            server.status.as_deref().unwrap_or("-")
        ));
    }
    for warning in &discovery.warnings {
        report.push_str(&format!("WARN: {warning}\n"));
    }
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
