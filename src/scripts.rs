//! Скрипты запуска: свои команды перед запуском игры и после ее выхода (для каждой игры свои).
//!
//! В Aurelia скрипт оборачивает команду запуска (Linux). Vaporust запускает игры через Steam,
//! поэтому вместо обертки - команды до и после: например, включить программу для модов,
//! сменить разрешение экрана, закрыть лишние программы. Команда выполняется как в cmd.exe
//! (можно несколько через &&, можно запустить .bat), в папке игры. Переменные окружения:
//! VAPORUST_APPID, VAPORUST_GAME_NAME, VAPORUST_GAME_DIR.

use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct GameScripts {
    /// Команда перед запуском; игра запускается, когда она закончится (до 2 минут).
    pub before: String,
    /// Команда после выхода из игры (Vaporust ее не ждет).
    pub after: String,
}

impl GameScripts {
    pub fn is_empty(&self) -> bool {
        self.before.trim().is_empty() && self.after.trim().is_empty()
    }
}

fn command(text: &str, appid: u32, name: &str, dir: &Path) -> Command {
    let mut c = Command::new("cmd.exe");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // Сама команда без окна консоли; программы, которые она запускает, - со своими окнами.
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        c.raw_arg(format!("/D /C {text}"));
    }
    #[cfg(not(windows))]
    c.args(["/C", text]);
    if dir.is_dir() {
        c.current_dir(dir);
    }
    c.env("VAPORUST_APPID", appid.to_string()).env("VAPORUST_GAME_NAME", name).env("VAPORUST_GAME_DIR", dir);
    c
}

/// Выполнить команду «перед запуском» и дождаться ее (не дольше 2 минут).
/// Ok(None) - все хорошо, Ok(Some(текст)) - предупреждение (запуск продолжается).
pub fn run_before(s: &GameScripts, appid: u32, name: &str, dir: &Path) -> Option<String> {
    let text = s.before.trim();
    if text.is_empty() {
        return None;
    }
    let mut child = match command(text, appid, name, dir).spawn() {
        Ok(c) => c,
        Err(e) => return Some(format!("Команда перед запуском не выполнена: {e}")),
    };
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(st)) if st.success() => return None,
            Ok(Some(st)) => {
                return Some(format!("Команда перед запуском завершилась с кодом {}", st.code().unwrap_or(-1)))
            }
            Ok(None) if start.elapsed() > Duration::from_secs(120) => {
                return Some("Команда перед запуском идет дольше 2 минут: запускаю игру, не дожидаясь ее.".into())
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(200)),
            Err(e) => return Some(format!("Команда перед запуском: {e}")),
        }
    }
}

/// Запустить команду «после выхода» (не дожидаясь ее).
pub fn run_after(s: &GameScripts, appid: u32, name: &str, dir: &Path) -> Option<String> {
    let text = s.after.trim();
    if text.is_empty() {
        return None;
    }
    command(text, appid, name, dir).spawn().err().map(|e| format!("Команда после выхода не выполнена: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn before_and_after() {
        let dir = std::env::temp_dir().join(format!("vaporust_scripts_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let s = GameScripts {
            before: "echo %VAPORUST_APPID% %VAPORUST_GAME_NAME%> before.txt".into(),
            after: "echo done> \"after file.txt\"".into(),
        };
        assert_eq!(run_before(&s, 70, "Half-Life", &dir), None);
        assert_eq!(std::fs::read_to_string(dir.join("before.txt")).unwrap().trim(), "70 Half-Life");
        assert_eq!(run_after(&s, 70, "Half-Life", &dir), None);
        let t = Instant::now();
        while !dir.join("after file.txt").exists() && t.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(100));
        }
        assert!(dir.join("after file.txt").exists());
        let bad = GameScripts { before: "exit 3".into(), ..Default::default() };
        assert!(run_before(&bad, 70, "x", &dir).unwrap().contains("кодом 3"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
