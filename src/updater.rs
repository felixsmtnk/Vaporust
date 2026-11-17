//! Checking for Vaporust updates and auto-updating from GitHub Releases.
//!
//! Once at startup (if enabled in settings) we ask the GitHub API about releases,
//! find one newer than the current version and the archive for our bitness (windows-x64 / windows-x86).
//! "Update": download the zip, unpack it ourselves (zip crate), rename
//! the running vaporust.exe to vaporust.exe.old (Windows allows that), put the new one in place and
//! restart. The old file is deleted on the next start.

use serde_json::Value;
use std::cmp::Ordering;
use std::path::Path;
use std::sync::{Arc, Mutex};

const API: &str = "https://api.github.com/repos/felixsmtnk/Vaporust/releases?per_page=10";
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

/// Archive name for our bitness.
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
    /// The "<archive>.sha256" file from the same release (GitHub Actions builds it together with the archive).
    pub sha_url: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub enum State {
    #[default]
    Idle,
    Checking,
    UpToDate,
    Available(Release),
    Downloading(String),
    /// The new version is in place - a restart is needed.
    Ready,
    Failed(String),
}

pub type Shared = Arc<Mutex<State>>;

/// Compare versions like 0.2.0-alpha, 0.1.0-alpha.3, 1.0.0. A version without "-..." is newer
/// than the same with "-..."; parts after "-" are compared by dots (numbers as numbers).
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
    let resp = ehttp::fetch_blocking(&req).map_err(|e| trf!("нет связи с GitHub: {e}", "no connection to GitHub: {e}"))?;
    if !resp.ok {
        return Err(trf!("GitHub ответил {} {}", "GitHub answered {} {}", resp.status, resp.status_text));
    }
    Ok(resp.bytes)
}

/// The newest release newer than the current version (drafts are skipped).
pub fn parse_latest(json: &[u8], current: &str) -> Result<Option<Release>, String> {
    let v: Value = serde_json::from_slice(json).map_err(|_| tr!("непонятный ответ GitHub", "unclear answer from GitHub").to_string())?;
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
        let assets = r.get("assets").and_then(Value::as_array);
        let name_of = |x: &Value| x.get("name").and_then(Value::as_str).unwrap_or_default().to_string();
        let asset = assets.and_then(|a| a.iter().find(|x| name_of(x).ends_with(asset_suffix())));
        let sha_url = asset.and_then(|asset| {
            let want = format!("{}.sha256", name_of(asset));
            assets?
                .iter()
                .find(|x| name_of(x) == want)
                .and_then(|x| x.get("browser_download_url"))
                .and_then(Value::as_str)
                .map(str::to_string)
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
            sha_url,
        });
    }
    Ok(best)
}

/// Check for updates in the background.
pub fn check(state: &Shared, ctx: &eframe::egui::Context) {
    *state.lock().unwrap() = State::Checking;
    let (state, ctx) = (state.clone(), ctx.clone());
    std::thread::spawn(move || {
        let res = get(API).and_then(|b| parse_latest(&b, CURRENT));
        *state.lock().unwrap() = match res {
            Ok(Some(r)) => State::Available(r),
            Ok(None) => State::UpToDate,
            Err(e) => State::Failed(trf!("Не удалось проверить обновления: {e}", "Could not check for updates: {e}")),
        };
        ctx.request_repaint();
    });
}

/// Download and install the new version (in the background). After State::Ready a restart is needed.
pub fn install(release: Release, state: &Shared, ctx: &eframe::egui::Context) {
    let (state, ctx) = (state.clone(), ctx.clone());
    std::thread::spawn(move || {
        let set = |s: State| {
            *state.lock().unwrap() = s;
            ctx.request_repaint();
        };
        set(State::Downloading(trf!("Скачиваю Vaporust {}...", "Downloading Vaporust {}...", release.version)));
        let res = (|| -> Result<(), String> {
            let url = release.asset_url.clone().ok_or(tr!("в релизе нет архива для этой Windows", "the release has no archive for this Windows"))?;
            let sha_url = release
                .sha_url
                .clone()
                .ok_or(tr!("в релизе нет контрольной суммы архива (.sha256) - скачайте новую версию со страницы релиза вручную", "the release has no archive checksum (.sha256) - download the new version from the release page manually"))?;
            let expected = parse_sha(&get(&sha_url)?)?;
            let bytes = get(&url)?;
            verify_sha(&bytes, &expected)?;
            set(State::Downloading(tr!("Устанавливаю...", "Installing...").into()));
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            replace_exe(&exe, &bytes)
        })();
        set(match res {
            Ok(()) => State::Ready,
            Err(e) => State::Failed(trf!("Обновление не удалось: {e}", "Update failed: {e}")),
        });
    });
}

