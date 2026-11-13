//! Проверка обновлений Vaporust и автообновление с GitHub Releases.
//!
//! Раз при запуске (если включено в настройках) спрашиваем GitHub API о релизах,
//! находим новее текущей версии и архив под нашу разрядность (windows-x64 / windows-x86).
//! «Обновить»: скачиваем zip, распаковываем сами (библиотека zip), переименовываем
//! работающий vaporust.exe в vaporust.exe.old (Windows это позволяет), кладем новый и
//! перезапускаемся. Старый файл удаляется при следующем запуске.

use serde_json::Value;
use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const API: &str = "https://api.github.com/repos/felixsmtnk/Vaporust/releases?per_page=10";
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

/// Имя архива под нашу разрядность.
pub fn asset_suffix() -> &'static str {
    if cfg!(target_pointer_width = "64") {
        "windows-x64.zip"
    } else {
        "windows-x86.zip"
    }
}

#[derive(Clone, Debug)]
pub struct Release {
    pub version: String,
    pub notes: String,
    pub page: String,
    pub asset_url: Option<String>,
    pub asset_size: u64,
}

#[derive(Clone, Debug, Default)]
pub enum State {
    #[default]
    Idle,
    Checking,
    UpToDate,
    Available(Release),
    Downloading(String),
    /// Новая версия на месте - нужен перезапуск.
    Ready,
    Failed(String),
}

pub type Shared = Arc<Mutex<State>>;

/// Сравнить версии вида 0.2.0-alpha, 0.1.0-alpha.3, 1.0.0. Версия без «-…» новее
/// такой же с «-…»; части после «-» сравниваются по точкам (числа - как числа).
pub fn compare(a: &str, b: &str) -> Ordering {
    fn split(v: &str) -> (Vec<u64>, Option<Vec<String>>) {
        let v = v.trim().trim_start_matches('v');
        let (core, pre) = match v.split_once('-') {
            Some((c, p)) => (c, Some(p.split('.').map(str::to_string).collect())),
            None => (v, None),
        };
        (core.split('.').map(|n| n.parse().unwrap_or(0)).collect(), pre)
    }
    let (ca, pa) = split(a);
    let (cb, pb) = split(b);
    for i in 0..ca.len().max(cb.len()) {
        let o = ca.get(i).unwrap_or(&0).cmp(cb.get(i).unwrap_or(&0));
        if o != Ordering::Equal {
            return o;
        }
    }
    match (pa, pb) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(x), Some(y)) => {
            for i in 0..x.len().max(y.len()) {
                let o = match (x.get(i), y.get(i)) {
                    (None, None) => Ordering::Equal,
                    (None, Some(_)) => Ordering::Less,
                    (Some(_), None) => Ordering::Greater,
                    (Some(p), Some(q)) => match (p.parse::<u64>(), q.parse::<u64>()) {
                        (Ok(m), Ok(n)) => m.cmp(&n),
                        (Ok(_), Err(_)) => Ordering::Less,
                        (Err(_), Ok(_)) => Ordering::Greater,
                        _ => p.cmp(q),
                    },
                };
                if o != Ordering::Equal {
                    return o;
                }
            }
            Ordering::Equal
        }
    }
}

fn get(url: &str) -> Result<Vec<u8>, String> {
    let mut req = ehttp::Request::get(url);
    req.headers = ehttp::Headers::new(&[
        ("User-Agent", "Vaporust-updater"),
        ("Accept", "application/vnd.github+json"),
    ]);
    let resp = ehttp::fetch_blocking(&req).map_err(|e| format!("нет связи с GitHub: {e}"))?;
    if !resp.ok {
        return Err(format!("GitHub ответил {} {}", resp.status, resp.status_text));
    }
    Ok(resp.bytes)
}

/// Самый новый релиз новее текущей версии (черновики пропускаем).
pub fn parse_latest(json: &[u8], current: &str) -> Result<Option<Release>, String> {
    let v: Value = serde_json::from_slice(json).map_err(|_| "непонятный ответ GitHub".to_string())?;
    let mut best: Option<Release> = None;
    for r in v.as_array().into_iter().flatten() {
        if r.get("draft").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        let tag = r.get("tag_name").and_then(Value::as_str).unwrap_or_default();
        let ver = tag.trim_start_matches('v').to_string();
        if compare(&ver, current) != Ordering::Greater {
            continue;
        }
        if best.as_ref().is_some_and(|b| compare(&ver, &b.version) != Ordering::Greater) {
            continue;
        }
        let asset = r
            .get("assets")
            .and_then(Value::as_array)
            .and_then(|a| {
                a.iter().find(|x| {
                    x.get("name").and_then(Value::as_str).is_some_and(|n| n.ends_with(asset_suffix()))
                })
            });
        best = Some(Release {
            version: ver,
            notes: r.get("body").and_then(Value::as_str).unwrap_or_default().to_string(),
            page: r.get("html_url").and_then(Value::as_str).unwrap_or_default().to_string(),
            asset_url: asset
                .and_then(|a| a.get("browser_download_url"))
                .and_then(Value::as_str)
                .map(str::to_string),
            asset_size: asset.and_then(|a| a.get("size")).and_then(Value::as_u64).unwrap_or(0),
        });
    }
    Ok(best)
}

