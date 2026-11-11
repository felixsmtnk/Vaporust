//! Параметры запуска игр: поле LaunchOptions в localconfig.vdf пользователя Steam.
//!
//! Путь в файле: UserLocalConfigStore / Software / Valve / Steam / apps / <appid> / LaunchOptions.
//! Steam перезаписывает этот файл при выходе, поэтому сохранять можно только при закрытом Steam.
//! Файл правится построчно: всё, кроме одной нужной строки, остаётся байт в байт.

use std::{
    fs,
    path::{Path, PathBuf},
};

const APPS_PATH: [&str; 5] = ["UserLocalConfigStore", "Software", "Valve", "Steam", "apps"];

/// localconfig.vdf текущего пользователя: тот, что менялся последним
/// (Steam переписывает его при каждом выходе вошедшего аккаунта).
pub fn find_localconfig(steam_dir: &Path) -> Option<PathBuf> {
    fs::read_dir(steam_dir.join("userdata"))
        .ok()?
        .flatten()
        .map(|e| e.path().join("config").join("localconfig.vdf"))
        .filter_map(|p| {
            let t = fs::metadata(&p).ok()?.modified().ok()?;
            Some((t, p))
        })
        .max_by_key(|(t, _)| *t)
        .map(|(_, p)| p)
}

/// Строки в кавычках из строки VDF (с разбором \" и \\).
fn tokens(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c == '/' && line.trim_start().starts_with("//") {
            break;
        }
        if c != '"' {
            continue;
        }
        let mut s = String::new();
        while let Some(c) = chars.next() {
            match c {
                '\\' => match chars.next() {
                    Some('n') => s.push('\n'),
                    Some('t') => s.push('\t'),
                    Some(o) => s.push(o),
                    None => {}
                },
                '"' => break,
                _ => s.push(c),
            }
        }
        out.push(s);
    }
    out
}

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Где в файле лежит блок игры и её строка LaunchOptions.
#[derive(Default)]
struct Found {
    /// Строка с закрывающей скобкой блока apps.
    apps_close: Option<usize>,
    /// Строка с закрывающей скобкой блока игры.
    app_close: Option<usize>,
    /// Строка "LaunchOptions" внутри блока игры и её значение.
    launch: Option<(usize, String)>,
}

fn locate(lines: &[&str], appid: u32) -> Found {
    let appid = appid.to_string();
    let mut stack: Vec<String> = Vec::new();
    let mut pending: Option<String> = None;
    let mut found = Found::default();

    let is_apps = |st: &[String]| {
        st.len() == APPS_PATH.len()
            && st.iter().zip(APPS_PATH).all(|(a, b)| a.eq_ignore_ascii_case(b))
    };
    let is_app = |st: &[String]| st.len() == APPS_PATH.len() + 1 && is_apps(&st[..APPS_PATH.len()]) && st[APPS_PATH.len()] == appid;

    for (i, line) in lines.iter().enumerate() {
        let t = line.trim();
        if t == "{" {
            stack.push(pending.take().unwrap_or_default());
            continue;
        }
        if t == "}" {
            if is_app(&stack) {
                found.app_close = Some(i);
            } else if is_apps(&stack) {
                found.apps_close = Some(i);
            }
            stack.pop();
            continue;
        }
        let tk = tokens(t);
        match tk.len() {
            1 => pending = Some(tk[0].clone()),
            2.. => {
                if is_app(&stack) && tk[0].eq_ignore_ascii_case("LaunchOptions") {
                    found.launch = Some((i, tk[1].clone()));
                }
            }
            _ => {}
        }
    }
    found
}

/// Текущие параметры запуска игры ("" если не заданы).
pub fn read(localconfig: &Path, appid: u32) -> Result<String, String> {
    let text = fs::read_to_string(localconfig)
        .map_err(|e| format!("Не удалось прочитать настройки Steam: {e}"))?;
    let lines: Vec<&str> = text.split('\n').collect();
    Ok(locate(&lines, appid).launch.map(|(_, v)| v).unwrap_or_default())
}

