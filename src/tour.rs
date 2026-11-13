//! Обход разделов со снимками экрана - для разработки и проверки интерфейса.
//! Запуск: VAPORUST_TOUR=<папка> vaporust.exe. Vaporust сам открывает разделы главного окна
//! по очереди и сохраняет снимок каждого (<папка>\NN_имя.png), затем пишет «done.txt».
//! Ничего не покупает, не отправляет и не меняет: только переключает вкладки.

use crate::net::NetState;
use crate::{LauncherApp, View};
use eframe::egui;
use std::path::PathBuf;

pub struct Tour {
    dir: PathBuf,
    step: usize,
    next_at: f64,
    /// Снимок какого шага ждем (имя файла).
    waiting: Option<String>,
    /// Шаг применен, ждем, пока раздел догрузится.
    shown: Option<String>,
    started: Option<f64>,
}

impl Tour {
    pub fn from_env() -> Option<Self> {
        let dir = PathBuf::from(std::env::var_os("VAPORUST_TOUR")?);
        let _ = std::fs::create_dir_all(&dir);
        Some(Tour { dir, step: 0, next_at: 0.0, waiting: None, shown: None, started: None })
    }
}

/// Шаги обхода: имя и сколько секунд ждать загрузки раздела.
const STEPS: &[(&str, f64)] = &[
    ("library_grid", 4.0),
    ("library_list", 3.0),
    ("game_page", 6.0),
    ("downloads", 3.0),
    ("profile", 8.0),
    ("friends", 5.0),
    ("store_search", 8.0),
    ("store_deals", 8.0),
    ("store_charts", 8.0),
    ("store_discovery", 8.0),
    ("store_wishlist", 8.0),
    ("store_page", 9.0),
    ("store_page_scrolled", 3.0),
    ("inventory", 8.0),
    ("news", 10.0),
    ("activity", 8.0),
    ("groups", 5.0),
    ("screenshots", 6.0),
    ("storage", 6.0),
    ("badges", 8.0),
    ("points", 8.0),
    ("family", 8.0),
    ("stats", 4.0),
    ("community", 8.0),
    ("workshop", 8.0),
    ("workshop_item", 6.0),
    ("points_stickers", 8.0),
    ("settings_0", 4.0),
    ("settings_1", 2.0),
    ("settings_2", 2.0),
    ("settings_3", 2.0),
    ("settings_4", 2.0),
    ("settings_5", 2.0),
    ("settings_6", 2.0),
    ("settings_7", 2.0),
    ("settings_8", 2.0),
];