/// Проверить обновления в фоне.
pub fn check(state: &Shared, ctx: &eframe::egui::Context) {
    *state.lock().unwrap() = State::Checking;
    let (state, ctx) = (state.clone(), ctx.clone());
    std::thread::spawn(move || {
        let res = get(API).and_then(|b| parse_latest(&b, CURRENT));
        *state.lock().unwrap() = match res {
            Ok(Some(r)) => State::Available(r),
            Ok(None) => State::UpToDate,
            Err(e) => State::Failed(format!("Не удалось проверить обновления: {e}")),
        };
        ctx.request_repaint();
    });
}

/// Скачать и поставить новую версию (в фоне). После State::Ready нужен перезапуск.
pub fn install(release: Release, state: &Shared, ctx: &eframe::egui::Context) {
    let (state, ctx) = (state.clone(), ctx.clone());
    std::thread::spawn(move || {
        let set = |s: State| {
            *state.lock().unwrap() = s;
            ctx.request_repaint();
        };
        set(State::Downloading(format!("Скачиваю Vaporust {}...", release.version)));
        let res = (|| -> Result<(), String> {
            let url = release.asset_url.clone().ok_or("в релизе нет архива для этой Windows")?;
            let bytes = get(&url)?;
            set(State::Downloading("Устанавливаю...".into()));
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            replace_exe(&exe, &bytes)
        })();
        set(match res {
            Ok(()) => State::Ready,
            Err(e) => State::Failed(format!("Обновление не удалось: {e}")),
        });
    });
}

/// Распаковать архив и заменить exe (работающий файл переименовываем в .old).
pub fn replace_exe(exe: &Path, zip: &[u8]) -> Result<(), String> {
    use std::io::Read;
    // Распаковываем сами (библиотека zip): системный tar.exe не понимает не-латинские буквы в
    // путях (например, «C:\Users\Иван\...») на Windows с другим языком.
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(zip)).map_err(|e| format!("не удалось открыть архив: {e}"))?;
    let idx = (0..z.len())
        .find(|&i| z.by_index(i).map(|f| f.name().to_ascii_lowercase().ends_with("vaporust.exe")).unwrap_or(false))
        .ok_or("в архиве нет vaporust.exe")?;
    let mut data = Vec::new();
    z.by_index(idx)
        .map_err(|e| format!("не удалось распаковать архив: {e}"))?
        .read_to_end(&mut data)
        .map_err(|e| format!("не удалось распаковать архив: {e}"))?;
    let tmp = std::env::temp_dir().join(format!("vaporust-update-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    let new_exe = tmp.join("vaporust.exe");
    std::fs::write(&new_exe, data).map_err(|e| e.to_string())?;
    let old = exe.with_extension("exe.old");
    let _ = std::fs::remove_file(&old);
    std::fs::rename(exe, &old).map_err(|e| format!("не удалось заменить программу (папка доступна для записи?): {e}"))?;
    if let Err(e) = std::fs::copy(&new_exe, exe) {
        // Вернуть как было.
        let _ = std::fs::rename(&old, exe);
        return Err(format!("не удалось записать новую версию: {e}"));
    }
    let _ = std::fs::remove_dir_all(&tmp);
    Ok(())
}

/// При запуске: убрать файл старой версии, оставшийся после обновления.
pub fn cleanup_old() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(exe.with_extension("exe.old"));
    }
}

