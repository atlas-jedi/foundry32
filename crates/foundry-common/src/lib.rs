//! Shared Win32 helpers for the Foundry32 workspace (the hub and each tool).
//!
//! Everything here is self-contained — it depends only on `std`, `winapi` and
//! `native-windows-gui`, never on any app's own types — so both `foundry32`
//! and `mcp-console` can build the same look and behavior from one place.

pub mod lang;
pub mod theme;
pub mod ui;
pub mod winproc;

pub use lang::{detect_system_lang, Lang};
pub use winproc::{run_captured, CapturedOutput};
