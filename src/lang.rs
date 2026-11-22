//! UI language. Affects only the app's own hint text: status line, key cheat
//! sheet, and `:` command echo. CLI output (`-h` help, argument errors) is
//! always English.

/// UI language (English by default; `--lang zh` or `view.lang` switches to
/// Chinese)
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Lang {
    Zh,
    #[default]
    En,
}

impl Lang {
    /// Pick the entry for the current language (for static text).
    pub fn pick<'a>(&self, zh: &'a str, en: &'a str) -> &'a str {
        match self {
            Lang::Zh => zh,
            Lang::En => en,
        }
    }

    /// Pick the entry for the current language (for built strings). Both sides
    /// are evaluated, which is fine: this is only hint text.
    pub fn pick_owned(&self, zh: String, en: String) -> String {
        match self {
            Lang::Zh => zh,
            Lang::En => en,
        }
    }

    /// The other language (a bare `:lang` switches to it).
    pub fn other(&self) -> Lang {
        match self {
            Lang::Zh => Lang::En,
            Lang::En => Lang::Zh,
        }
    }

    /// Short code, used in hints.
    pub fn code(&self) -> &'static str {
        match self {
            Lang::Zh => "zh",
            Lang::En => "en",
        }
    }

    /// Parse the argument of a CLI flag or `:lang`.
    pub fn parse(s: &str) -> Option<Lang> {
        match s.trim().to_ascii_lowercase().as_str() {
            "zh" | "cn" | "zh-cn" | "chinese" | "中文" => Some(Lang::Zh),
            "en" | "en-us" | "english" => Some(Lang::En),
            _ => None,
        }
    }
}