/// Contents of a .sha256 file: 64 hex characters (any case; spaces and a file name
/// after the sum are allowed, as with sha256sum).
fn parse_sha(text: &[u8]) -> Result<String, String> {
    let text = String::from_utf8_lossy(text);
    let first = text.split_whitespace().next().unwrap_or_default().trim_start_matches('\u{feff}');
    if first.len() == 64 && first.chars().all(|c| c.is_ascii_hexdigit()) {
        Ok(first.to_ascii_lowercase())
    } else {
        Err(tr!("непонятный файл контрольной суммы", "unclear checksum file").into())
    }
}

/// Compare the SHA-256 of the downloaded archive with the published sum. A mismatch - the archive is damaged
/// or tampered with, it must not be installed.
fn verify_sha(data: &[u8], expected: &str) -> Result<(), String> {
    use sha2::{Digest, Sha256};
    let got: String = Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect();
    if got.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err(tr!("контрольная сумма архива не совпала (архив поврежден при скачивании или подменен), обновление не установлено", "the archive checksum did not match (the archive was damaged while downloading or replaced), the update was not installed").into())
    }
}

/// Exe bitness from the PE header: Some(true) - 64 bit (x64), Some(false) - 32 bit (x86).
/// Only for a whole executable: an executable image (not a DLL), an optional header that matches
/// the machine, sections that lie inside the file. 70 bytes with the right start are not a program.
fn pe_is_64(data: &[u8]) -> Option<bool> {
    let u16_at = |o: usize| data.get(o..o.checked_add(2)?).map(|b| u16::from_le_bytes([b[0], b[1]]));
    let u32_at = |o: usize| data.get(o..o.checked_add(4)?).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    if data.get(..2)? != b"MZ" {
        return None;
    }
    let pe = u32_at(0x3c)? as usize;
    if data.get(pe..pe.checked_add(4)?)? != b"PE\0\0" {
        return None;
    }
    let is64 = match u16_at(pe + 4)? {
        0x8664 => true,
        0x014c => false,
        _ => return None,
    };
    let sections = u16_at(pe + 6)? as usize;
    let opt_size = u16_at(pe + 20)? as usize;
    let characteristics = u16_at(pe + 22)?;
    // IMAGE_FILE_EXECUTABLE_IMAGE set, IMAGE_FILE_DLL not.
    if characteristics & 0x0002 == 0 || characteristics & 0x2000 != 0 || sections == 0 || sections > 96 {
        return None;
    }
    let opt = pe + 24;
    let magic = u16_at(opt)?;
    if (is64 && magic != 0x20b) || (!is64 && magic != 0x10b) || opt_size < if is64 { 240 } else { 224 } {
        return None;
    }
    let table = opt + opt_size;
    for i in 0..sections {
        let s = table + i * 40;
        let raw_size = u32_at(s + 16)? as usize;
        let raw_ptr = u32_at(s + 20)? as usize;
        if raw_size > 0 && raw_ptr.checked_add(raw_size)? > data.len() {
            return None;
        }
    }
    Some(is64)
}

/// The program is Vaporust: its version resource names the product (UTF-16 "Vaporust").
fn is_vaporust(data: &[u8]) -> bool {
    let needle: Vec<u8> = "Vaporust".encode_utf16().flat_map(|c| c.to_le_bytes()).collect();
    data.windows(needle.len()).any(|w| w == needle.as_slice())
}

