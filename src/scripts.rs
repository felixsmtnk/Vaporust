//! Launch scripts: own commands before a game launches and after it exits (per game).
//!
//! In Aurelia a script wraps the launch command (Linux). Vaporust launches games through Steam,
//! so instead of a wrapper there are commands before and after: for example start a mod tool,
//! change the screen resolution, close unneeded programs. The command runs as in cmd.exe
//! (several via && are allowed, a .bat can be started), in the game folder. Environment variables:
//! VAPORUST_APPID, VAPORUST_GAME_NAME, VAPORUST_GAME_DIR.

use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct GameScripts {
    /// Command before launch; the game starts when it finishes (up to 2 minutes).
    pub before: String,
    /// Command after the game exits (Vaporust does not wait for it).
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
        // The command itself without a console window; programs it starts get their own windows.
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

/// Run the "before launch" command and wait for it (at most 2 minutes).
/// Ok(None) - all good, Ok(Some(text)) - a warning (the launch continues).
pub fn run_before(s: &GameScripts, appid: u32, name: &str, dir: &Path) -> Option<String> {
    let text = s.before.trim();
    if text.is_empty() {
        return None;
    }
    let mut child = match command(text, appid, name, dir).spawn() {
        Ok(c) => c,
        Err(e) => return Some(trf!("Команда перед запуском не выполнена: {e}", "The pre-launch command did not run: {e}")),
    };
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(st)) if st.success() => return None,
            Ok(Some(st)) => {
                return Some(trf!("Команда перед запуском завершилась с кодом {}", "The pre-launch command ended with code {}", st.code().unwrap_or(-1)))
            }
            Ok(None) if start.elapsed() > Duration::from_secs(120) => {
                return Some(tr!("Команда перед запуском идет дольше 2 минут: запускаю игру, не дожидаясь ее.", "The pre-launch command takes longer than 2 minutes: starting the game without waiting for it.").into())
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(200)),
            Err(e) => return Some(trf!("Команда перед запуском: {e}", "Pre-launch command: {e}")),
        }
    }
}

/// Start the "after exit" command (without waiting for it).
pub fn run_after(s: &GameScripts, appid: u32, name: &str, dir: &Path) -> Option<String> {
    let text = s.after.trim();
    if text.is_empty() {
        return None;
    }
    command(text, appid, name, dir).spawn().err().map(|e| trf!("Команда после выхода не выполнена: {e}", "The after-exit command did not run: {e}"))
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
