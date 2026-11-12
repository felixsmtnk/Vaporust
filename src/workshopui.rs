//! Мастерская Steam в интерфейсе: поиск и подписка (на странице игры в библиотеке),
//! включение/выключение подписанных модов (во вкладке «Мастерская» свойств игры).
//! Сами моды Steam скачивает и обновляет при запуске игры.

use crate::net::{self, Cmd};
use crate::social::{SocialCmd, WorkshopState};
use aurelia::core::models::WorkshopItem;
use eframe::egui;

/// Состояние поиска в Мастерской для открытой игры.
#[derive(Default)]
pub struct WsUi {
    pub appid: u32,
    pub text: String,
    /// Тип запроса Steam: 0 популярные, 3 в тренде, 1 новые, 21 обновлённые, 9 по подпискам.
    pub sort: u32,
    pub loaded: bool,
}

impl WsUi {
    /// Для новой игры — сбросить поиск и один раз запросить подписки и популярное.
    pub fn ensure(&mut self, appid: u32, cmds: &mut Vec<Cmd>) {
        if self.appid != appid {
            *self = WsUi { appid, ..Default::default() };
        }
        if !self.loaded {
            self.loaded = true;
            cmds.push(Cmd::Social(SocialCmd::WorkshopSubscribed(appid)));
            cmds.push(Cmd::Social(SocialCmd::WorkshopQuery { appid, text: String::new(), sort: self.sort }));
        }
    }
}

fn fmt_date(unix: u64) -> String {
    use chrono::{Local, TimeZone};
    Local
        .timestamp_opt(unix as i64, 0)
        .single()
        .map(|t| t.format("%d.%m.%Y").to_string())
        .unwrap_or_default()
}

fn item_info(ui: &mut egui::Ui, it: &WorkshopItem) {
    let url = format!("https://steamcommunity.com/sharedfiles/filedetails/?id={}", it.id);
    ui.hyperlink_to(&it.title, url).on_hover_text("Открыть страницу мода в браузере");
    if it.file_size > 0 {
        ui.weak(net::fmt_bytes(it.file_size));
    }
    if it.time_updated > 0 {
        ui.weak(format!("обновлён {}", fmt_date(it.time_updated as u64)));
    }
}

/// Поиск в Мастерской с кнопками «Подписаться» / «Отписаться» (страница игры).
pub fn browse(ui: &mut egui::Ui, w: &mut WsUi, ws: &WorkshopState, cmds: &mut Vec<Cmd>) {
    let appid = w.appid;
    let subs = ws.sub_ids.get(&appid).cloned().unwrap_or_default();
    ui.horizontal_wrapped(|ui| {
        ui.strong("Мастерская");
        ui.weak(format!("подписок: {} (включить/выключить — в «Свойствах»)", subs.len()));
    });
    if let Some(e) = &ws.error {
        ui.colored_label(egui::Color32::from_rgb(230, 110, 110), e);
    }
    ui.horizontal_wrapped(|ui| {
        let edit = ui.add(egui::TextEdit::singleline(&mut w.text).hint_text("Поиск модов").desired_width(220.0));
        let sorts = [(0u32, "популярные"), (3, "в тренде"), (1, "новые"), (21, "обновлённые"), (9, "по подпискам")];
        let before = w.sort;
        egui::ComboBox::from_id_salt("ws_sort")
            .selected_text(sorts.iter().find(|s| s.0 == w.sort).map_or("популярные", |s| s.1))
            .show_ui(ui, |ui| {
                for (v, l) in sorts {
                    ui.selectable_value(&mut w.sort, v, l);
                }
            });
        let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if enter || ui.button("Искать").clicked() || before != w.sort {
            cmds.push(Cmd::Social(SocialCmd::WorkshopQuery { appid, text: w.text.clone(), sort: w.sort }));
        }
        if ws.searching.contains(&appid) {
            ui.spinner();
        }
    });
    match ws.results.get(&appid) {
        None => {}
        Some(Err(_)) => {
            ui.weak("У этой игры нет Мастерской.");
        }
        Some(Ok(items)) if items.is_empty() => {
            ui.weak("Ничего не найдено.");
        }
        Some(Ok(items)) => {
            if let Some(t) = ws.total.get(&appid) {
                ui.weak(format!("Всего модов: {t}"));
            }
            for it in items {
                ui.horizontal_wrapped(|ui| {
                    item_info(ui, it);
                    if subs.contains(&it.id) {
                        if ui.small_button("Отписаться").clicked() {
                            cmds.push(Cmd::Social(SocialCmd::WorkshopSubscribe { appid, id: it.id, on: false }));
                        }
                    } else if ui.small_button("Подписаться").clicked() {
                        cmds.push(Cmd::Social(SocialCmd::WorkshopSubscribe { appid, id: it.id, on: true }));
                    }
                });
            }
        }
    }
}

