//! Language of the interface: French or English.
//!
//! The texts are written next to each other where they are shown:
//! `t!("Fichier", "File")` (a `&'static str`) and `tf!("{n} fichiers", "{n} files")` (a `String`,
//! same arguments as `format!`).

use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    Fr,
    En,
}

/// (language, code in the settings, name shown).
pub const LANGS: [(Lang, &str, &str); 2] = [(Lang::Fr, "fr", "Français"), (Lang::En, "en", "English")];

static LANG: AtomicU8 = AtomicU8::new(0);

pub fn lang() -> Lang {
    if LANG.load(Ordering::Relaxed) == 1 {
        Lang::En
    } else {
        Lang::Fr
    }
}

pub fn en() -> bool {
    lang() == Lang::En
}

/// The language of the code `code` ("fr", "en"); empty: the system's (French if it is French,
/// else English).
pub fn from_code(code: &str) -> Lang {
    let code = if code.trim().is_empty() { system_code() } else { code.trim().to_lowercase() };
    if code.starts_with("fr") {
        Lang::Fr
    } else {
        Lang::En
    }
}

fn system_code() -> String {
    ["LC_ALL", "LC_MESSAGES", "LANG", "LANGUAGE"]
        .iter()
        .filter_map(|v| std::env::var(v).ok())
        .find(|v| !v.is_empty() && v != "C" && v != "POSIX")
        .unwrap_or_else(|| "fr".into())
        .to_lowercase()
}

pub fn set(lang: Lang) {
    LANG.store(if lang == Lang::En { 1 } else { 0 }, Ordering::Relaxed);
}

pub fn name(lang: Lang) -> &'static str {
    LANGS.iter().find(|(l, _, _)| *l == lang).map(|(_, _, n)| *n).unwrap_or("Français")
}

/// The text in the language of the interface: `t!("Fichier", "File")`.
#[macro_export]
macro_rules! t {
    ($fr:expr, $en:expr $(,)?) => {
        if $crate::i18n::en() {
            $en
        } else {
            $fr
        }
    };
}

/// `format!` in the language of the interface: `tf!("{n} fichiers", "{n} files")`.
#[macro_export]
macro_rules! tf {
    ($fr:literal, $en:literal $(, $arg:expr)* $(,)?) => {
        if $crate::i18n::en() {
            format!($en $(, $arg)*)
        } else {
            format!($fr $(, $arg)*)
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes() {
        assert_eq!(from_code("fr"), Lang::Fr);
        assert_eq!(from_code("en"), Lang::En);
        assert_eq!(from_code("fr_BE.UTF-8"), Lang::Fr);
        assert_eq!(from_code("de"), Lang::En);
        for (l, c, _) in LANGS {
            assert_eq!(from_code(c), l);
        }
    }
}