/// Unpack the archive and replace the exe (the running file is renamed to .old).
pub fn replace_exe(exe: &Path, zip: &[u8]) -> Result<(), String> {
    use std::io::Read;
    // We unpack ourselves (zip crate): the system tar.exe does not understand non-Latin letters in
    // paths (for example "C:\Users\Иван\...") on Windows with a different language.
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(zip)).map_err(|e| trf!("не удалось открыть архив: {e}", "could not open the archive: {e}"))?;
    let idx = (0..z.len())
        .find(|&i| z.by_index(i).map(|f| f.name().to_ascii_lowercase().ends_with("vaporust.exe")).unwrap_or(false))
        .ok_or(tr!("в архиве нет vaporust.exe", "the archive has no vaporust.exe"))?;
    let mut data = Vec::new();
    z.by_index(idx)
        .map_err(|e| trf!("не удалось распаковать архив: {e}", "could not unpack the archive: {e}"))?
        .read_to_end(&mut data)
        .map_err(|e| trf!("не удалось распаковать архив: {e}", "could not unpack the archive: {e}"))?;
    match pe_is_64(&data) {
        Some(is64) if is64 == cfg!(target_pointer_width = "64") => {}
        Some(_) => return Err(tr!("в архиве программа другой разрядности (x64/x86), обновление не установлено", "the archive has a program of a different bitness (x64/x86), the update was not installed").into()),
        None => return Err(tr!("в архиве не программа Windows, обновление не установлено", "the archive has no Windows program, the update was not installed").into()),
    }
    if !is_vaporust(&data) {
        return Err(tr!("программа в архиве - не Vaporust, обновление не установлено", "the program in the archive is not Vaporust, the update was not installed").into());
    }
    let tmp =std::env::temp_dir().join(format!("vaporust-update-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    let new_exe = tmp.join("vaporust.exe");
    std::fs::write(&new_exe, data).map_err(|e| e.to_string())?;
    let old = exe.with_extension("exe.old");
    let _ = std::fs::remove_file(&old);
    std::fs::rename(exe, &old).map_err(|e| trf!("не удалось заменить программу (папка доступна для записи?): {e}", "could not replace the program (is the folder writable?): {e}"))?;
    if let Err(e) = std::fs::copy(&new_exe, exe) {
        // Restore as it was.
        let _ = std::fs::rename(&old, exe);
        return Err(trf!("не удалось записать новую версию: {e}", "could not write the new version: {e}"));
    }
    let _ = std::fs::remove_dir_all(&tmp);
    Ok(())
}

/// At startup: remove the old version file left after an update.
pub fn cleanup_old() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(exe.with_extension("exe.old"));
    }
}

