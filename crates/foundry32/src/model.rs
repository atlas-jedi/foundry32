//! Merges the catalog with installed state into the view the UI renders, and
//! the semver comparison that decides when an update is available.

use crate::installed::{InstalledState, InstalledTool};
use crate::registry::{Catalog, ToolEntry};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolStatus {
    NotInstalled,
    Installed,
    UpdateAvailable,
}

#[derive(Clone, Debug)]
pub struct ToolView {
    pub entry: ToolEntry,
    pub installed: Option<InstalledTool>,
    pub status: ToolStatus,
}

/// One view per catalog tool, in catalog order. An installed tool whose catalog
/// version is strictly newer than what's on disk is `UpdateAvailable`; equal or
/// older is `Installed` — a downgrade is never surfaced as an update.
pub fn merge(catalog: &Catalog, installed: &InstalledState) -> Vec<ToolView> {
    catalog
        .tools
        .iter()
        .map(|entry| {
            let inst = installed.get(&entry.id).cloned();
            let status = match &inst {
                None => ToolStatus::NotInstalled,
                Some(tool) if is_newer(&entry.version, &tool.version) => ToolStatus::UpdateAvailable,
                Some(_) => ToolStatus::Installed,
            };
            ToolView { entry: entry.clone(), installed: inst, status }
        })
        .collect()
}

fn parse_semver(version: &str) -> Option<(u64, u64, u64)> {
    let core = version.trim().trim_start_matches('v').split('-').next()?;
    let mut numbers = core.split('.');
    let major = numbers.next()?.parse().ok()?;
    let minor = numbers.next()?.parse().ok()?;
    let patch = numbers.next().unwrap_or("0").parse().ok()?;
    Some((major, minor, patch))
}

/// True if `candidate` is a strictly newer semver than `current`.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    match (parse_semver(candidate), parse_semver(current)) {
        (Some(cand), Some(curr)) => cand > curr,
        _ => false,
    }
}
