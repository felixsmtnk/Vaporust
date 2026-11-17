//! Small helpers shared by many modules: Windows folders, system programs,
//! text pieces of web pages, numbers from JSON, URL encoding.

use std::path::PathBuf;

/// A Windows "known folder" (Documents, Pictures...) where the user really has it:
/// it may be moved to OneDrive or another disk, so "%USERPROFILE%\Documents" is not enough.
#[cfg(windows)]
fn known_folder(id: &windows_sys::core::GUID) -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::UI::Shell::SHGetKnownFolderPath;
    let mut p: windows_sys::core::PWSTR = std::ptr::null_mut();
    // SAFETY: the function writes a buffer allocated by the shell; we read it up to the zero and free it.
    unsafe {
        let hr = SHGetKnownFolderPath(id, 0, std::ptr::null_mut(), &mut p);
        if hr < 0 || p.is_null() {
            if !p.is_null() {
                CoTaskMemFree(p as _);
            }
            return None;
        }
        let mut n = 0;
        while *p.add(n) != 0 {
            n += 1;
        }
        let s = std::ffi::OsString::from_wide(std::slice::from_raw_parts(p, n));
        CoTaskMemFree(p as _);
        Some(PathBuf::from(s))
    }
}

fn home_sub(name: &str) -> PathBuf {
    std::env::var_os("USERPROFILE").map(PathBuf::from).unwrap_or_default().join(name)
}

/// The "Documents" folder.
pub fn documents() -> PathBuf {
    #[cfg(windows)]
    if let Some(p) = known_folder(&windows_sys::Win32::UI::Shell::FOLDERID_Documents) {
        return p;
    }
    home_sub("Documents")
}

/// The "Pictures" folder.
pub fn pictures() -> PathBuf {
    #[cfg(windows)]
    if let Some(p) = known_folder(&windows_sys::Win32::UI::Shell::FOLDERID_Pictures) {
        return p;
    }
    home_sub("Pictures")
}

/// Full path of a Windows program (cmd.exe, explorer.exe...). Started by a short name, Windows
/// would first look in the folder of Vaporust itself (often "Downloads", where anything may lie).
pub fn system_exe(name: &str) -> PathBuf {
    let win = std::env::var_os("SystemRoot").or_else(|| std::env::var_os("windir")).map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    if name.eq_ignore_ascii_case("explorer.exe") {
        return win.join(name);
    }
    if name.eq_ignore_ascii_case("powershell.exe") {
        return win.join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
    }
    win.join("System32").join(name)
}

/// Text between two markers (first occurrence).
pub fn between<'a>(s: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let i = s.find(start)? + start.len();
    let j = s[i..].find(end)? + i;
    Some(&s[i..j])
}

/// A number from JSON: Steam gives numbers both as numbers and as strings.
pub fn num(v: &serde_json::Value) -> u64 {
    v.as_u64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())).unwrap_or(0)
}

/// URL-encode a value (everything but letters, digits and -_.~).
pub fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Decode HTML entities: &amp; &lt; &gt; &quot; &#39; &#NNN; &#xHH; &nbsp;.
pub fn html_unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        // The ';' within the next 12 characters (by characters: a byte cut could split a letter).
        let Some(semi) = rest.char_indices().take(12).find(|(_, c)| *c == ';').map(|(i, _)| i) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let ent = &rest[1..semi];
        let ch = match ent {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            _ if ent.starts_with("#x") || ent.starts_with("#X") => u32::from_str_radix(&ent[2..], 16).ok().and_then(char::from_u32),
            _ if ent.starts_with('#') => ent[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        match ch {
            Some(c) => {
                out.push(c);
                rest = &rest[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Write a file via a temporary neighbor and rename: a crash or a power cut never leaves
/// a half-written file (Steam would stop seeing the game, settings would be lost).
pub fn write_atomic(path: &std::path::Path, data: &[u8]) -> std::io::Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".vaporust-tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, data)?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers() {
        assert_eq!(between("a<b>c</b>", "<b>", "</b>"), Some("c"));
        assert_eq!(num(&serde_json::json!("42")), 42);
        assert_eq!(num(&serde_json::json!(7)), 7);
        assert_eq!(urlencode("a b&ж"), "a%20b%26%D0%B6");
        assert_eq!(html_unescape("a &amp; b &quot;c&quot; &#39;d&#39; &#x41; &unknown; &"), "a & b \"c\" 'd' A &unknown; &");
        // Russian letters right after '&': no cut inside a letter.
        assert_eq!(html_unescape("&жжжжжжжжжжжж; ж&amp;"), "&жжжжжжжжжжжж; ж&");
        assert!(system_exe("cmd.exe").ends_with(r"System32\cmd.exe"));
        assert!(documents().is_absolute());
    }
}
