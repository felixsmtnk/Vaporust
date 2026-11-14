//! Саундтреки из библиотеки (как «Музыка» в Steam): папки steamapps\music\* и папки с «Soundtrack»
//! / «OST» внутри установленных игр. MP3, WAV и WMA играют прямо в программе (Windows MCI, без
//! лишних библиотек); FLAC, OGG и прочее открываются в проигрывателе Windows.

use crate::LauncherApp;
use eframe::egui;
use std::path::{Path, PathBuf};

const AUDIO: [&str; 7] = ["mp3", "wav", "wma", "flac", "ogg", "m4a", "aac"];
const PLAYABLE: [&str; 3] = ["mp3", "wav", "wma"];

#[derive(Clone, Debug)]
pub struct Album {
    pub name: String,
    pub dir: PathBuf,
    pub tracks: Vec<PathBuf>,
}

#[derive(Default)]
pub struct Music {
    pub albums: Option<Vec<Album>>,
    pub album: usize,
    /// Играющий трек: (альбом, номер).
    pub playing: Option<(usize, usize)>,
    pub paused: bool,
    pub volume: f32,
}

fn ext(p: &Path) -> String {
    p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

fn collect(dir: &Path, depth: u32, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            if depth > 0 {
                collect(&p, depth - 1, out);
            }
        } else if AUDIO.contains(&ext(&p).as_str()) {
            out.push(p);
        }
    }
}

/// Найти саундтреки во всех библиотеках Steam.
pub fn scan(steam_dir: &Path) -> Vec<Album> {
    let mut albums = Vec::new();
    for lib in crate::local::library_dirs(steam_dir) {
        let apps = lib.join("steamapps");
        // Саундтреки-DLC Steam ставит в steamapps\music\<название>.
        if let Ok(rd) = std::fs::read_dir(apps.join("music")) {
            for e in rd.flatten() {
                let mut tracks = Vec::new();
                collect(&e.path(), 3, &mut tracks);
                if !tracks.is_empty() {
                    albums.push(Album { name: e.file_name().to_string_lossy().into_owned(), dir: e.path(), tracks });
                }
            }
        }
        // Папки «Soundtrack» / «OST» внутри игр (на два уровня вглубь).
        if let Ok(rd) = std::fs::read_dir(apps.join("common")) {
            for game in rd.flatten() {
                let mut stack = vec![(game.path(), 0u32)];
                while let Some((d, depth)) = stack.pop() {
                    let Ok(sub) = std::fs::read_dir(&d) else { continue };
                    for s in sub.flatten() {
                        let p = s.path();
                        if !p.is_dir() {
                            continue;
                        }
                        let n = s.file_name().to_string_lossy().to_lowercase();
                        if n.contains("soundtrack") || n == "ost" || n.ends_with(" ost") || n.starts_with("ost ") {
                            let mut tracks = Vec::new();
                            collect(&p, 2, &mut tracks);
                            if !tracks.is_empty() {
                                albums.push(Album { name: format!("{} - {}", game.file_name().to_string_lossy(), s.file_name().to_string_lossy()), dir: p.clone(), tracks });
                            }
                        } else if depth < 1 {
                            stack.push((p, depth + 1));
                        }
                    }
                }
            }
        }
    }
    albums.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    albums
}

#[cfg(windows)]
fn mci(cmd: &str) -> Result<String, String> {
    use windows_sys::Win32::Media::Multimedia::mciSendStringW;
    let w: Vec<u16> = cmd.encode_utf16().chain(std::iter::once(0)).collect();
    let mut buf = [0u16; 256];
    let r = unsafe { mciSendStringW(w.as_ptr(), buf.as_mut_ptr(), buf.len() as u32, std::ptr::null_mut()) };
    if r != 0 {
        return Err(format!("ошибка MCI {r}"));
    }
    let len = buf.iter().position(|c| *c == 0).unwrap_or(0);
    Ok(String::from_utf16_lossy(&buf[..len]))
}

#[cfg(not(windows))]
fn mci(_cmd: &str) -> Result<String, String> {
    Err("только Windows".into())
}

fn stop_all() {
    let _ = mci("close vaporust_music");
}

fn play_file(p: &Path, volume: f32) -> Result<(), String> {
    stop_all();
    let kind = if ext(p) == "wav" { "waveaudio" } else { "mpegvideo" };
    mci(&format!("open \"{}\" type {kind} alias vaporust_music", p.display()))?;
    if kind == "mpegvideo" {
        let _ = mci(&format!("setaudio vaporust_music volume to {}", (volume * 1000.0) as u32));
    }
    mci("play vaporust_music").map(|_| ())
}

/// Позиция и длина текущего трека, мс.
fn position() -> Option<(u64, u64)> {
    let _ = mci("set vaporust_music time format milliseconds");
    let pos = mci("status vaporust_music position").ok()?.trim().parse().ok()?;
    let len = mci("status vaporust_music length").ok()?.trim().parse().ok()?;
    Some((pos, len))
}

fn mmss(ms: u64) -> String {
    format!("{}:{:02}", ms / 60000, ms / 1000 % 60)
}

