//! JAFIZ engine — "já fiz?" ("did I test it yet?").
//!
//! One engine, two front-ends. This library holds ALL the logic — parse a
//! suite of manual test scenarios from Markdown, resolve where suites live,
//! record a run's per-step verdicts to JSON, and render the result back as
//! Markdown. The two binaries are thin clients:
//!
//! - `jafiz.exe` — console subsystem, goes on PATH; the CLI Claude runs to
//!   read a run back (`jafiz report`) and to validate what it just wrote
//!   (`jafiz check`).
//! - `jafiz-gui.exe` — GUI subsystem, launched by the Foundry32 hub; where the
//!   user marks each step while testing by hand.
//!
//! Why two exes and not one: a Windows PE declares a single subsystem bit at
//! build time. A `console` exe flashes a console window on double-click; a
//! `windows` (GUI) exe isn't awaited by the shell. Same split as WITN, and as
//! `python.exe`/`pythonw.exe`.
//!
//! The division of labour inside the engine: `parser` is text→model, `store`
//! is the only module that touches the filesystem, `runs` is the only one that
//! mutates a run, `report` is model→text. The suite Markdown is **never**
//! rewritten — progress lives beside it under `.runs/`.

pub mod clock;
pub mod model;
pub mod parser;
pub mod runs;
pub mod settings;
pub mod store;

pub use model::{Run, RunFile, Scenario, Step, StepResult, StepStatus, Suite};
