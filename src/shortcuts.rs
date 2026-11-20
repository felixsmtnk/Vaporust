//! Non-Steam games in the real Steam: the file userdata\<account>\config\shortcuts.vdf (binary VDF).
//! Vaporust can import games from there into its library and add its non-Steam games there.
//! We write only while Steam is closed (otherwise Steam overwrites the file), make a backup once
//! shortcuts.vdf.vaporust.bak, other entries are kept the same in meaning (the field order is the same).

use std::path::{Path, PathBuf};

/// A binary VDF node.
#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    Map(Vec<(String, Node)>),
    Str(String),
    Int(u32),
    /// Rare types (float, uint64 and the like): the type and raw bytes - kept as is.
    Raw(u8, Vec<u8>),
}

fn read_cstr(b: &[u8], i: &mut usize) -> Result<String, String> {
    let start = *i;
    while *i < b.len() && b[*i] != 0 {
        *i += 1;
    }
    if *i >= b.len() {
        return Err(tr!("обрыв строки", "string cut off").into());
    }
    // Not UTF-8 (an old path in a local code page): an error, so the file is never rewritten with
    // replaced characters.
    let s = String::from_utf8(b[start..*i].to_vec()).map_err(|_| tr!("строка не в UTF-8", "a string is not UTF-8").to_string())?;
    *i += 1;
    Ok(s)
}

fn parse_map(b: &[u8], i: &mut usize, depth: u32) -> Result<Vec<(String, Node)>, String> {
    if depth > 32 {
        return Err(tr!("слишком глубокая вложенность", "nesting too deep").into());
    }
    let mut out = Vec::new();
    loop {
        let Some(&t) = b.get(*i) else { return Err(tr!("обрыв файла", "file cut off").into()) };
        *i += 1;
        if t == 0x08 {
            return Ok(out);
        }
        let key = read_cstr(b, i)?;
        let node = match t {
            0x00 => Node::Map(parse_map(b, i, depth + 1)?),
            0x01 => Node::Str(read_cstr(b, i)?),
            0x02 => {
                let v = b.get(*i..*i + 4).ok_or(tr!("обрыв числа", "number cut off"))?;
                *i += 4;
                Node::Int(u32::from_le_bytes([v[0], v[1], v[2], v[3]]))
            }
            0x03 | 0x04 | 0x06 => {
                let v = b.get(*i..*i + 4).ok_or(tr!("обрыв числа", "number cut off"))?.to_vec();
                *i += 4;
                Node::Raw(t, v)
            }
            0x07 | 0x0A => {
                let v = b.get(*i..*i + 8).ok_or(tr!("обрыв числа", "number cut off"))?.to_vec();
                *i += 8;
                Node::Raw(t, v)
            }
            other => return Err(trf!("неизвестный тип {other}", "unknown type {other}")),
        };
        out.push((key, node));
    }
}

/// File parsing: the root is a single map.
pub fn parse(b: &[u8]) -> Result<Vec<(String, Node)>, String> {
    let mut i = 0;
    let root = parse_map(b, &mut i, 0)?;
    // Steam writes one more 0x08 after the root map; anything else after it - not our format.
    if b[i..].iter().any(|&x| x != 0x08) || b.len() - i > 1 {
        return Err(tr!("лишние данные в конце файла", "extra data at the end of the file").into());
    }
    Ok(root)
}

fn write_map(out: &mut Vec<u8>, m: &[(String, Node)]) {
    for (k, n) in m {
        let t = match n {
            Node::Map(_) => 0x00,
            Node::Str(_) => 0x01,
            Node::Int(_) => 0x02,
            Node::Raw(t, _) => *t,
        };
        out.push(t);
        out.extend_from_slice(k.as_bytes());
        out.push(0);
        match n {
            Node::Map(m) => write_map(out, m),
            Node::Str(s) => {
                out.extend_from_slice(s.as_bytes());
                out.push(0);
            }
            Node::Int(v) => out.extend_from_slice(&v.to_le_bytes()),
            Node::Raw(_, v) => out.extend_from_slice(v),
        }
    }
    out.push(0x08);
}

pub fn write(root: &[(String, Node)]) -> Vec<u8> {
    let mut out = Vec::new();
    write_map(&mut out, root);
    out
}

/// A non-Steam game shortcut in Steam.
#[derive(Clone, Debug, PartialEq)]
pub struct Shortcut {
    pub name: String,
    /// Exe path without quotes.
    pub exe: String,
    pub start_dir: String,
    pub args: String,
}

fn get<'a>(m: &'a [(String, Node)], key: &str) -> Option<&'a Node> {
    m.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, n)| n)
}

fn get_str(m: &[(String, Node)], key: &str) -> String {
    match get(m, key) {
        Some(Node::Str(s)) => s.clone(),
        _ => String::new(),
    }
}

fn unquote(s: &str) -> String {
    s.trim().trim_matches('"').to_string()
}

/// The current account's shortcut file (next to the most recent localconfig.vdf).
pub fn path(steam_dir: &Path) -> Option<PathBuf> {
    crate::launchopts::find_localconfig(steam_dir).map(|p| p.with_file_name("shortcuts.vdf"))
}

