//! DAILY engine — write down what you did during the day, read it back grouped
//! by task at the stand-up.
//!
//! One engine, two front-ends. This library holds ALL the logic — the calendar,
//! the day files, every edit, and the text a day turns into. The two binaries
//! are thin clients:
//!
//! - `daily.exe` — console subsystem, goes on PATH; the CLI Claude runs to read
//!   a day back (`daily report`), to learn how the user writes (`daily style`)
//!   and to add what the user did not get to write down (`daily add`).
//! - `daily-gui.exe` — GUI subsystem, launched by the Foundry32 hub; the window
//!   where the user writes entries down while working.
//!
//! Why two exes and not one: a Windows PE declares a single subsystem bit at
//! build time. A `console` exe flashes a console window on double-click; a
//! `windows` (GUI) exe isn't awaited by the shell. Same split as JAFIZ and WITN.
//!
//! The division of labour inside the engine: `store` is the only module that
//! touches the filesystem, `edit` is the only one that changes a day, `report`
//! is day→text. Both front-ends write through `edit`, and `edit` always reads
//! the day back from disk right before changing it — the window and the CLI can
//! both be writing the same day, and neither may write over the other.

pub mod clock;
pub mod edit;
pub mod model;
pub mod report;
pub mod settings;
pub mod store;

pub use clock::Date;
pub use model::{Author, Day, Entry, Group};
