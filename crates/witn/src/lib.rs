//! WITN engine — "Where Is The Node?"
//!
//! One engine, two front-ends. This library holds ALL the logic — enumerate
//! `node.exe`, map PID→listening ports, derive a friendly app name, build the
//! parent→child tree, (later) terminate. The two binaries are thin clients:
//!
//! - `witn.exe` — console subsystem, goes on PATH; the CLI, with synchronous
//!   terminal I/O and native `[y/N]` confirmation.
//! - `witn-gui.exe` — GUI subsystem, launched by the Foundry32 hub; never
//!   spawns a console. (Added in Phase 3.)
//!
//! Why two exes and not one: a Windows PE declares a single subsystem bit at
//! build time. A `console` exe flashes a console window on double-click; a
//! `windows` (GUI) exe isn't awaited by the shell (the prompt returns before
//! output) and needs `AttachConsole` gymnastics that still can't do interactive
//! stdin. Keeping the logic here and shipping two thin front-ends is the clean
//! way — cf. `python.exe`/`pythonw.exe`, `java.exe`/`javaw.exe`.
//!
//! Everything here reads only the current user's own processes and needs no
//! elevation (`PROCESS_QUERY_LIMITED_INFORMATION`), matching the hub's
//! asInvoker posture.

pub mod appname;
pub mod model;
pub mod peb;
pub mod ports;
pub mod procscan;
pub mod proctree;
pub mod tree;

pub use model::NodeProc;
pub use procscan::Scanner;
