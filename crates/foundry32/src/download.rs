//! Streaming download with SHA-256 verification.
//!
//! The body is read in 64 KiB chunks through `std::io::Read` — NOT the per-byte
//! `ResponseLazy` iterator, which yields one byte at a time. Hashing, writing
//! and progress happen in a single pass, and a cancel flag is checked between
//! chunks (minreq's timeout is a total deadline that would kill a slow-but-
//! -progressing download, so no network timeout is set here).

use crate::registry::is_allowed_url;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

pub struct Progress {
    pub done: u64,
    /// Total size from Content-Length, or None when the server didn't send it.
    pub total: Option<u64>,
}

#[derive(Debug)]
pub enum DlError {
    Http(String),
    Sha { expected: String, got: String },
    Cancelled,
    Io(String),
}

impl std::fmt::Display for DlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DlError::Http(m) => write!(f, "download failed: {m}"),
            DlError::Sha { expected, got } => {
                write!(f, "checksum mismatch (expected {expected}, got {got})")
            }
            DlError::Cancelled => write!(f, "cancelled"),
            DlError::Io(m) => write!(f, "write failed: {m}"),
        }
    }
}

const BUF: usize = 64 * 1024;

/// Downloads `url` into `dest_tmp`, verifying it hashes to `expected_sha`
/// (lowercase 64-hex). The temp file is removed on any failure.
pub fn download_verify(
    url: &str,
    expected_sha: &str,
    dest_tmp: &Path,
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(Progress),
) -> Result<(), DlError> {
    if !is_allowed_url(url) {
        return Err(DlError::Http(format!("refusing non-allowed URL: {url}")));
    }

    let response = minreq::get(url)
        .with_header("User-Agent", concat!("foundry32/", env!("CARGO_PKG_VERSION")))
        .send_lazy()
        .map_err(|e| DlError::Http(e.to_string()))?;
    if !(200..300).contains(&response.status_code) {
        return Err(DlError::Http(format!("HTTP {}", response.status_code)));
    }
    // Header keys are lowercased by minreq. Content-Length may be absent.
    let total = response
        .headers
        .get("content-length")
        .and_then(|s| s.trim().parse::<u64>().ok());

    if let Some(dir) = dest_tmp.parent() {
        std::fs::create_dir_all(dir).map_err(|e| DlError::Io(e.to_string()))?;
    }
    let result = stream_to_file(response, expected_sha, dest_tmp, cancel, &mut on_progress, total);
    if result.is_err() {
        let _ = std::fs::remove_file(dest_tmp);
    }
    result
}

fn stream_to_file(
    mut reader: minreq::ResponseLazy,
    expected_sha: &str,
    dest_tmp: &Path,
    cancel: &AtomicBool,
    on_progress: &mut impl FnMut(Progress),
    total: Option<u64>,
) -> Result<(), DlError> {
    let mut file = File::create(dest_tmp).map_err(|e| DlError::Io(e.to_string()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; BUF];
    let mut done: u64 = 0;

    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(DlError::Cancelled);
        }
        let n = reader.read(&mut buf).map_err(|e| DlError::Http(e.to_string()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        file.write_all(&buf[..n]).map_err(|e| DlError::Io(e.to_string()))?;
        done += n as u64;
        on_progress(Progress { done, total });
    }
    file.flush().map_err(|e| DlError::Io(e.to_string()))?;
    drop(file);

    let got = format!("{:x}", hasher.finalize());
    let expected = expected_sha.to_lowercase();
    if got != expected {
        return Err(DlError::Sha { expected, got });
    }
    Ok(())
}
