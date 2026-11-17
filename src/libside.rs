//! The left column of the library: list choice, search, filters, the game list; the empty library screen.

use crate::*;

impl LauncherApp {
    /// The library shows no games: tell apart "Steam not found", "filters hide everything",
    /// "still loading", "no connection" and "really no games", and offer the next step.
    pub(crate) fn empty_library(&mut self, ui: &mut egui::Ui, net: &NetState) {
        let filtered = !self.filter.trim().is_empty() || self.collection.is_some() || self.genre.is_some() || self.only_installed;
        let any_games = !self.games.is_empty() || !net.owned.is_empty() || !self.settings.custom_games.is_empty();
        let (title, text) = if self.steam.is_none() {
            (
                tr!("Steam не найден", "Steam not found"),
                tr!(
                    "Vaporust берет игры из папок настоящего Steam и запускает их через него. Установите Steam и войдите в него хотя бы раз, затем нажмите «Пересканировать».",
                    "Vaporust takes games from the folders of the real Steam and launches them through it. Install Steam and sign in to it at least once, then press \"Rescan\"."
                ),
            )
        } else if filtered && any_games {
            (
                tr!("Ничего не найдено", "Nothing found"),
                tr!("Поиск, коллекция, жанр или «Только установленные» скрывают все игры.", "The search, collection, genre or \"Installed only\" hide all games."),
            )
        } else if net.account.is_some() && net.owned.is_empty() && net.busy {
            (tr!("Загружаю список игр...", "Loading the game list..."), tr!("Это занимает несколько секунд.", "This takes a few seconds."))
        } else if net.offline && !any_games {
            (
                tr!("Нет связи со Steam", "No connection to Steam"),
                tr!("Список купленных игр загрузится, когда связь вернется. Установленные игры видны и без связи.", "The list of owned games loads when the connection is back. Installed games are visible without it."),
            )
        } else if net.account.is_none() {
            (
                tr!("Установленных игр не найдено", "No installed games found"),
                tr!(
                    "Без входа видны только игры, установленные в папки Steam. Войдите в аккаунт, чтобы увидеть все купленные игры, или добавьте папку библиотеки.",
                    "Without signing in, only games installed in Steam folders are visible. Sign in to see all owned games, or add a library folder."
                ),
            )
        } else {
            (
                tr!("Игр пока нет", "No games yet"),
                tr!("На аккаунте нет игр, и в папках Steam ничего не установлено.", "The account has no games, and nothing is installed in the Steam folders."),
            )
        };
        ui.add_space(40.0);
        ui.vertical_centered(|ui| {
            ui.set_max_width(520.0);
            ui.label(egui::RichText::new(title).font(crate::theme::semibold(20.0)).color(egui::Color32::WHITE).color(theme::TEXT));
            ui.add_space(6.0);
            ui.label(egui::RichText::new(text).color(theme::WEAK));
            ui.add_space(14.0);
            // Buttons one under another: centered (a horizontal row inside vertical_centered sticks to the left).
            {
                ui.spacing_mut().item_spacing.y = 8.0;
                if filtered && any_games {
                    if ui.button(tr!("Сбросить фильтры", "Reset filters")).clicked() {
                        self.filter.clear();
                        self.collection = None;
                        self.genre = None;
                        self.only_installed = false;
                    }
                } else {
                    if ui.button(tr!("Пересканировать", "Rescan")).on_hover_text(tr!("Заново прочитать папки Steam", "Read the Steam folders again")).clicked() {
                        self.rescan();
                    }
                    if self.steam.is_some() && ui.button(tr!("Добавить папку библиотеки...", "Add a library folder...")).clicked() {
                        self.settings_section = 1; // "Library"
                        self.view = View::Settings;
                    }
                    if ui.button(tr!("Добавить стороннюю игру...", "Add a non-Steam game...")).clicked() {
                        self.custom_edit = Some(custom::Edit { to_steam: self.settings.custom_to_steam, ..Default::default() });
                    }
                }
            }
        });
    }