/// Записать параметры запуска. Steam должен быть закрыт.
pub fn write(localconfig: &Path, appid: u32, value: &str) -> Result<(), String> {
    let text = fs::read_to_string(localconfig)
        .map_err(|e| format!("Не удалось прочитать настройки Steam: {e}"))?;
    let lines: Vec<&str> = text.split('\n').collect();
    let found = locate(&lines, appid);
    let mut out: Vec<String> = lines.iter().map(|s| s.to_string()).collect();
    let tabs = |n: usize| "\t".repeat(n);
    let depth = APPS_PATH.len() + 1; // глубина содержимого блока игры
    let value = value.trim().replace(['\r', '\n'], " ");
    let kv = format!("{}\"LaunchOptions\"\t\t\"{}\"", tabs(depth), escape(&value));

    if let Some((i, _)) = found.launch {
        // Сохраняем отступ, как был в файле.
        let indent: String = lines[i].chars().take_while(|c| c.is_whitespace()).collect();
        out[i] = format!("{indent}\"LaunchOptions\"\t\t\"{}\"", escape(&value));
    } else if value.is_empty() {
        return Ok(()); // нечего удалять
    } else if let Some(i) = found.app_close {
        out.insert(i, kv);
    } else if let Some(i) = found.apps_close {
        let block = [
            format!("{}\"{appid}\"", tabs(depth - 1)),
            format!("{}{{", tabs(depth - 1)),
            kv,
            format!("{}}}", tabs(depth - 1)),
        ];
        for (k, l) in block.into_iter().enumerate() {
            out.insert(i + k, l);
        }
    } else {
        return Err("В настройках Steam не найден раздел apps. Запустите Steam один раз обычным способом.".into());
    }

    // Резервная копия до первой нашей правки, затем запись через временный файл.
    let backup = localconfig.with_extension("vdf.vaporust.bak");
    if !backup.exists() {
        fs::copy(localconfig, &backup).map_err(|e| format!("Не удалось сделать резервную копию: {e}"))?;
    }
    let tmp = localconfig.with_extension("vdf.vaporust.tmp");
    fs::write(&tmp, out.join("\n")).map_err(|e| format!("Не удалось записать настройки: {e}"))?;
    fs::rename(&tmp, localconfig).map_err(|e| format!("Не удалось записать настройки: {e}"))?;

    // Проверка: читаем обратно.
    let back = read(localconfig, appid)?;
    if back != value {
        return Err(format!("После записи прочиталось «{back}» вместо «{value}»."));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\"UserLocalConfigStore\"\n{\n\t\"Software\"\n\t{\n\t\t\"Valve\"\n\t\t{\n\t\t\t\"Steam\"\n\t\t\t{\n\t\t\t\t\"apps\"\n\t\t\t\t{\n\t\t\t\t\t\"70\"\n\t\t\t\t\t{\n\t\t\t\t\t\t\"Playtime\"\t\t\"979\"\n\t\t\t\t\t\t\"LaunchOptions\"\t\t\"-gl\"\n\t\t\t\t\t}\n\t\t\t\t\t\"80\"\n\t\t\t\t\t{\n\t\t\t\t\t\t\"Playtime\"\t\t\"50\"\n\t\t\t\t\t}\n\t\t\t\t}\n\t\t\t}\n\t\t}\n\t}\n\t\"apps\"\n\t{\n\t\t\"70\"\n\t\t{\n\t\t\t\"LaunchOptions\"\t\t\"wrong\"\n\t\t}\n\t}\n}\n";

    fn tmp_file(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("vaporust_test_{name}_{}.vdf", std::process::id()));
        fs::write(&p, SAMPLE).unwrap();
        p
    }

    #[test]
    fn read_existing_and_missing() {
        let p = tmp_file("read");
        assert_eq!(read(&p, 70).unwrap(), "-gl");
        assert_eq!(read(&p, 80).unwrap(), "");
        assert_eq!(read(&p, 90).unwrap(), "");
    }

    #[test]
    fn write_replace_insert_new() {
        let p = tmp_file("write");
        write(&p, 70, "-novid \"a b\" C:\\x").unwrap();
        assert_eq!(read(&p, 70).unwrap(), "-novid \"a b\" C:\\x");
        write(&p, 80, "-high").unwrap();
        assert_eq!(read(&p, 80).unwrap(), "-high");
        write(&p, 90, "-w 800").unwrap();
        assert_eq!(read(&p, 90).unwrap(), "-w 800");
        // Второй раздел apps (не тот путь) не тронут.
        assert!(fs::read_to_string(&p).unwrap().contains("\"wrong\""));
        write(&p, 70, "").unwrap();
        assert_eq!(read(&p, 70).unwrap(), "");
    }
}

#[cfg(test)]
mod real_file {
    /// Ручная проверка на копии настоящего файла:
    /// VAPORUST_LC=<копия localconfig.vdf> cargo test real_copy -- --ignored
    #[test]
    #[ignore]
    fn real_copy() {
        let p = std::path::PathBuf::from(std::env::var("VAPORUST_LC").unwrap());
        println!("70 = {:?}", super::read(&p, 70).unwrap());
        super::write(&p, 70, "-gl -novid").unwrap();
        super::write(&p, 3620, "-test").unwrap();
    }
}
