//! The tool catalog: schema, defensive parsing, and a three-tier load —
//! fetch the release asset, fall back to a last-known-good disk cache, then the
//! embedded copy. Includes anti-rollback (a stale catalog can't downgrade the
//! registry) and a host allow-list on download URLs.

use crate::paths;
use serde_json::Value;
use std::fs;

/// Where the hub fetches the live catalog: the latest release's asset, which is
/// always consistent with that release's tool binaries (and dodges the raw-CDN
/// staleness window). Override with `FOUNDRY32_REGISTRY_URL` for development.
const RELEASE_ASSET_URL: &str =
    "https://github.com/atlas-jedi/mcp-hangar/releases/latest/download/registry.json";

/// Embedded copy — offline / first-run fallback, regenerated at release time.
const EMBEDDED: &str = include_str!("../../../registry.json");

const SCHEMA: u32 = 1;
/// Hosts a `download_url` may point at. Matched as a suffix on the URL host so
/// `objects.githubusercontent.com` (GitHub's asset CDN) is covered too.
const ALLOWED_HOST_SUFFIXES: [&str; 2] = ["github.com", "githubusercontent.com"];

#[derive(Clone, Debug)]
pub struct ToolEntry {
    pub id: String,
    pub name: String,
    pub publisher: String,
    pub description_en: String,
    pub description_pt: String,
    pub version: String,
    pub exe: String,
    pub download_url: String,
    /// Lowercase 64-hex SHA-256, or empty when the registry hasn't been filled
    /// in yet (metadata-only, e.g. the embedded seed) — install is blocked then.
    pub sha256: String,
    pub size_bytes: u64,
    pub homepage: String,
}

impl ToolEntry {
    /// A tool can be installed only once its download is pinned to a verifiable
    /// hash. The embedded/offline catalog carries empty hashes for display only.
    pub fn is_installable(&self) -> bool {
        is_hex64(&self.sha256) && is_allowed_url(&self.download_url)
    }
}

#[derive(Clone, Debug, Default)]
pub struct Catalog {
    pub registry_version: u64,
    pub tools: Vec<ToolEntry>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// Fetched fresh from the release asset.
    Live,
    /// Served from the last-known-good disk cache.
    Cache,
    /// The binary's built-in seed (offline / first run).
    Embedded,
}

/// Loads the catalog, most-trusted source first: live fetch → disk cache →
/// embedded. A successful live fetch also refreshes the cache. Anti-rollback:
/// a source whose `registry_version` is lower than one we've already accepted
/// is discarded.
pub fn load() -> (Catalog, Source) {
    let url = std::env::var("FOUNDRY32_REGISTRY_URL").unwrap_or_else(|_| RELEASE_ASSET_URL.to_string());
    let cached = read_file(&paths::registry_cache_path()).and_then(|raw| parse(&raw).ok());
    let floor = cached.as_ref().map(|c| c.registry_version).unwrap_or(0);

    if let Ok(raw) = fetch(&url) {
        if let Ok(catalog) = parse(&raw) {
            if catalog.registry_version >= floor {
                let _ = write_cache(&raw);
                return (catalog, Source::Live);
            }
        }
    }
    if let Some(catalog) = cached {
        return (catalog, Source::Cache);
    }
    // parse() on the embedded seed is infallible in practice (it ships with us).
    (parse(EMBEDDED).unwrap_or_default(), Source::Embedded)
}

fn fetch(url: &str) -> Result<String, String> {
    let response = minreq::get(url)
        .with_header("User-Agent", concat!("foundry32/", env!("CARGO_PKG_VERSION")))
        .with_header("Accept", "application/json")
        .with_timeout(15)
        .send()
        .map_err(|e| e.to_string())?;
    if !(200..300).contains(&response.status_code) {
        return Err(format!("catalog HTTP {}", response.status_code));
    }
    response.as_str().map(str::to_owned).map_err(|e| e.to_string())
}

/// Parses and validates a catalog document. Rejects a wrong schema outright;
/// silently drops individual entries that are missing required fields or carry
/// a malformed download URL, so one bad tool never breaks the whole catalog.
pub fn parse(raw: &str) -> Result<Catalog, String> {
    let root: Value = serde_json::from_str(raw).map_err(|e| e.to_string())?;
    let schema = root["schema"].as_u64().unwrap_or(0) as u32;
    if schema != SCHEMA {
        return Err(format!("unsupported registry schema {schema}"));
    }
    let registry_version = root["registry_version"].as_u64().unwrap_or(0);
    let tools = root["tools"]
        .as_array()
        .map(|arr| arr.iter().filter_map(parse_entry).collect())
        .unwrap_or_default();
    Ok(Catalog { registry_version, tools })
}

fn parse_entry(value: &Value) -> Option<ToolEntry> {
    let s = |key: &str| value[key].as_str().unwrap_or("").trim().to_string();
    let entry = ToolEntry {
        id: s("id"),
        name: s("name"),
        publisher: s("publisher"),
        description_en: s("description_en"),
        description_pt: s("description_pt"),
        version: s("version"),
        exe: s("exe"),
        download_url: s("download_url"),
        sha256: s("sha256").to_lowercase(),
        size_bytes: value["size_bytes"].as_u64().unwrap_or(0),
        homepage: s("homepage"),
    };
    // Required fields for a usable entry. download_url, when present, must be a
    // trusted HTTPS host; sha256 may be empty (metadata-only), but if present
    // it must be well-formed.
    if entry.id.is_empty() || entry.exe.is_empty() || entry.version.is_empty() {
        return None;
    }
    if entry.download_url.is_empty() || !is_allowed_url(&entry.download_url) {
        return None;
    }
    if !entry.sha256.is_empty() && !is_hex64(&entry.sha256) {
        return None;
    }
    Some(entry)
}

pub fn is_hex64(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| b.is_ascii_hexdigit())
}

/// True for `https://` URLs whose host ends in one of the allowed suffixes.
pub fn is_allowed_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else { return false };
    let host = rest.split(['/', ':']).next().unwrap_or("");
    ALLOWED_HOST_SUFFIXES
        .iter()
        .any(|suffix| host == *suffix || host.ends_with(&format!(".{suffix}")))
}

fn read_file(path: &std::path::Path) -> Option<String> {
    fs::read_to_string(path).ok()
}

fn write_cache(raw: &str) -> std::io::Result<()> {
    let path = paths::registry_cache_path();
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(path, raw)
}
