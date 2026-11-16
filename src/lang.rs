//! Interface language (Russian or English) and the language in which Vaporust asks Steam
//! for texts (descriptions, news, reviews, item names) - they follow each other.
//!
//! Strings are translated in place: `tr!("Готово.", "Done.")` for plain text and
//! `trf!("Скачано {n} из {}", "Downloaded {n} of {}", total)` instead of `format!`.
//! Both languages sit side by side in the code, the compiler checks both format strings.
//!
//! Some pages are intentionally requested in a fixed language (l=english or l=russian): their
//! parsing relies on Steam's own words - their language must not be changed.

use std::sync::atomic::{AtomicBool, Ordering};

static EN: AtomicBool = AtomicBool::new(false);

/// English interface.
pub fn en() -> bool {
    EN.load(Ordering::Relaxed)
}

pub fn set_en(on: bool) {
    EN.store(on, Ordering::Relaxed);
}

/// Language of Steam texts (the l= and language parameters).
pub fn steam() -> &'static str {
    if en() { "english" } else { "russian" }
}

/// Language from the setting: "ru", "en" or "" (as in Windows).
pub fn resolve(setting: &str) -> bool {
    // VAPORUST_LANG=en|ru overrides the setting (screenshots for the README in both languages).
    static FORCED: std::sync::OnceLock<Option<bool>> = std::sync::OnceLock::new();
    let forced = *FORCED.get_or_init(|| match std::env::var("VAPORUST_LANG").as_deref() {
        Ok("en") => Some(true),
        Ok("ru") => Some(false),
        _ => None,
    });
    if let Some(f) = forced {
        return f;
    }
    match setting {
        "en" => true,
        "ru" => false,
        _ => !windows_is_russian(),
    }
}

/// The Windows interface language is Russian, Ukrainian, Belarusian or Kazakh -
/// such users usually read Russian, everyone else gets English.
pub fn windows_is_russian() -> bool {
    static CACHE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *CACHE.get_or_init(windows_ui_is_russian)
}

fn windows_ui_is_russian() -> bool {
    #[cfg(windows)]
    {
        #[link(name = "kernel32")]
        extern "system" {
            fn GetUserDefaultUILanguage() -> u16;
        }
        // Primary language id: lower 10 bits. 0x19 Russian, 0x22 Ukrainian, 0x23 Belarusian, 0x3F Kazakh.
        let primary = unsafe { GetUserDefaultUILanguage() } & 0x3ff;
        matches!(primary, 0x19 | 0x22 | 0x23 | 0x3f)
    }
    #[cfg(not(windows))]
    {
        true
    }
}

/// Plain text in two languages: `tr!("Готово.", "Done.")`.
macro_rules! tr {
    ($ru:literal, $en:literal) => {
        if $crate::lang::en() { $en } else { $ru }
    };
}

/// `format!` in two languages: `trf!("Скачано {}", "Downloaded {}", n)`.
macro_rules! trf {
    ($ru:literal, $en:literal $($rest:tt)*) => {
        if $crate::lang::en() { format!($en $($rest)*) } else { format!($ru $($rest)*) }
    };
}

#[cfg(test)]
mod tests {
    #[test]
    fn macros_follow_language() {
        // Only the default (Russian) branch: other tests run in parallel and expect Russian texts.
        assert_eq!(tr!("Готово.", "Done."), "Готово.");
        assert_eq!(trf!("{:.1} МБ", "{:.1} MB", 1.25f64), "1.2 МБ");
    }
}
