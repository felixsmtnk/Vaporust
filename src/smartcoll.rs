//! Smart collections, like the dynamic collections of Steam: a name and rules (installed, played,
//! genre, owner); the games are picked by the rules every time. Kept in Vaporust settings only:
//! the Steam cloud collections are not touched, so nothing can break there.

use crate::{theme, LauncherApp, Row};
use eframe::egui;
use serde::{Deserialize, Serialize};

/// The prefix of a smart collection in the list choice ("__smart:<index>").
pub const PREFIX: &str = "__smart:";

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Smart {
    pub name: String,
    /// Some(true) installed only, Some(false) not installed only.
    pub installed: Option<bool>,
    /// Some(true) launched at least once, Some(false) never launched.
    pub played: Option<bool>,
    /// A store tag (genre) the game must have.
    pub genre: Option<String>,
    /// Some(true) family games only, Some(false) own games only.
    pub family: Option<bool>,
    /// Only games with an update available.
    pub update: bool,
}

impl Smart {
    /// Does the game fit the rules (`tags` - its store tags, `update` - an update is available).
    pub fn fits(&self, r: &Row, tags: Option<&[String]>, update: bool) -> bool {
        if self.installed.is_some_and(|i| i != r.installed) {
            return false;
        }
        if let Some(p) = self.played {
            let played = r.playtime_min.unwrap_or(0) > 0 || r.last_played > 0;
            if p != played {
                return false;
            }
        }
        if self.family.is_some_and(|f| f != r.family) {
            return false;
        }
        if self.update && !update {
            return false;
        }
        if let Some(g) = &self.genre {
            if !tags.is_some_and(|t| t.iter().any(|x| x == g)) {
                return false;
            }
        }
        true
    }
}

/// The editor of a new smart collection.
#[derive(Default)]
pub struct SmartEdit {
    pub open: bool,
    pub draft: Smart,
}

fn tri(ui: &mut egui::Ui, value: &mut Option<bool>, any: &str, yes: &str, no: &str) {
    ui.radio_value(value, None, any);
    ui.radio_value(value, Some(true), yes);
    ui.radio_value(value, Some(false), no);
}

impl LauncherApp {
    /// The window "New smart collection" (rules, then "Create").
    pub(crate) fn smart_dialog(&mut self, ctx: &egui::Context, genres: &[String]) {
        if !self.smart_edit.open {
            return;
        }
        let mut open = !crate::theme::esc(ctx); // Esc - cancel
        let mut create = false;
        egui::Window::new(tr!("Новая умная коллекция", "New smart collection"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(420.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                let d = &mut self.smart_edit.draft;
                ui.label(egui::RichText::new(tr!("Игры подбираются сами по правилам, как динамические коллекции Steam.", "Games are picked by the rules, like Steam dynamic collections.")).color(theme::WEAK));
                ui.add_space(6.0);
                egui::Grid::new("smart_rules").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                    ui.label(tr!("Название", "Name"));
                    ui.add(egui::TextEdit::singleline(&mut d.name).desired_width(240.0));
                    ui.end_row();
                    ui.label(tr!("Установлена", "Installed"));
                    ui.horizontal(|ui| tri(ui, &mut d.installed, tr!("неважно", "any"), tr!("да", "yes"), tr!("нет", "no")));
                    ui.end_row();
                    ui.label(tr!("Запускалась", "Played"));
                    ui.horizontal(|ui| tri(ui, &mut d.played, tr!("неважно", "any"), tr!("да", "yes"), tr!("ни разу", "never")));
                    ui.end_row();
                    ui.label(tr!("Чья игра", "Owner"));
                    ui.horizontal(|ui| tri(ui, &mut d.family, tr!("любая", "any"), tr!("семейная", "family"), tr!("своя", "own")));
                    ui.end_row();
                    ui.label(tr!("Жанр", "Genre"));
                    egui::ComboBox::from_id_salt("smart_genre")
                        .selected_text(d.genre.clone().unwrap_or_else(|| tr!("любой", "any").into()))
                        .height(300.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut d.genre, None, tr!("любой", "any"));
                            for g in genres {
                                ui.selectable_value(&mut d.genre, Some(g.clone()), g);
                            }
                        });
                    ui.end_row();
                    ui.label(tr!("Обновление", "Update"));
                    theme::switch(ui, true, &mut d.update, tr!("только с доступным обновлением", "only with an update available"));
                    ui.end_row();
                });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let ok = !d.name.trim().is_empty();
                    if ui.add_enabled(ok, egui::Button::new(egui::RichText::new(tr!("Создать", "Create")).color(egui::Color32::WHITE)).fill(theme::ACCENT)).clicked() {
                        create = true;
                    }
                    if ui.button(tr!("Отмена", "Cancel")).clicked() {
                        self.smart_edit.open = false;
                    }
                });
            });
        if create {
            let mut s = std::mem::take(&mut self.smart_edit.draft);
            s.name = s.name.trim().to_string();
            self.settings.smart.push(s);
            self.collection = Some(format!("{PREFIX}{}", self.settings.smart.len() - 1));
            self.collections_page = false;
            self.smart_edit.open = false;
        }
        if !open {
            self.smart_edit.open = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(installed: bool, minutes: u32, family: bool) -> Row {
        Row { installed, playtime_min: Some(minutes), family, ..Default::default() }
    }

    #[test]
    fn rules() {
        let never_installed = Smart { installed: Some(true), played: Some(false), ..Default::default() };
        assert!(never_installed.fits(&row(true, 0, false), None, false));
        assert!(!never_installed.fits(&row(true, 5, false), None, false));
        assert!(!never_installed.fits(&row(false, 0, false), None, false));
        let rpg = Smart { genre: Some("RPG".into()), family: Some(false), ..Default::default() };
        let tags = vec!["Action".to_string(), "RPG".to_string()];
        assert!(rpg.fits(&row(false, 0, false), Some(&tags), false));
        assert!(!rpg.fits(&row(false, 0, true), Some(&tags), false));
        assert!(!rpg.fits(&row(false, 0, false), None, false));
        let upd = Smart { update: true, ..Default::default() };
        assert!(upd.fits(&row(true, 1, false), None, true));
        assert!(!upd.fits(&row(true, 1, false), None, false));
    }
}

impl LauncherApp {
    /// Genres (store tags) found among the library games, the most common first.
    pub(crate) fn known_genres(&self, net: &crate::net::NetState) -> Vec<String> {
        let st = net.store.lock().unwrap();
        let mut count: std::collections::HashMap<String, usize> = Default::default();
        for id in self.row_map(net).keys() {
            if let Some(a) = st.apps.get(id) {
                for t in a.tags.iter().take(8) {
                    if let Some(n) = &t.name {
                        *count.entry(n.clone()).or_default() += 1;
                    }
                }
            }
        }
        let mut v: Vec<(String, usize)> = count.into_iter().collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        v.into_iter().take(60).map(|(g, _)| g).collect()
    }
}

impl LauncherApp {
    /// The genre rule of the chosen smart collection, if any.
    pub(crate) fn smart_genre(&self) -> Option<&str> {
        let i: usize = self.collection.as_deref()?.strip_prefix(PREFIX)?.parse().ok()?;
        self.settings.smart.get(i)?.genre.as_deref()
    }
}
