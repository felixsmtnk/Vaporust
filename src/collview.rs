//! The library collections page, as in Steam: "YOUR COLLECTIONS" with a "Create a new collection"
//! tile and a tile per collection (name and number of games). A click shows the collection.

use crate::net::{Cmd, NetState};
use crate::{colls, theme, LauncherApp};
use eframe::egui;

impl LauncherApp {
    pub(crate) fn collections_view(&mut self, ui: &mut egui::Ui, net: &NetState) {
        let colls: Vec<colls::Coll> = net.collections.clone().unwrap_or_default();
        egui::ScrollArea::vertical().id_salt("collections_page").auto_shrink([false, false]).show(ui, |ui| {
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                ui.add_space(16.0);
                theme::caps_title(ui, tr!("Ваши коллекции", "Your collections"), None);
            });
            ui.add_space(10.0);
            let tile = egui::vec2(150.0, 150.0);
            let gap = 14.0;
            let w = ui.available_width() - 32.0;
            let per_row = ((w + gap) / (tile.x + gap)).floor().max(1.0) as usize;
            let new_key = egui::Id::new("new_collection_name");
            let mut new_name: Option<String> = ui.data(|d| d.get_temp(new_key));
            // The tiles: the first one creates a collection.
            // Creating a collection needs the Steam account (collections live in the Steam cloud).
            let mut items: Vec<Option<&colls::Coll>> = if net.account.is_some() { vec![None] } else { Vec::new() };
            items.extend(colls.iter().filter(|c| c.id != "hidden").map(Some));
            if net.account.is_none() {
                ui.horizontal(|ui| {
                    ui.add_space(16.0);
                    ui.label(egui::RichText::new(tr!("Коллекции хранятся в аккаунте Steam: войдите, чтобы их увидеть.", "Collections are stored in the Steam account: sign in to see them.")).color(theme::WEAK));
                });
            }
            for chunk in items.chunks(per_row) {
                ui.horizontal(|ui| {
                    ui.add_space(16.0);
                    ui.spacing_mut().item_spacing.x = gap;
                    for it in chunk {
                        let (rect, resp) = ui.allocate_exact_size(tile, egui::Sense::click());
                        let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
                        let t = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), 0.1);
                        let p = ui.painter();
                        match it {
                            None => {
                                p.rect_stroke(rect.shrink(1.0), 2.0, egui::Stroke::new(1.0, theme::lerp_color(theme::BUTTON_HOVER, theme::TEXT2, t)));
                                theme::icon_plus(p, egui::Rect::from_center_size(rect.center() - egui::vec2(0.0, 18.0), egui::vec2(20.0, 20.0)), theme::TEXT);
                                p.text(
                                    rect.center() + egui::vec2(0.0, 14.0),
                                    egui::Align2::CENTER_CENTER,
                                    tr!("СОЗДАТЬ КОЛЛЕКЦИЮ", "CREATE A COLLECTION"),
                                    theme::semibold(11.5),
                                    theme::TEXT,
                                );
                                if resp.clicked() {
                                    new_name = Some(String::new());
                                }
                            }
                            Some(c) => {
                                p.rect_filled(rect, 2.0, theme::lerp_color(egui::Color32::from_rgb(52, 63, 82), egui::Color32::from_rgb(64, 78, 102), t));
                                let name = if c.id == "favorite" { tr!("Избранное", "Favorites").to_string() } else { c.name.clone() };
                                let galley = p.layout(name.to_uppercase(), theme::semibold(13.0), theme::TEXT, rect.width() - 20.0);
                                let h = galley.size().y + 18.0;
                                p.galley(egui::pos2(rect.center().x - galley.size().x / 2.0, rect.center().y - h / 2.0), galley, theme::TEXT);
                                p.text(rect.center() + egui::vec2(0.0, h / 2.0 - 6.0), egui::Align2::CENTER_CENTER, format!("( {} )", c.apps.len()), egui::FontId::proportional(12.0), theme::TEXT2);
                                if resp.clicked() {
                                    self.collection = Some(c.id.clone());
                                    self.collections_page = false;
                                    self.lib_page = None;
                                }
                            }
                        }
                    }
                });
                ui.add_space(gap);
            }
            // Name of a new collection.
            if let Some(mut name) = new_name.clone() {
                let mut close = false;
                ui.horizontal(|ui| {
                    ui.add_space(16.0);
                    ui.label(tr!("Название:", "Name:"));
                    let edit = ui.add(egui::TextEdit::singleline(&mut name).desired_width(220.0));
                    edit.request_focus();
                    let ok = !name.trim().is_empty();
                    let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if (theme::accent_button(ui, tr!("Создать", "Create")).clicked() || enter) && ok {
                        let _ = self.tx.send(Cmd::Coll(colls::CollCmd::Create { name: name.trim().to_string(), appid: None }));
                        close = true;
                    } else if ui.button(tr!("Отмена", "Cancel")).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        close = true;
                    }
                });
                new_name = if close { None } else { Some(name) };
            }
            ui.data_mut(|d| match &new_name {
                Some(n) => d.insert_temp(new_key, n.clone()),
                None => d.remove::<String>(new_key),
            });
        });
    }
}