impl crate::LauncherApp {
    /// Полоса над главным окном: «Доступна новая версия», ход установки, перезапуск.
    pub(crate) fn update_banner(&mut self, ctx: &eframe::egui::Context) {
        use eframe::egui;
        let state = self.upd.lock().unwrap().clone();
        let show = match &state {
            State::Available(_) => !self.upd_hidden,
            State::Downloading(_) | State::Ready => true,
            State::Failed(_) | State::UpToDate => self.upd_manual && !self.upd_hidden,
            _ => false,
        };
        if !show {
            return;
        }
        egui::TopBottomPanel::top("update_banner").show(ctx, |ui| {
            ui.add_space(3.0);
            ui.horizontal(|ui| match &state {
                State::Available(r) => {
                    ui.colored_label(egui::Color32::from_rgb(240, 200, 80), format!("Доступна новая версия Vaporust {}", r.version));
                    if r.asset_size > 0 {
                        ui.weak(format!("({})", crate::net::fmt_bytes(r.asset_size)));
                    }
                    let notes: String = r.notes.lines().take(25).collect::<Vec<_>>().join("\n");
                    if !r.page.is_empty() && ui.button("Что нового").on_hover_text(notes).clicked() {
                        ui.ctx().open_url(egui::OpenUrl::new_tab(&r.page));
                    }
                    if r.asset_url.is_some() {
                        if ui.button("Обновить").on_hover_text("Скачать и установить, затем перезапустить Vaporust").clicked() {
                            install(r.clone(), &self.upd, ui.ctx());
                        }
                    } else {
                        ui.weak("(архива для этой Windows в релизе нет - скачайте со страницы)");
                    }
                    if ui.button("Позже").clicked() {
                        self.upd_hidden = true;
                    }
                }
                State::Downloading(t) => {
                    crate::spin::spinner(ui);
                    ui.label(t);
                }
                State::Ready => {
                    ui.colored_label(egui::Color32::from_rgb(110, 200, 110), "Новая версия установлена.");
                    if ui.button("Перезапустить Vaporust").clicked() {
                        if crate::portable::restart().is_ok() {
                            crate::tray::request_exit();
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    }
                }
                State::UpToDate => {
                    ui.label(format!("У вас последняя версия Vaporust ({CURRENT})."));
                    if ui.small_button("OK").clicked() {
                        self.upd_hidden = true;
                    }
                }
                State::Failed(e) => {
                    ui.colored_label(egui::Color32::from_rgb(230, 110, 110), e);
                    if ui.small_button("OK").clicked() {
                        self.upd_hidden = true;
                    }
                }
                _ => {}
            });
            ui.add_space(3.0);
        });
    }

    /// Проверить обновления по кнопке (показать и «нет обновлений», и ошибку).
    pub(crate) fn check_updates_now(&mut self, ctx: &eframe::egui::Context) {
        self.upd_manual = true;
        self.upd_hidden = false;
        check(&self.upd, ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert_eq!(compare("0.2.0-alpha", "0.1.0-alpha.4"), Ordering::Greater);
        assert_eq!(compare("0.1.0-alpha.10", "0.1.0-alpha.9"), Ordering::Greater);
        assert_eq!(compare("0.2.0", "0.2.0-beta"), Ordering::Greater);
        assert_eq!(compare("0.2.0-beta", "0.2.0-alpha"), Ordering::Greater);
        assert_eq!(compare("v0.2.0-alpha", "0.2.0-alpha"), Ordering::Equal);
        assert_eq!(compare("0.2.0-alpha.1", "0.2.0-alpha"), Ordering::Greater);
    }

    #[test]
    fn latest_release() {
        let json = br#"[
          {"tag_name":"v0.1.0-alpha.3","draft":false,"body":"old","html_url":"u3","assets":[{"name":"Vaporust-0.1.0-alpha.3-windows-x64.zip","browser_download_url":"d3","size":10}]},
          {"tag_name":"v0.3.0-alpha","draft":true,"assets":[]},
          {"tag_name":"v0.2.1-alpha","draft":false,"body":"new","html_url":"u21","assets":[{"name":"Vaporust-0.2.1-alpha-windows-x64.zip","browser_download_url":"d21","size":20},{"name":"Vaporust-0.2.1-alpha-windows-x86.zip","browser_download_url":"d21x86","size":19}]}
        ]"#;
        let r = parse_latest(json, "0.2.0-alpha").unwrap().unwrap();
        assert_eq!(r.version, "0.2.1-alpha");
        assert_eq!(r.asset_url.as_deref(), Some(if cfg!(target_pointer_width = "64") { "d21" } else { "d21x86" }));
        assert!(parse_latest(json, "0.2.1-alpha").unwrap().is_none());
    }

    /// Живой тест: запрос к GitHub (только чтение).
    #[test]
    #[ignore]
    fn github_live() {
        let b = get(API).unwrap();
        println!("{:?}", parse_latest(&b, "0.0.1").unwrap());
        println!("{:?}", parse_latest(&b, CURRENT).unwrap());
    }

    /// Живой тест замены exe на копии в папке VAPORUST_DUMP: скачать alpha.3 и подменить.
    #[test]
    #[ignore]
    fn replace_live() {
        let dir = PathBuf::from(std::env::var("VAPORUST_DUMP").unwrap());
        let exe = dir.join("vaporust.exe");
        std::fs::write(&exe, b"old exe").unwrap();
        let rel = parse_latest(&get(API).unwrap(), "0.0.1").unwrap().unwrap();
        let zip = get(rel.asset_url.as_deref().unwrap()).unwrap();
        replace_exe(&exe, &zip).unwrap();
        let len = std::fs::metadata(&exe).unwrap().len();
        println!("new exe {len} bytes, old exists: {}", exe.with_extension("exe.old").exists());
        assert!(len > 1_000_000);
    }
}
