//! Local wall-clock time and calendar dates, via Win32.
//!
//! The workspace has no date/time crate and does not want one: `std::time`
//! only offers an epoch offset. `GetLocalTime` gives today as the user's clock
//! shows it, and a round trip through `FILETIME` (`SystemTimeToFileTime` and
//! back) does the calendar arithmetic — month lengths, leap years, the day of
//! the week — that stepping from one day to the next needs.

use foundry_common::lang::Lang;
use winapi::shared::minwindef::FILETIME;
use winapi::um::minwinbase::SYSTEMTIME;

/// FILETIME counts 100-nanosecond intervals; this many make a day.
const TICKS_PER_DAY: i64 = 864_000_000_000;

/// A calendar day. Field order makes the derived ordering chronological.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date {
    pub year: u16,
    pub month: u16,
    pub day: u16,
}

fn local_now() -> SYSTEMTIME {
    use winapi::um::sysinfoapi::GetLocalTime;
    // SAFETY: GetLocalTime only writes the SYSTEMTIME we hand it.
    let mut now: SYSTEMTIME = unsafe { std::mem::zeroed() };
    unsafe { GetLocalTime(&mut now) };
    now
}

/// Noon of `date` as a SYSTEMTIME — noon so that no arithmetic on it can ever
/// be read as a neighbouring day.
fn noon(date: Date) -> SYSTEMTIME {
    // SAFETY: SYSTEMTIME is plain data; all-zero is a valid starting value.
    let mut time: SYSTEMTIME = unsafe { std::mem::zeroed() };
    time.wYear = date.year;
    time.wMonth = date.month;
    time.wDay = date.day;
    time.wHour = 12;
    time
}

/// The date as a FILETIME tick count, or `None` when it is not a real day —
/// `SystemTimeToFileTime` rejects 2026-02-30 and month 13, which is exactly the
/// validation `Date::parse` needs.
fn to_ticks(date: Date) -> Option<i64> {
    use winapi::um::timezoneapi::SystemTimeToFileTime;
    let time = noon(date);
    let mut file_time = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
    // SAFETY: both pointers are to locals that outlive the call.
    let ok = unsafe { SystemTimeToFileTime(&time, &mut file_time) };
    if ok == 0 {
        return None;
    }
    Some(((file_time.dwHighDateTime as i64) << 32) | file_time.dwLowDateTime as i64)
}

fn from_ticks(ticks: i64) -> Option<SYSTEMTIME> {
    use winapi::um::timezoneapi::FileTimeToSystemTime;
    let file_time = FILETIME { dwLowDateTime: ticks as u32, dwHighDateTime: (ticks >> 32) as u32 };
    // SAFETY: SYSTEMTIME is plain data; FileTimeToSystemTime only writes it.
    let mut time: SYSTEMTIME = unsafe { std::mem::zeroed() };
    let ok = unsafe { FileTimeToSystemTime(&file_time, &mut time) };
    (ok != 0).then_some(time)
}

impl Date {
    /// Today, by the user's own clock.
    pub fn today() -> Date {
        let now = local_now();
        Date { year: now.wYear, month: now.wMonth, day: now.wDay }
    }

    /// Reads `2026-11-04`. Anything else — another shape, or a day the
    /// calendar does not have — is `None`.
    pub fn parse(text: &str) -> Option<Date> {
        let mut parts = text.trim().splitn(3, '-');
        let year = parts.next()?.parse::<u16>().ok()?;
        let month = parts.next()?.parse::<u16>().ok()?;
        let day = parts.next()?.parse::<u16>().ok()?;
        let date = Date { year, month, day };
        let valid = (1601..=9999).contains(&year) && to_ticks(date).is_some();
        valid.then_some(date)
    }

    /// `2026-11-04` — the day file's name, sortable as text.
    pub fn iso(&self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }

    /// `20261104` — the prefix of every entry id written on this day.
    pub fn compact(&self) -> String {
        format!("{:04}{:02}{:02}", self.year, self.month, self.day)
    }

    /// The day `days` away (negative goes back). A date is only ever built
    /// from the calendar, so the arithmetic cannot fail in practice; should it,
    /// the date is returned unchanged rather than inventing one.
    pub fn shift(&self, days: i64) -> Date {
        to_ticks(*self)
            .and_then(|ticks| from_ticks(ticks + days * TICKS_PER_DAY))
            .map(|time| Date { year: time.wYear, month: time.wMonth, day: time.wDay })
            .unwrap_or(*self)
    }

    /// Day of the week, 0 = Sunday … 6 = Saturday.
    pub fn weekday(&self) -> u16 {
        to_ticks(*self).and_then(from_ticks).map(|time| time.wDayOfWeek).unwrap_or(0)
    }

    /// `qua, 04/11/2026` / `Wed, 11/04/2026` — how a heading names the day.
    pub fn display(&self, lang: Lang) -> String {
        let weekday = self.weekday() as usize % 7;
        match lang {
            Lang::PtBr => {
                const NAMES: [&str; 7] = ["dom", "seg", "ter", "qua", "qui", "sex", "sáb"];
                format!("{}, {:02}/{:02}/{:04}", NAMES[weekday], self.day, self.month, self.year)
            }
            Lang::En => {
                const NAMES: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
                format!("{}, {:02}/{:02}/{:04}", NAMES[weekday], self.month, self.day, self.year)
            }
        }
    }
}

/// `2026-11-04 09:12:33` — sorts lexicographically, reads as a human date.
pub fn timestamp() -> String {
    let now = local_now();
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        now.wYear, now.wMonth, now.wDay, now.wHour, now.wMinute, now.wSecond
    )
}

/// `09:12` — the time an entry is written down at.
pub fn now_hm() -> String {
    let now = local_now();
    format!("{:02}:{:02}", now.wHour, now.wMinute)
}

/// `091233` — the second an entry is created, the variable half of its id.
pub fn now_hms_compact() -> String {
    let now = local_now();
    format!("{:02}{:02}{:02}", now.wHour, now.wMinute, now.wSecond)
}

/// Normalises a typed time to `HH:MM`, accepting `9:5` and `09:05` alike.
/// `None` when it is not a time of day.
pub fn parse_hm(text: &str) -> Option<String> {
    let (hours, minutes) = text.trim().split_once(':')?;
    let hours = hours.trim().parse::<u8>().ok()?;
    let minutes = minutes.trim().parse::<u8>().ok()?;
    (hours < 24 && minutes < 60).then(|| format!("{hours:02}:{minutes:02}"))
}