impl crate::LauncherApp {
    /// Bar above the main window: "A new version is available", install progress, restart.
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
        // A slim strip in the program's style: a colored edge on the left, the text, one main
        // (blue) button and quiet secondary ones on the right.
        let accent = match &state {
            State::Ready => crate::theme::PLAY,
            State::Failed(_) => egui::Color32::from_rgb(200, 90, 90),
            _ => crate::theme::ACCENT,
        };
        let frame = egui::Frame::none().fill(egui::Color32::from_rgb(24, 40, 58)).inner_margin(egui::Margin { left: 14.0, right: 10.0, top: 6.0, bottom: 6.0 });
        let primary = |ui: &mut egui::Ui, text: &str| ui.add(egui::Button::new(egui::RichText::new(text).color(egui::Color32::WHITE)).fill(crate::theme::ACCENT).rounding(3.0));
        let quiet = |ui: &mut egui::Ui, text: &str| ui.add(egui::Button::new(egui::RichText::new(text).color(crate::theme::WEAK)).frame(false)).on_hover_cursor(egui::CursorIcon::PointingHand);
        let panel = egui::TopBottomPanel::top("update_banner").frame(frame).show_separator_line(false).show(ctx, |ui| {
            ui.horizontal(|ui| match &state {
                State::Available(r) => {
                    ui.label(egui::RichText::new(trf!("Вышла новая версия Vaporust {}", "Vaporust {} is available", r.version)).strong().color(egui::Color32::WHITE));
                    if r.asset_size > 0 {
                        ui.label(egui::RichText::new(crate::net::fmt_bytes(r.asset_size)).color(crate::theme::WEAK));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if quiet(ui, tr!("Позже", "Later")).clicked() {
                            self.upd_hidden = true;
                        }
                        let notes: String = r.notes.lines().take(25).collect::<Vec<_>>().join("\n");
                        if !r.page.is_empty() && quiet(ui, tr!("Что нового", "What's new")).on_hover_text(notes).clicked() {
                            ui.ctx().open_url(egui::OpenUrl::new_tab(&r.page));
                        }
                        if r.asset_url.is_some() {
                            if primary(ui, tr!("Обновить", "Update")).on_hover_text(tr!("Скачать и установить, затем перезапустить Vaporust", "Download and install, then restart Vaporust")).clicked() {
                                // Started by the user: its result (an error too) is always shown.
                                self.upd_manual = true;
                                install(r.clone(), &self.upd, ui.ctx());
                            }
                        } else {
                            ui.label(egui::RichText::new(tr!("архива для этой Windows в выпуске нет - скачайте со страницы", "the release has no archive for this Windows - download it from the page")).color(crate::theme::WEAK));
                        }
                    });
                }
                State::Downloading(t) => {
                    crate::spin::spinner(ui);
                    ui.label(egui::RichText::new(t).color(egui::Color32::WHITE));
                }
                State::Ready => {
                    ui.label(egui::RichText::new(tr!("Новая версия установлена", "The new version is installed")).strong().color(egui::Color32::WHITE));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if primary(ui, tr!("Перезапустить Vaporust", "Restart Vaporust")).clicked() && crate::portable::restart().is_ok() {
                            crate::tray::request_exit();
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });
                }
                State::UpToDate => {
                    ui.label(egui::RichText::new(trf!("У вас последняя версия Vaporust ({CURRENT})", "You have the latest Vaporust version ({CURRENT})")).color(egui::Color32::WHITE));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if quiet(ui, "OK").clicked() {
                            self.upd_hidden = true;
                        }
                    });
                }
                State::Failed(e) => {
                    ui.label(egui::RichText::new(e).color(egui::Color32::from_rgb(240, 150, 150)));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if quiet(ui, "OK").clicked() {
                            self.upd_hidden = true;
                        }
                    });
                }
                _ => {}
            });
        });
        let r = panel.response.rect;
        ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("update_banner_edge")))
            .rect_filled(egui::Rect::from_min_max(r.min, egui::pos2(r.min.x + 3.0, r.max.y)), 0.0, accent);
    }

    /// Check for updates by a button (show both "no updates" and an error).
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
          {"tag_name":"v0.2.1-alpha","draft":false,"body":"new","html_url":"u21","assets":[{"name":"Vaporust-0.2.1-alpha-windows-x64.zip","browser_download_url":"d21","size":20},{"name":"Vaporust-0.2.1-alpha-windows-x86.zip","browser_download_url":"d21x86","size":19},{"name":"Vaporust-0.2.1-alpha-windows-x64.zip.sha256","browser_download_url":"s21","size":66},{"name":"Vaporust-0.2.1-alpha-windows-x86.zip.sha256","browser_download_url":"s21x86","size":66}]}
        ]"#;
        let r = parse_latest(json, "0.2.0-alpha").unwrap().unwrap();
        assert_eq!(r.version, "0.2.1-alpha");
        let x64 = cfg!(target_pointer_width = "64");
        assert_eq!(r.asset_url.as_deref(), Some(if x64 { "d21" } else { "d21x86" }));
        assert_eq!(r.sha_url.as_deref(), Some(if x64 { "s21" } else { "s21x86" }));
        assert!(parse_latest(json, "0.2.1-alpha").unwrap().is_none());
        // Old releases without .sha256 - no sum (the install refuses).
        let r = parse_latest(json, "0.0.1").unwrap().unwrap();
        assert_eq!(r.version, "0.2.1-alpha");
    }

    #[test]
    fn checksum() {
        // SHA-256 of the string "abc".
        let sum = "BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD";
        let parsed = parse_sha(format!("{sum}\r\n").as_bytes()).unwrap();
        assert!(verify_sha(b"abc", &parsed).is_ok());
        assert!(verify_sha(b"abd", &parsed).is_err());
        assert!(parse_sha(format!("\u{feff}{}  file.zip", sum.to_lowercase()).as_bytes()).is_ok());
        assert!(parse_sha(b"not a sum").is_err());
        assert!(parse_sha(b"").is_err());
    }

    #[test]
    fn exe_arch() {
        let me = std::fs::read(std::env::current_exe().unwrap()).unwrap();
        assert_eq!(pe_is_64(&me), Some(cfg!(target_pointer_width = "64")));
        assert_eq!(pe_is_64(b"old exe"), None);
        // Only the first bytes right ("MZ", "PE", x64): not a program.
        let mut fake = vec![0u8; 70];
        fake[..2].copy_from_slice(b"MZ");
        fake[0x3c] = 0x40;
        fake[0x40..0x44].copy_from_slice(b"PE\0\0");
        fake[0x44..0x46].copy_from_slice(&0x8664u16.to_le_bytes());
        assert_eq!(pe_is_64(&fake), None);
        assert!(is_vaporust(&me), "the test program carries the Vaporust version resource");
    }

    /// Live test: a request to GitHub (read only).
    #[test]
    #[ignore]
    fn github_live() {
        let b = get(API).unwrap();
        println!("{:?}", parse_latest(&b, "0.0.1").unwrap());
        println!("{:?}", parse_latest(&b, CURRENT).unwrap());
    }

    /// Live test of exe replacement on a copy in the VAPORUST_DUMP folder: download alpha.3 and swap.
    #[test]
    #[ignore]
    fn replace_live() {
        let dir = std::path::PathBuf::from(std::env::var("VAPORUST_DUMP").unwrap());
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