    /// Library left column, as in Steam: "Home", search, collection, filters and the game list.
    /// Library search and filters: in the left column or (toolbar) as a row above the table.
    /// The list choice of the library (all games, family, ready to play, Steam collections) with the
    /// menu of an own collection (rename, delete).
    pub(crate) fn collection_combo(&mut self, ui: &mut egui::Ui, net: &NetState, width: f32) {
                    // Without a login there are no Steam collections, but the built-in lists still work.
                    let no_colls: Vec<colls::Coll> = Vec::new();
                    {
                        let colls = net.collections.as_ref().unwrap_or(&no_colls);
                        let current = self
                            .collection
                            .as_ref()
                            .and_then(|id| colls.iter().find(|c| &c.id == id))
                            .map(|c| c.name.clone())
                            .or_else(|| (self.collection.as_deref() == Some(FAMILY)).then(|| tr!("Семейная библиотека", "Family library").to_string()))
                            .or_else(|| (self.collection.as_deref() == Some(UNPLAYED)).then(|| tr!("Ни разу не запускались", "Never launched").to_string()))
                            .or_else(|| (self.collection.as_deref() == Some(READY)).then(|| tr!("Готово к игре", "Ready to play").to_string()))
                            .or_else(|| (self.collection.as_deref() == Some(UPDATES)).then(|| tr!("Есть обновление", "Update available").to_string()))
                            .unwrap_or_else(|| tr!("Все игры", "All games").into());
                        let family_n = self.row_map(&net).values().filter(|r| r.family).count();
                        // Own collection (not built-in and not filter-based): rename, delete.
                        let own = self
                            .collection
                            .as_ref()
                            .and_then(|id| colls.iter().find(|c| &c.id == id && c.id.starts_with("uc-") && !c.dynamic))
                            .cloned();
                        let own_coll = own.is_some();
                        ui.horizontal(|ui| {
                        egui::ComboBox::from_id_salt("collection")
                            .width(width - if own_coll { 34.0 } else { 4.0 })
                            .truncate()
                            .selected_text(current)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.collection, None, tr!("Все игры", "All games"));
                                if family_n > 0 {
                                    ui.selectable_value(
                                        &mut self.collection,
                                        Some(FAMILY.to_string()),
                                        trf!("Семейная библиотека ({family_n})", "Family library ({family_n})"),
                                    )
                                    .on_hover_text(tr!("Игры, которыми с вами делятся члены семьи Steam", "Games shared with you by Steam Family members"));
                                }
                                let unplayed_n = net.owned.iter().filter(|g| g.playtime_forever_minutes == 0).count();
                                if unplayed_n > 0 {
                                    ui.selectable_value(
                                        &mut self.collection,
                                        Some(UNPLAYED.to_string()),
                                        trf!("Ни разу не запускались ({unplayed_n})", "Never launched ({unplayed_n})"),
                                    )
                                    .on_hover_text(tr!("Купленные игры, в которых 0 минут", "Owned games with 0 minutes played"));
                                }
                                ui.selectable_value(&mut self.collection, Some(READY.to_string()), tr!("Готово к игре", "Ready to play"))
                                    .on_hover_text(tr!("Установленные игры, которым не нужно обновление", "Installed games that need no update"));
                                ui.selectable_value(&mut self.collection, Some(UPDATES.to_string()), tr!("Есть обновление", "Update available"))
                                    .on_hover_text(tr!("Установленные игры, для которых вышло обновление", "Installed games with an update available"));
                                ui.separator();
                                for c in colls {
                                    ui.selectable_value(
                                        &mut self.collection,
                                        Some(c.id.clone()),
                                        format!("{} ({})", c.name, c.apps.len()),
                                    );
                                }
                            })
                            .response
                            .on_hover_text(tr!("Коллекции из Steam. Добавить игру - правый щелчок по ней. «Скрытые» в «Все игры» не показываются.", "Collections from Steam. To add a game, right-click it. \"Hidden\" are not shown in \"All games\"."));
                        if let Some(c) = own {
                            ui.menu_button("...", |ui| {
                                let key = egui::Id::new("coll_rename");
                                let mut name: String =
                                    ui.data_mut(|d| d.get_temp(key)).unwrap_or_else(|| c.name.clone());
                                ui.horizontal(|ui| {
                                    ui.add(egui::TextEdit::singleline(&mut name).desired_width(150.0));
                                    let ok = !name.trim().is_empty() && name.trim() != c.name;
                                    if ui.add_enabled(ok, egui::Button::new(tr!("Переименовать", "Rename"))).clicked() {
                                        let _ = self.tx.send(Cmd::Coll(colls::CollCmd::Rename {
                                            id: c.id.clone(),
                                            name: name.trim().to_string(),
                                        }));
                                        ui.data_mut(|d| d.remove::<String>(key));
                                        ui.close_menu();
                                    }
                                });
                                ui.data_mut(|d| d.insert_temp(key, name));
                                ui.separator();
                                let confirm = egui::Id::new("coll_delete_confirm");
                                let asked = ui.data(|d| d.get_temp::<String>(confirm)) == Some(c.id.clone());
                                if !asked {
                                    if ui.button(tr!("Удалить коллекцию...", "Delete collection...")).clicked() {
                                        ui.data_mut(|d| d.insert_temp(confirm, c.id.clone()));
                                    }
                                } else {
                                    ui.label(trf!("Удалить «{}»? Игры останутся в библиотеке.", "Delete \"{}\"? The games stay in the library.", c.name));
                                    ui.horizontal(|ui| {
                                        if ui.button(tr!("Удалить", "Delete")).clicked() {
                                            let _ = self.tx.send(Cmd::Coll(colls::CollCmd::Delete { id: c.id.clone() }));
                                            self.collection = None;
                                            ui.data_mut(|d| d.remove::<String>(confirm));
                                            ui.close_menu();
                                        }
                                        if ui.button(tr!("Нет", "No")).clicked() {
                                            ui.data_mut(|d| d.remove::<String>(confirm));
                                        }
                                    });
                                }
                            })
                            .response
                            .on_hover_text(tr!("Переименовать или удалить эту коллекцию (изменится и в Steam)", "Rename or delete this collection (it changes in Steam too)"));
                        }
                        });
                    }
    }

    pub(crate) fn library_filters(&mut self, ui: &mut egui::Ui, net: &NetState, toolbar: bool) {
        // No count: the search covers all library games (own, family, non-Steam), and the count
        // of "own" ones (228) next to the "All games (345)" title was confusing.
        let hint = tr!("Поиск по названию", "Search by name");
        let w = if toolbar { 220.0 } else { ui.available_width() };
        theme::search_field(ui, &mut self.filter, hint, w);
        self.collection_combo(ui, net, if toolbar { 190.0 } else { ui.available_width() });
        if !toolbar {
        egui::ComboBox::from_id_salt("sort")
            .width(ui.available_width() - 4.0)
            .truncate()
            .selected_text(trf!("Сортировка: {}", "Sort: {}", self.settings.sort.label()))
            .show_ui(ui, |ui| {
                for s in settings::Sort::ALL {
                    ui.selectable_value(&mut self.settings.sort, s, s.label());
                }
            });
        }
        self.genre_combo(ui, net, toolbar);
        ui.checkbox(&mut self.only_installed, tr!("Только установленные", "Installed only"));
        if let Some(e) = &net.collections_error {
            ui.colored_label(egui::Color32::from_rgb(230, 110, 110), e);
        }
    }

    /// Genre choice. Tags come from the store: the first time the list is opened, details of
    /// all library games are requested (50 per request).
    pub(crate) fn genre_combo(&mut self, ui: &mut egui::Ui, net: &NetState, toolbar: bool) {
        let ids: Vec<u32> = self.row_map(net).keys().copied().filter(|id| *id < crate::custom::BASE).collect();
        let (genres, loaded) = {
            let st = net.store.lock().unwrap();
            let mut count: std::collections::HashMap<String, usize> = Default::default();
            let mut loaded = 0;
            for id in &ids {
                if let Some(a) = st.apps.get(id) {
                    loaded += 1;
                    for t in a.tags.iter().take(8) {
                        if let Some(n) = &t.name {
                            *count.entry(n.clone()).or_default() += 1;
                        }
                    }
                }
            }
            let mut v: Vec<(String, usize)> = count.into_iter().filter(|(_, n)| *n >= 2).collect();
            v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            v.truncate(40);
            (v, loaded)
        };
        let text = match &self.genre {
            Some(g) => trf!("Жанр: {g}", "Genre: {g}"),
            None => tr!("Жанр: любой", "Genre: any").to_string(),
        };
        let resp = egui::ComboBox::from_id_salt("genre")
            .width(if toolbar { 170.0 } else { ui.available_width() - 4.0 })
            .truncate()
            .selected_text(text)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut self.genre, None, tr!("Любой", "Any"));
                if loaded < ids.len() {
                    ui.horizontal(|ui| {
                        crate::spin::spinner(ui);
                        ui.weak(trf!("загружаю метки: {loaded} из {}", "loading tags: {loaded} of {}", ids.len()));
                    });
                }
                for (g, n) in &genres {
                    ui.selectable_value(&mut self.genre, Some(g.clone()), format!("{g} ({n})"));
                }
            });
        if (resp.response.clicked() || self.genre.is_some()) && !self.genre_asked {
            self.genre_asked = true;
            let _ = self.tx.send(Cmd::Store(store::StoreCmd::Details(ids)));
        }
        resp.response.on_hover_text(tr!("Метки игр из магазина Steam", "Game tags from the Steam store"));
    }

    pub(crate) fn library_sidebar(&mut self, ui: &mut egui::Ui, net: &NetState, rows: &[Row], vc: &libview::Ctx) -> Option<libview::Action> {
        let mut out = None;
        let table = self.settings.view == settings::View::List || !self.settings.covers;
        // The head of the list, as in Steam: a darker block with "Home" and the collections button,
        // the list choice with "recent" and "ready to play" icons, the search with the filter button.
        egui::Frame::none().fill(theme::TOP).inner_margin(egui::Margin { left: 6.0, right: 6.0, top: 6.0, bottom: 6.0 }).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing = egui::vec2(4.0, 5.0);
            ui.horizontal(|ui| {
                let home = self.lib_page.is_none() && !self.collections_page;
                let w = ui.available_width() - 32.0;
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, 30.0), egui::Sense::click());
                let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
                let fill = if home { theme::SELECTED } else if resp.hovered() { theme::BUTTON } else { theme::SIDEBAR };
                ui.painter().rect_filled(rect, 2.0, fill);
                theme::icon_home(ui.painter(), egui::Rect::from_center_size(rect.left_center() + egui::vec2(16.0, 0.0), egui::vec2(14.0, 14.0)), theme::TEXT);
                ui.painter().text(rect.left_center() + egui::vec2(30.0, 0.0), egui::Align2::LEFT_CENTER, tr!("Главная", "Home"), egui::FontId::proportional(13.5), egui::Color32::WHITE);
                if resp.clicked() {
                    self.lib_page = None;
                    self.collections_page = false;
                }
                let (g, gr) = ui.allocate_exact_size(egui::vec2(28.0, 30.0), egui::Sense::click());
                let gr = gr.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(tr!("Коллекции", "Collections"));
                ui.painter().rect_filled(g, 2.0, if self.collections_page { theme::SELECTED } else if gr.hovered() { theme::BUTTON } else { theme::SIDEBAR });
                theme::icon_grid(ui.painter(), g.shrink2(egui::vec2(7.0, 8.0)), theme::TEXT2);
                if gr.clicked() {
                    self.lib_page = None;
                    self.collections_page = !self.collections_page;
                }
            });
            ui.horizontal(|ui| {
                let w = ui.available_width() - 48.0;
                self.collection_combo(ui, net, w);
                let recent = self.settings.sort == settings::Sort::LastPlayed;
                if theme::bare_icon(ui, theme::icon_clock, 20.0, true, recent).on_hover_text(tr!("Сначала недавние", "Recent first")).clicked() {
                    self.settings.sort = if recent { settings::Sort::Name } else { settings::Sort::LastPlayed };
                }
                let ready = self.collection.as_deref() == Some(READY);
                if theme::bare_icon(ui, theme::icon_play_circle, 20.0, true, ready).on_hover_text(tr!("Только готовые к игре", "Ready to play only")).clicked() {
                    self.collection = if ready { None } else { Some(READY.to_string()) };
                }
            });
            ui.horizontal(|ui| {
                let w = ui.available_width() - 28.0;
                theme::search_field(ui, &mut self.filter, tr!("Поиск по названию", "Search by name"), w);
                let active = self.only_installed || self.genre.is_some() || self.settings.sort != settings::Sort::Name;
                let fid = ui.make_persistent_id("lib_filters");
                let f = theme::bare_icon(ui, theme::icon_filter, 22.0, true, active).on_hover_text(tr!("Сортировка и фильтры", "Sort and filters"));
                if f.clicked() {
                    ui.memory_mut(|m| m.toggle_popup(fid));
                }
                egui::popup::popup_below_widget(ui, fid, &f, egui::PopupCloseBehavior::CloseOnClickOutside, |ui| {
                    ui.set_min_width(230.0);
                    theme::caps_title(ui, tr!("Сортировка", "Sort by"), None);
                    for s in settings::Sort::ALL {
                        ui.radio_value(&mut self.settings.sort, s, s.label());
                    }
                    ui.separator();
                    theme::caps_title(ui, tr!("Фильтры", "Filters"), None);
                    ui.checkbox(&mut self.only_installed, tr!("Только установленные", "Installed only"));
                    self.genre_combo(ui, net, false);
                });
            });
            if let Some(e) = &net.collections_error {
                ui.colored_label(egui::Color32::from_rgb(230, 110, 110), e);
            }
        });
        // In the table view games are already shown as the table on the right - a second list is not needed.
        if table {
            return out;
        }
        // The group header, as in Steam: "- ALL GAMES (shown/total)".
        let total = self.row_map(net).len();
        let title = match self.collection.as_deref() {
            None => tr!("Все игры", "All games").to_string(),
            Some(FAMILY) => tr!("Семейная библиотека", "Family library").to_string(),
            Some(UNPLAYED) => tr!("Ни разу не запускались", "Never launched").to_string(),
            Some(READY) => tr!("Готово к игре", "Ready to play").to_string(),
            Some(UPDATES) => tr!("Есть обновление", "Update available").to_string(),
            Some(id) => net.collections.as_ref().and_then(|c| c.iter().find(|c| c.id == id)).map(|c| c.name.clone()).unwrap_or_default(),
        };
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            let mut job = theme::spaced(&format!("- {}", title.to_uppercase()), theme::semibold(11.5), theme::TEXT2, 0.8);
            job.append(&format!("  ({}/{total})", rows.len()), 0.0, egui::TextFormat { font_id: egui::FontId::proportional(11.0), color: theme::DIM, ..Default::default() });
            ui.add(egui::Label::new(job).selectable(false).truncate());
        });
        ui.add_space(2.0);
        if let Some(a) = libview::sidebar(ui, rows, vc, &self.covers, self.lib_page, self.settings.covers) {
            out = Some(a);
        }
        out
    }
}