impl crate::LauncherApp {
    /// Открыть Мастерскую игры: в главном окне или (detached) в отдельном.
    pub(crate) fn open_workshop(&mut self, appid: u32, name: String, detached: bool) {
        self.ws_open = Some((appid, name));
        self.ws_detached = detached;
        if !detached {
            self.view = crate::View::Workshop;
        }
    }

    /// Поиск модов Мастерской (и в главном окне, и в отдельном).
    pub(crate) fn workshop_contents(&mut self, ui: &mut egui::Ui, net: &net::NetState) {
        let Some((appid, name)) = self.ws_open.clone() else { return };
        let mut cmds = Vec::new();
        self.ws_ui.ensure(appid, &mut cmds);
        ui.heading(format!("Мастерская: {name}"));
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            let ws = net.workshop.lock().unwrap();
            browse(ui, &mut self.ws_ui, &ws, &mut cmds);
        });
        for c in cmds {
            let _ = self.tx.send(c);
        }
    }

    /// Мастерская в отдельном окне (открыта с Ctrl).
    pub(crate) fn workshop_window(&mut self, ctx: &egui::Context, net: &net::NetState) {
        if !self.ws_detached {
            return;
        }
        let Some((_, name)) = self.ws_open.clone() else { return };
        let closed = ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("vaporust_workshop"),
            egui::ViewportBuilder::default()
                .with_title(format!("Мастерская: {name} — Vaporust"))
                .with_inner_size([720.0, 560.0])
                .with_min_inner_size([480.0, 320.0])
                .with_icon(crate::icons::icon(crate::icons::Kind::Launcher)),
            |ctx, _| {
                egui::CentralPanel::default().show(ctx, |ui| self.workshop_contents(ui, net));
                ctx.input(|i| i.viewport().close_requested())
            },
        );
        if closed {
            self.ws_open = None;
            self.ws_detached = false;
        }
    }
}

/// Подписанные моды с галочками «включён» (вкладка свойств). Снятая галочка = отписка;
/// мод остаётся в списке, пока окно открыто, чтобы его можно было включить обратно.
pub fn subscriptions(ui: &mut egui::Ui, appid: u32, ws: &WorkshopState, cmds: &mut Vec<Cmd>) {
    let subs = ws.sub_ids.get(&appid).cloned().unwrap_or_default();
    ui.weak("Включённые моды Steam скачает и обновит при следующем запуске игры. Найти новые — на странице игры в библиотеке.");
    if let Some(e) = &ws.error {
        ui.colored_label(egui::Color32::from_rgb(230, 110, 110), e);
    }
    match ws.subscribed.get(&appid) {
        None => {
            ui.spinner();
        }
        Some(Err(_)) => {
            ui.weak("У этой игры нет Мастерской.");
        }
        Some(Ok(list)) if list.is_empty() => {
            ui.weak("Подписанных модов нет.");
        }
        Some(Ok(list)) => {
            ui.label(format!("Включено модов: {} из {}", list.iter().filter(|i| subs.contains(&i.id)).count(), list.len()));
            ui.separator();
            for it in list {
                ui.horizontal_wrapped(|ui| {
                    let mut on = subs.contains(&it.id);
                    if ui.checkbox(&mut on, "").changed() {
                        cmds.push(Cmd::Social(SocialCmd::WorkshopSubscribe { appid, id: it.id, on }));
                    }
                    item_info(ui, it);
                });
            }
        }
    }
}