/// All shortcuts from the file (no file - an empty list).
pub fn read(path: &Path) -> Result<Vec<Shortcut>, String> {
    let b = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.to_string()),
    };
    let root = parse(&b)?;
    let Some(Node::Map(list)) = get(&root, "shortcuts") else { return Ok(Vec::new()) };
    Ok(list
        .iter()
        .filter_map(|(_, n)| match n {
            Node::Map(m) => Some(Shortcut {
                name: get_str(m, "AppName"),
                exe: unquote(&get_str(m, "Exe")),
                start_dir: unquote(&get_str(m, "StartDir")),
                args: get_str(m, "LaunchOptions"),
            }),
            _ => None,
        })
        .collect())
}

/// Shortcut id, as Steam and third-party programs compute it: crc32(quoted exe + name)
/// with the high bit set.
pub fn shortcut_appid(exe_quoted: &str, name: &str) -> u32 {
    let mut h = crc32fast::Hasher::new();
    h.update(exe_quoted.as_bytes());
    h.update(name.as_bytes());
    h.finalize() | 0x8000_0000
}

fn same_exe(a: &str, b: &str) -> bool {
    a.replace('/', "\\").to_lowercase() == b.replace('/', "\\").to_lowercase()
}

/// Add a shortcut. Ok(false) - this exe is already there. Only while Steam is closed!
pub fn add(path: &Path, name: &str, exe: &str, args: &str) -> Result<bool, String> {
    let mut root = match std::fs::read(path) {
        Ok(b) => parse(&b)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(e.to_string()),
    };
    if !root.iter().any(|(k, _)| k.eq_ignore_ascii_case("shortcuts")) {
        root.push(("shortcuts".into(), Node::Map(Vec::new())));
    }
    let list = root
        .iter_mut()
        .find(|(k, _)| k.eq_ignore_ascii_case("shortcuts"))
        .and_then(|(_, n)| if let Node::Map(m) = n { Some(m) } else { None })
        .ok_or(tr!("в файле нет списка ярлыков", "the file has no shortcut list"))?;
    let exists = list.iter().any(|(_, n)| matches!(n, Node::Map(m) if same_exe(&unquote(&get_str(m, "Exe")), exe)));
    if exists {
        return Ok(false);
    }
    let exe_q = format!("\"{exe}\"");
    let dir = Path::new(exe).parent().map(|p| format!("\"{}\\\"", p.display())).unwrap_or_default();
    let next = list.iter().filter_map(|(k, _)| k.parse::<u32>().ok()).max().map_or(0, |n| n + 1);
    let s = |v: &str| Node::Str(v.to_string());
    let entry = vec![
        ("appid".to_string(), Node::Int(shortcut_appid(&exe_q, name))),
        ("AppName".into(), s(name)),
        ("Exe".into(), s(&exe_q)),
        ("StartDir".into(), s(&dir)),
        ("icon".into(), s("")),
        ("ShortcutPath".into(), s("")),
        ("LaunchOptions".into(), s(args)),
        ("IsHidden".into(), Node::Int(0)),
        ("AllowDesktopConfig".into(), Node::Int(1)),
        ("AllowOverlay".into(), Node::Int(1)),
        ("OpenVR".into(), Node::Int(0)),
        ("Devkit".into(), Node::Int(0)),
        ("DevkitGameID".into(), s("")),
        ("DevkitOverrideAppID".into(), Node::Int(0)),
        ("LastPlayTime".into(), Node::Int(0)),
        ("FlatpakAppID".into(), s("")),
        ("sortas".into(), s("")),
        ("tags".into(), Node::Map(Vec::new())),
    ];
    list.push((next.to_string(), Node::Map(entry)));
    let bytes = write(&root);
    // Check: what we write reads back.
    parse(&bytes)?;
    let bak = path.with_file_name("shortcuts.vdf.vaporust.bak");
    if path.exists() && !bak.exists() {
        std::fs::copy(path, &bak).map_err(|e| trf!("копия: {e}", "backup: {e}"))?;
    }
    let tmp = path.with_file_name("shortcuts.vdf.vaporust.tmp");
    std::fs::write(&tmp, &bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_add() {
        let dir = std::env::temp_dir().join(format!("vaporust_sc_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("shortcuts.vdf");
        assert!(add(&p, "Игра", r"C:\Games\Игра\game.exe", "-x").unwrap());
        assert!(!add(&p, "Игра", r"c:\games\игра\GAME.exe", "").unwrap(), "a duplicate is not added");
        assert!(add(&p, "Вторая", r"C:\B\b.exe", "").unwrap());
        let v = read(&p).unwrap();
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].name, "Игра");
        assert_eq!(v[0].exe, r"C:\Games\Игра\game.exe");
        assert_eq!(v[0].start_dir, r"C:\Games\Игра\");
        assert_eq!(v[0].args, "-x");
        // Reading and writing without changes gives the same bytes.
        let b = std::fs::read(&p).unwrap();
        assert_eq!(write(&parse(&b).unwrap()), b);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The user's real file: only reading and comparing bytes after writing into memory.
    #[test]
    #[ignore]
    fn real_file_roundtrip() {
        let steam = crate::local::find_steam().unwrap();
        let p = path(&steam.dir).unwrap();
        let b = std::fs::read(&p).unwrap();
        assert_eq!(write(&parse(&b).unwrap()), b, "writing without changes must match byte for byte");
        for s in read(&p).unwrap() {
            println!("{} | {} | {}", s.name, s.exe, s.start_dir);
        }
    }
}