impl LauncherApp {
    pub(crate) fn tour_tick(&mut self, ctx: &egui::Context, net: &NetState) {
        let Some(t) = self.tour.as_mut() else { return };
        let now = ctx.input(|i| i.time);
        // Готовые снимки.
        let shots: Vec<std::sync::Arc<egui::ColorImage>> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| if let egui::Event::Screenshot { image, .. } = e { Some(image.clone()) } else { None })
                .collect()
        });
        for img in shots {
            if let Some(name) = t.waiting.take() {
                let [w, h] = img.size;
                let raw: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
                if let Some(buf) = image::RgbaImage::from_raw(w as u32, h as u32, raw) {
                    let _ = buf.save(t.dir.join(format!("{name}.png")));
                }
                // Память на каждом шаге (для замеров): время, шаг, рабочий набор, частная, кэш картинок.
                let (ws, private) = crate::stats::memory();
                let line = format!(
                    "{} {name} taskmgr={:.1} private={:.1} images={:.1}\n",
                    chrono::Local::now().format("%H:%M:%S"),
                    ws as f64 / 1048576.0,
                    private as f64 / 1048576.0,
                    crate::imgload::cache_bytes(ctx) as f64 / 1048576.0
                );
                use std::io::Write;
                if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(t.dir.join("mem.txt")) {
                    let _ = f.write_all(line.as_bytes());
                }
            }
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(300));
        if t.waiting.is_some() || now < t.next_at {
            return;
        }
        // Ждем входа и библиотеки (до 40 с).
        let started = *t.started.get_or_insert(now);
        if t.step == 0 && t.shown.is_none() && (net.owned.is_empty() || self.games.is_empty()) && now - started < 40.0 {
            return;
        }
        if let Some(name) = t.shown.take() {
            t.waiting = Some(name);
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot);
            return;
        }
        let Some(&(name, wait)) = STEPS.get(t.step) else {
            let _ = std::fs::write(t.dir.join("done.txt"), "ok");
            self.tour = None;
            return;
        };
        let idx = t.step;
        t.step += 1;
        t.shown = Some(format!("{:02}_{name}", idx + 1));
        t.next_at = now + wait;
        self.tour_apply(name, net);
    }

    fn tour_apply(&mut self, name: &str, net: &NetState) {
        use crate::storewin::Tab;
        let game = self
            .games
            .iter()
            .find(|g| g.ready && g.appid == 550)
            .or_else(|| self.games.iter().find(|g| g.ready))
            .map(|g| (g.appid, g.name.clone()));
        let store_tab = |app: &mut LauncherApp, tab: Tab| {
            app.open_store(None, false);
            if let Some(w) = app.store_win.as_mut() {

                w.set_tab(tab);
            }
        };
        match name {
            "library_grid" | "library_list" => {
                self.close_extras(View::Library);
                self.view = View::Library;
                self.lib_page = None;
                self.settings.view =
                    if name == "library_grid" { crate::settings::View::Grid } else { crate::settings::View::List };
            }
            "game_page" => {
                self.settings.view = crate::settings::View::Grid;
                self.view = View::Library;
                self.lib_page = game.as_ref().map(|g| g.0);
            }
            "workshop_item" => {
                let id = net.workshop.lock().unwrap().results.get(&self.ws_ui.appid).and_then(|r| r.as_ref().ok()).and_then(|v| v.first()).map(|i| i.id);
                if let Some(id) = id {
                    self.ws_ui.item = Some(id);
                    let _ = self.tx.send(crate::net::Cmd::Social(crate::social::SocialCmd::WorkshopDetail(id)));
                }
            }
            "points_stickers" => {
                self.open_points();
                self.points_ui.class = 11;
            }
            "friends" => self.open_friends_page(1),
            "downloads" => {
                self.close_extras(View::Downloads);
                self.view = View::Downloads;
            }
            "profile" => {
                if let Some(me) = net.my_id {
                    self.open_profile(me, net);
                }
            }
            "store_search" => {
                store_tab(self, Tab::Search);
                if let Some(w) = self.store_win.as_mut() {
                    w.tour_search("portal");
                }
                let _ = self.tx.send(crate::net::Cmd::Store(crate::store::StoreCmd::Search("portal".into())));
            }
            "store_deals" => store_tab(self, Tab::Deals),
            "store_charts" => store_tab(self, Tab::Charts),
            "store_discovery" => store_tab(self, Tab::Discovery),
            "store_wishlist" => store_tab(self, Tab::Wishlist),
            "store_page" => self.open_store(Some(620), false),
            "store_page_scrolled" => crate::storewin::TOUR_SCROLL.store(1400, std::sync::atomic::Ordering::Relaxed),
            "inventory" => self.open_inventory(false),
            "news" => {
                self.close_extras(View::News);
                self.view = View::News;
            }
            "activity" => self.open_activity(),
            "groups" => self.open_groups(),
            "screenshots" => self.open_shots(None),
            "storage" => self.open_storage(),
            "badges" => self.open_badges(),
            "points" => self.open_points(),
            "family" => self.open_family(),
            "stats" => self.open_stats(),
            "community" => {
                if let Some((id, n)) = game.clone() {
                    self.open_community(id, n);
                }
            }
            "workshop" => {
                if let Some((id, n)) = game.clone() {
                    self.open_workshop(id, n, false);
                }
            }
            s if s.starts_with("settings_") => {
                self.close_extras(View::Settings);
                self.view = View::Settings;
                self.settings_section = s[9..].parse().unwrap_or(0);
            }
            _ => {}
        }
        let _ = net;
    }
}
