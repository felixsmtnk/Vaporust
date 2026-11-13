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
        Ok(u) if u.len() > 2 => out.replace(&format!(r"\Users\{u}\"), r"\Users\<user>\"),
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
