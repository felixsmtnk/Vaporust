//! Vaporust error log: <data folder>\vaporust.log (over 1 MB - the old one becomes
//! vaporust.log.old). Logs errors of game launches, downloads, Steam requests and program crashes.
//! No personal data (login, SteamID) is written here - the log can be attached to a bug report.

use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

static LOCK: Mutex<()> = Mutex::new(());

pub fn path() -> Option<PathBuf> {
    let base = if crate::portable::enabled() { crate::portable::data_dir() } else { crate::portable::normal_dir() };
    base.map(|d| d.join("vaporust.log"))
}

/// Remove long numbers (SteamID and so on) and the user folder path from the text.
/// Remove login keys: values of access_token=, refresh_token=, sessionid= and the JWT keys themselves
/// (eyJ...). Such text could end up in an error together with the request URL.
fn hide_keys(s: &str) -> String {
    let mut out = s.to_string();
    for key in ["access_token=", "refresh_token=", "sessionid=", "steamLoginSecure="] {
        let mut from = 0;
        while let Some(p) = out[from..].find(key) {
            let start = from + p + key.len();
            let end = out[start..].find(|c: char| c == '&' || c == '"' || c == ';' || c.is_whitespace()).map(|e| start + e).unwrap_or(out.len());
            out.replace_range(start..end, tr!("<скрыт>", "<hidden>"));
            from = start + tr!("<скрыт>", "<hidden>").len();
        }
    }
    // JWT: "eyJ" + a long string of letters, digits, "-", "_", ".".
    let mut res = String::with_capacity(out.len());
    let mut rest = out.as_str();
    while let Some(p) = rest.find("eyJ") {
        res.push_str(&rest[..p]);
        let tail = &rest[p..];
        let len = tail.find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')).unwrap_or(tail.len());
        if len >= 30 {
            res.push_str(tr!("<ключ>", "<key>"));
        } else {
            res.push_str(&tail[..len]);
        }
        rest = &tail[len.max(3)..];
    }
    res.push_str(rest);
    res
}

fn scrub(s: &str) -> String {
    let s = &hide_keys(s);
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
        // Any length: short names (for example "PC") are personal too.
        Ok(u) if !u.is_empty() => out.replace(&format!(r"\Users\{u}\"), r"\Users\<user>\"),
        _ => out,
    }
}

/// Write a line: "time [where] text".
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

/// Program crashes go to the log too (with the code location).
pub fn install_panic_hook() {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        write(tr!("сбой", "crash"), &format!("{} v{}", info, env!("CARGO_PKG_VERSION")));
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

/// Windows version from the registry (no personal data): "Windows 10 Pro 22H2, build 19045".
fn windows_version() -> String {
    #[cfg(windows)]
    {
        use winreg::enums::HKEY_LOCAL_MACHINE;
        if let Ok(k) = winreg::RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion") {
            let get = |n: &str| k.get_value::<String, _>(n).unwrap_or_default();
            return trf!("{} {}, сборка {}", "{} {}, build {}", get("ProductName"), get("DisplayVersion"), get("CurrentBuild"));
        }
    }
    tr!("неизвестно", "unknown").into()
}

/// "Diagnostics" text for a bug report: version, Windows, window and scale, memory and
/// the last log lines. No login, SteamID, tokens or Windows user name
/// (the log is already cleaned by scrub, and nothing personal is added here).
pub fn diagnostics(extra: &[(&str, String)]) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "Vaporust {} ({})\n",
        env!("CARGO_PKG_VERSION"),
        if cfg!(target_pointer_width = "64") { tr!("64 бит", "64-bit") } else { tr!("32 бит", "32-bit") }
    ));
    s.push_str(&format!("Windows: {}\n", windows_version()));
    for (k, v) in extra {
        s.push_str(&format!("{k}: {v}\n"));
    }
    s.push_str(tr!("\nПоследние записи журнала:\n", "\nLast log entries:\n"));
    let lines: Vec<String> = path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|t| t.lines().map(scrub).collect())
        .unwrap_or_default();
    if lines.is_empty() {
        s.push_str(tr!("(пусто)\n", "(empty)\n"));
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
        let k = super::scrub("error sending request for url (https://api.steampowered.com/X/?access_token=eyJhbGciOiJFZERTQSIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJyOlN0ZWFt.abcdef&x=1)");
        assert!(!k.contains("eyJhbGci") && k.contains("access_token=<скрыт>") && k.contains("&x=1"), "{k}");
        assert!(super::scrub("токен eyJhbGciOiJFZERTQSIsInR5cCI6IkpXVCJ9.eyJzdWIiOiI3NjU2MTE5 конец").contains("<ключ> конец"));
        assert!(s.contains("<id>") && !s.contains("76561198000000000") && s.contains("730"), "{s}");
        let d = super::diagnostics(&[("Окно", "960x630".into())]);
        assert!(d.contains("Vaporust") && d.contains("Окно: 960x630"), "{d}");
        if let Ok(u) = std::env::var("USERNAME") {
            let p = super::scrub(&format!(r"C:\Users\{u}\AppData\x.log"));
            assert!(!p.contains(&format!(r"\{u}\")) && p.contains("<user>"), "{p}");
        }
    }
}
