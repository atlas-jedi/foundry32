//! UI language: the two supported languages and Windows UI-language detection.
//! The per-app string tables live in each app's own `i18n` module; only the
//! `Lang` selector and system detection are shared here.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    PtBr,
    En,
}

impl Lang {
    pub fn code(&self) -> &'static str {
        match self {
            Lang::PtBr => "pt-BR",
            Lang::En => "en",
        }
    }

    pub fn from_code(code: &str) -> Option<Lang> {
        match code {
            "pt-BR" => Some(Lang::PtBr),
            "en" => Some(Lang::En),
            _ => None,
        }
    }
}

/// Detects the Windows UI language: Portuguese → PtBr, anything else → En.
pub fn detect_system_lang() -> Lang {
    const LANG_PORTUGUESE: u16 = 0x16;
    // SAFETY: GetUserDefaultUILanguage takes no arguments and only returns a LANGID.
    let langid = unsafe { winapi::um::winnls::GetUserDefaultUILanguage() };
    if (langid & 0x3FF) == LANG_PORTUGUESE {
        Lang::PtBr
    } else {
        Lang::En
    }
}