impl LauncherApp {
    pub(crate) fn open_music(&mut self) {
        self.close_extras(crate::View::Music);
        self.view = crate::View::Music;
        if self.music.volume == 0.0 {
            self.music.volume = 0.8;
        }
        if self.music.albums.is_none() {
            self.music.albums = Some(self.steam.as_ref().map(|s| scan(&s.dir)).unwrap_or_default());
        }
    }

    pub(crate) fn music_contents(&mut self, ui: &mut egui::Ui) {
        let m = &mut self.music;
        let albums = m.albums.clone().unwrap_or_default();
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Саундтреки").size(20.0).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Найти заново").clicked() {
                    m.albums = None;
                }
            });
        });
        ui.separator();
        if albums.is_empty() {
            ui.weak("Саундтреков не найдено. Steam ставит купленные саундтреки в папку steamapps\\music библиотеки; \
                     у некоторых игр музыка лежит в папке «Soundtrack» внутри игры.");
            if m.albums.is_none() {
                m.albums = Some(self.steam.as_ref().map(|s| scan(&s.dir)).unwrap_or_default());
            }
            return;
        }
        // Плеер сверху: трек, позиция, пауза, стоп, громкость.
        if let Some((a, t)) = m.playing {
            let name = albums.get(a).and_then(|al| al.tracks.get(t)).and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let pos = position();
            egui::Frame::none().fill(crate::theme::CARD).rounding(4.0).inner_margin(egui::Margin::same(8.0)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui.button(if m.paused { "Играть" } else { "Пауза" }).clicked() {
                        let _ = mci(if m.paused { "resume vaporust_music" } else { "pause vaporust_music" });
                        m.paused = !m.paused;
                    }
                    if ui.button("Стоп").clicked() {
                        stop_all();
                        m.playing = None;
                    }
                    if ui.button("Следующий").clicked() {
                        let next = t + 1;
                        if let Some(p) = albums.get(a).and_then(|al| al.tracks.get(next)) {
                            if play_file(p, m.volume).is_ok() {
                                m.playing = Some((a, next));
                                m.paused = false;
                            }
                        }
                    }
                    ui.label(egui::RichText::new(crate::richtext::fix(ui, &name)).strong());
                    if let Some((p, l)) = pos {
                        ui.weak(format!("{} / {}", mmss(p), mmss(l)));
                        // Трек кончился - следующий.
                        if l > 0 && p >= l && !m.paused {
                            let next = t + 1;
                            match albums.get(a).and_then(|al| al.tracks.get(next)) {
                                Some(p) if PLAYABLE.contains(&ext(p).as_str()) && play_file(p, m.volume).is_ok() => m.playing = Some((a, next)),
                                _ => {
                                    stop_all();
                                    m.playing = None;
                                }
                            }
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(egui::Slider::new(&mut m.volume, 0.0..=1.0).show_value(false)).changed() {
                            let _ = mci(&format!("setaudio vaporust_music volume to {}", (m.volume * 1000.0) as u32));
                        }
                        ui.weak("Громкость");
                    });
                });
            });
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
            ui.add_space(6.0);
        }
        let album = m.album.min(albums.len() - 1);
        egui::SidePanel::left("music_albums").resizable(true).default_width(240.0).show_inside(ui, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                for (i, al) in albums.iter().enumerate() {
                    if ui.selectable_label(i == album, format!("{} ({})", al.name, al.tracks.len())).clicked() {
                        m.album = i;
                    }
                }
            });
        });
        egui::CentralPanel::default().show_inside(ui, |ui| {
            let al = &albums[album];
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(crate::richtext::fix(ui, &al.name)).size(16.0).strong());
                if ui.small_button("Папка").clicked() {
                    crate::local::open_folder(&al.dir);
                }
            });
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                for (i, p) in al.tracks.iter().enumerate() {
                    let e = ext(p);
                    let name = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                    let on = m.playing == Some((album, i));
                    ui.horizontal(|ui| {
                        let playable = PLAYABLE.contains(&e.as_str());
                        let label = if playable { "играть" } else { "открыть" };
                        if ui.small_button(label).on_hover_text(if playable { "Играть в Vaporust" } else { "Этот формат играет проигрыватель Windows" }).clicked() {
                            if playable {
                                if play_file(p, m.volume).is_ok() {
                                    m.playing = Some((album, i));
                                    m.paused = false;
                                }
                            } else {
                                let _ = std::process::Command::new("explorer.exe").arg(p).spawn();
                            }
                        }
                        let text = egui::RichText::new(format!("{:>2}. {}", i + 1, crate::richtext::fix(ui, &name)));
                        ui.label(if on { text.color(crate::theme::PLAY_TEXT).strong() } else { text });
                        ui.weak(e.to_uppercase());
                    });
                }
            });
        });
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn scan_temp_library() {
        let root = std::env::temp_dir().join(format!("vaporust_music_{}", std::process::id()));
        let album = root.join("steamapps").join("music").join("Тест OST");
        std::fs::create_dir_all(&album).unwrap();
        std::fs::write(album.join("01 Начало.mp3"), b"x").unwrap();
        std::fs::write(album.join("cover.jpg"), b"x").unwrap();
        let mut v = Vec::new();
        super::collect(&album, 1, &mut v);
        assert_eq!(v.len(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }
}
