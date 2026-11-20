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
/// The temporary name is unique (process number + counter, created only new), so two writers
/// never share it, and the data is flushed to the disk before the rename.
pub fn write_atomic(path: &std::path::Path, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    write_atomic_with(path, |f| f.write_all(data))
}

/// Copy `src` over `path` the same safe way as [`write_atomic`], without loading it into memory.
pub fn copy_atomic(src: &std::path::Path, path: &std::path::Path) -> std::io::Result<()> {
    let mut from = std::fs::File::open(src)?;
    write_atomic_with(path, |f| std::io::copy(&mut from, f).map(|_| ()))
}

/// The common part: a unique temporary neighbor, filled by `fill`, flushed to the disk, renamed.
fn write_atomic_with(path: &std::path::Path, fill: impl FnOnce(&mut std::fs::File) -> std::io::Result<()>) -> std::io::Result<()> {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(format!(".vaporust-tmp-{}-{n}", std::process::id()));
    let tmp = PathBuf::from(tmp);
    let res = (|| {
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        fill(&mut f)?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, path)
    })();
    if res.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    res
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

/// Is this host safe to fetch from after a redirect: not this computer and not the
/// local network (a page on the internet must not make Vaporust poke the router).
pub fn public_host(u: &url::Url) -> bool {
    match u.host() {
        Some(url::Host::Domain(d)) => {
            // "localhost." is localhost too: the trailing dot of a full domain name is dropped.
            let d = d.to_ascii_lowercase();
            let d = d.trim_end_matches('.');
            !(d == "localhost" || d.ends_with(".localhost") || d.ends_with(".local") || d.ends_with(".lan") || !d.contains('.'))
        }
        Some(url::Host::Ipv4(ip)) => !(ip.is_private() || ip.is_loopback() || ip.is_link_local() || ip.is_unspecified() || ip.is_broadcast()),
        // An IPv4 address written as IPv6 ([::ffff:127.0.0.1]) is checked as that IPv4 address.
        Some(url::Host::Ipv6(ip)) if ip.to_ipv4_mapped().is_some() => {
            let v4 = ip.to_ipv4_mapped().unwrap();
            !(v4.is_private() || v4.is_loopback() || v4.is_link_local() || v4.is_unspecified() || v4.is_broadcast())
        }
        Some(url::Host::Ipv6(ip)) => !(ip.is_loopback() || ip.is_unspecified() || (ip.segments()[0] & 0xfe00) == 0xfc00 || (ip.segments()[0] & 0xffc0) == 0xfe80),
        None => false,
    }
}

/// A GET that never keeps more than `max` bytes in memory: the body is read in pieces and
/// the download stops as soon as it grows past the limit, or when `keep_going` says no.
/// Redirects are followed by hand (at most 4), and only to public http(s) addresses.
/// Returns the body and its content type.
pub fn http_get_limited(url: &str, max: usize, keep_going: &dyn Fn() -> bool) -> Result<(Vec<u8>, Option<String>), String> {
    http_get_with(url, &[], max, keep_going)
}

/// [`http_get_limited`] with extra request headers.
pub fn http_get_with(url: &str, headers: &[(&str, &str)], max: usize, keep_going: &dyn Fn() -> bool) -> Result<(Vec<u8>, Option<String>), String> {
    use std::io::Read;
    let agent = ureq::AgentBuilder::new()
        .redirects(0)
        .timeout_connect(std::time::Duration::from_secs(10))
        .timeout_read(std::time::Duration::from_secs(20))
        .user_agent(concat!("Vaporust/", env!("CARGO_PKG_VERSION")))
        .build();
    let mut cur = url::Url::parse(url).map_err(|e| e.to_string())?;
    // The first address is checked like the redirects (a picture link in a chat must not reach the
    // local network). Tests use a local server.
    #[cfg(not(test))]
    if !public_host(&cur) {
        return Err("local address".into());
    }
    for _ in 0..5 {
        if !matches!(cur.scheme(), "http" | "https") || !cur.username().is_empty() || cur.password().is_some() {
            return Err("unsupported address".into());
        }
        let mut req = agent.request_url("GET", &cur);
        for (k, v) in headers {
            req = req.set(k, v);
        }
        let resp = match req.call() {
            Ok(r) => r,
            Err(ureq::Error::Status(code, _)) => return Err(format!("HTTP {code}")),
            Err(e) => return Err(e.to_string()),
        };
        if (300..400).contains(&resp.status()) {
            let loc = resp.header("location").ok_or("redirect without an address")?;
            let next = cur.join(loc).map_err(|e| e.to_string())?;
            if !public_host(&next) {
                return Err("redirect to a local address".into());
            }
            cur = next;
            continue;
        }
        if let Some(len) = resp.header("content-length").and_then(|v| v.trim().parse::<u64>().ok()) {
            if len > max as u64 {
                return Err("file too large".into());
            }
        }
        let mime = resp.header("content-type").map(str::to_owned);
        let mut body = Vec::new();
        let mut reader = resp.into_reader().take(max as u64 + 1);
        let mut buf = [0u8; 64 * 1024];
        loop {
            if !keep_going() {
                return Err("cancelled".into());
            }
            let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            body.extend_from_slice(&buf[..n]);
            if body.len() > max {
                return Err("file too large".into());
            }
        }
        return Ok((body, mime));
    }
    Err("too many redirects".into())
}

#[cfg(test)]
mod http_tests {
    use super::*;

    #[test]
    fn local_hosts_are_not_public() {
        for u in ["http://localhost/", "http://127.0.0.1/", "http://192.168.1.1/", "http://10.0.0.5/", "http://router/", "http://[::1]/", "http://nas.local/", "http://169.254.1.1/"] {
            assert!(!public_host(&url::Url::parse(u).unwrap()), "{u}");
        }
        for u in ["https://cdn.fastly.steamstatic.com/a.png", "https://8.8.8.8/"] {
            assert!(public_host(&url::Url::parse(u).unwrap()), "{u}");
        }
    }

    /// A tiny local server that answers with an endless body: the download must stop at
    /// the limit instead of reading everything.
    #[test]
    fn endless_body_stops_at_limit() {
        use std::io::{Read, Write};
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        std::thread::spawn(move || {
            if let Ok((mut s, _)) = l.accept() {
                let mut b = [0u8; 1024];
                let _ = s.read(&mut b);
                let _ = s.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: image/png\r\n\r\n");
                let chunk = [0u8; 8192];
                for _ in 0..10_000 {
                    if s.write_all(&chunk).is_err() {
                        break;
                    }
                }
            }
        });
        let r = http_get_limited(&format!("http://127.0.0.1:{port}/x.png"), 100_000, &|| true);
        assert_eq!(r.unwrap_err(), "file too large");
    }

    #[test]
    fn redirect_to_local_is_refused() {
        use std::io::{Read, Write};
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        std::thread::spawn(move || {
            if let Ok((mut s, _)) = l.accept() {
                let mut b = [0u8; 1024];
                let _ = s.read(&mut b);
                let _ = s.write_all(b"HTTP/1.1 302 Found\r\nLocation: http://192.168.0.1/admin\r\nContent-Length: 0\r\n\r\n");
            }
        });
        let r = http_get_limited(&format!("http://127.0.0.1:{port}/x.png"), 100_000, &|| true);
        assert_eq!(r.unwrap_err(), "redirect to a local address");
    }
}
