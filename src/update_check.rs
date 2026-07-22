//! Checks the newest GitHub release and compares it with the running version.
//! Notify-only: the app never downloads or installs anything by itself.

pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const RELEASES_URL: &str = "https://github.com/atlas-jedi/mcp-hangar/releases/latest";
const LATEST_API: &str = "https://api.github.com/repos/atlas-jedi/mcp-hangar/releases/latest";

pub struct UpdateInfo {
    pub latest_version: String,
    pub html_url: String,
}

pub fn check_for_update() -> Result<Option<UpdateInfo>, String> {
    let response = minreq::get(LATEST_API)
        .with_header("User-Agent", concat!("mcp-hangar/", env!("CARGO_PKG_VERSION")))
        .with_header("Accept", "application/vnd.github+json")
        .with_timeout(10)
        .send()
        .map_err(|e| e.to_string())?;

    if response.status_code == 404 {
        return Ok(None); // no release published yet
    }
    if !(200..300).contains(&response.status_code) {
        return Err(format!("GitHub API status {}", response.status_code));
    }

    let body: serde_json::Value =
        serde_json::from_str(response.as_str().map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let tag = body["tag_name"].as_str().unwrap_or("");
    let html_url = body["html_url"].as_str().unwrap_or(RELEASES_URL);

    if is_newer(tag, CURRENT_VERSION) {
        Ok(Some(UpdateInfo {
            latest_version: tag.trim_start_matches('v').to_string(),
            html_url: html_url.to_string(),
        }))
    } else {
        Ok(None)
    }
}

fn parse_semver(version: &str) -> Option<(u64, u64, u64)> {
    let core = version.trim().trim_start_matches('v').split('-').next()?;
    let mut numbers = core.split('.');
    let major = numbers.next()?.parse().ok()?;
    let minor = numbers.next()?.parse().ok()?;
    let patch = numbers.next().unwrap_or("0").parse().ok()?;
    Some((major, minor, patch))
}

pub fn is_newer(candidate: &str, current: &str) -> bool {
    match (parse_semver(candidate), parse_semver(current)) {
        (Some(cand), Some(curr)) => cand > curr,
        _ => false,
    }
}
