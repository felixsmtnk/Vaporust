//! Журнал ошибок Vaporust: <папка данных>\vaporust.log (больше 1 МБ - старый становится
//! vaporust.log.old). Пишутся ошибки запуска игр, загрузок, запросов к Steam и падения программы.
//! Личных данных (логин, SteamID) сюда не пишем - журнал можно приложить к сообщению об ошибке.

use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

static LOCK: Mutex<()> = Mutex::new(());

pub fn path() -> Option<PathBuf> {
    let base = if crate::portable::enabled() { crate::portable::data_dir() } else { crate::portable::normal_dir() };
    base.map(|d| d.join("vaporust.log"))
}

/// Убрать из текста длинные числа (SteamID и т.п.) и путь к папке пользователя.
fn scrub(s: &str) -> String {
    let mut out = String::new();
    let mut digits = String::new();
    for c in s.chars() {
        if c.is_ascii_digit() {
            digits.push(c);
            continue;
        }
        out.push_str(if digits.len() >= 15 { "<id>" } else { &digits });
        digits.clear();
        out.push(c);
    }
    out.push_str(if digits.len() >= 15 { "<id>" } else { &digits });
    match std::env::var("USERNAME") {
        // Любой длины: короткие имена (например, «PC») тоже личные.
        Ok(u) if !u.is_empty() => out.replace(&format!(r"\Users\{u}\"), r"\Users\<user>\"),
        _ => out,
    }
}

/// Записать строку: «время [где] текст».
pub fn write(place: &str, msg: &str) {
    let Some(p) = path() else { return };
    let _g = LOCK.lock();
    if p.metadata().is_ok_and(|m| m.len() > 1_000_000) {
        let _ = std::fs::rename(&p, p.with_extension("log.old"));
    }
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&p) {
        let line = scrub(&msg.replace('\n', " | "));
        let _ = writeln!(f, "{} [{place}] {line}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"));
    }
}

/// Падения программы - тоже в журнал (вместе с местом в коде).
pub fn install_panic_hook() {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        write("сбой", &format!("{} v{}", info, env!("CARGO_PKG_VERSION")));
        prev(info);
    }));
}

#[cfg(test)]
mod tests {
    #[test]
    fn scrub_ids() {
        assert_eq!(super::scrub("steamid 76561198000000000, app 730"), "steamid <id>, app 730");
    }
}

/// Версия Windows из реестра (без личных данных): «Windows 10 Pro 22H2, сборка 19045».
fn windows_version() -> String {
    #[cfg(windows)]
    {
        use winreg::enums::HKEY_LOCAL_MACHINE;
        if let Ok(k) = winreg::RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion") {
            let get = |n: &str| k.get_value::<String, _>(n).unwrap_or_default();
            return format!("{} {}, сборка {}", get("ProductName"), get("DisplayVersion"), get("CurrentBuild"));
        }
    }
    "неизвестно".into()
}

/// Текст «диагностики» для сообщения об ошибке: версия, Windows, окно и масштаб, память и
/// последние строки журнала. Без логина, SteamID, токенов и имени пользователя Windows
/// (журнал уже очищен scrub, а сюда больше ничего личного не добавляется).
pub fn diagnostics(extra: &[(&str, String)]) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "Vaporust {} ({})\n",
        env!("CARGO_PKG_VERSION"),
        if cfg!(target_pointer_width = "64") { "64 бит" } else { "32 бит" }
    ));
    s.push_str(&format!("Windows: {}\n", windows_version()));
    for (k, v) in extra {
        s.push_str(&format!("{k}: {v}\n"));
    }
    s.push_str("\nПоследние записи журнала:\n");
    let lines: Vec<String> = path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|t| t.lines().map(scrub).collect())
        .unwrap_or_default();
    if lines.is_empty() {
        s.push_str("(пусто)\n");
    }
    for l in lines.iter().rev().take(30).rev() {
        s.push_str(l);
        s.push('\n');
    }
    s
}

#[cfg(test)]
mod diag_tests {
    #[test]
    fn scrub_hides_ids_and_user() {
        let s = super::scrub("друг 76561198000000000 и appid 730");
        assert!(s.contains("<id>") && !s.contains("76561198000000000") && s.contains("730"), "{s}");
        let d = super::diagnostics(&[("Окно", "960x630".into())]);
        assert!(d.contains("Vaporust") && d.contains("Окно: 960x630"), "{d}");
        if let Ok(u) = std::env::var("USERNAME") {
            let p = super::scrub(&format!(r"C:\Users\{u}\AppData\x.log"));
            assert!(!p.contains(&format!(r"\{u}\")) && p.contains("<user>"), "{p}");
        }
    }
}
