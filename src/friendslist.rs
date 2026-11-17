//! The friends panel on the right, as the Steam friends list, and opening a chat.

use crate::*;

impl LauncherApp {
    pub(crate) fn friends_panel(&mut self, ui: &mut egui::Ui, net: &NetState) {
        // Friend list snapshot: (id, name, status, game, unread).
        let mut unknown_apps: Vec<u32> = Vec::new();
        let mut list: Vec<(u64, String, u32, Option<String>, u32)> = {
            let roster = net.roster.read().unwrap();
            roster
                .values()
                // 3 = friend (EFriendRelationship); Steam groups (account type 7) are in the same list - we hide them.
                .filter(|f| f.relationship == 3 && (f.steam_id >> 52) & 0xF == 1)
                .map(|f| {
                    // Game name: from Steam, from our library, from already resolved ones, otherwise the id.
                    let game = f.game_name.clone().filter(|g| !g.is_empty()).or_else(|| {
                        let a = f.game_app_id.filter(|a| *a != 0)?;
                        let known = net
                            .app_names
                            .get(&a)
                            .cloned()
                            .or_else(|| net.owned.iter().find(|g| g.app_id == a).map(|g| g.name.clone()));
                        Some(known.unwrap_or_else(|| {
                            unknown_apps.push(a);
                            trf!("игра {a}", "game {a}")
                        }))
                    });
                    // Own nickname for the friend (set in Steam) comes first, the Steam name in parentheses, as in Steam.
                    // The name has not arrived yet (Steam sends it a bit later for offline friends) - do not show the id.
                    let persona = f.persona_name.clone().filter(|n| !n.is_empty()).unwrap_or_else(|| tr!("загружается...", "loading...").into());
                    let shown = match &f.nickname {
                        Some(n) => format!("{n} ({persona})"),
                        None => persona,
                    };
                    // Detailed in-game status (map, score), if the game reports it.
                    let game = match (game, &f.rich_status) {
                        (Some(g), Some(rs)) => Some(format!("{g} · {rs}")),
                        (g, _) => g,
                    };
                    (
                        f.steam_id,
                        shown,
                        f.persona_state.unwrap_or(0),
                        game,
                        net.chats.get(&f.steam_id).map_or(0, |c| c.unread),
                    )
                })
                .collect()
        };
        // Unknown names are requested once.
        unknown_apps.retain(|a| self.requested_apps.insert(*a));
        if !unknown_apps.is_empty() {
            let _ = self.tx.send(Cmd::ResolveApps(unknown_apps));
        }
        let online = list.iter().filter(|f| f.2 != 0).count();
        // Order: with new messages, in game, online, offline; within each - by name.
        let rank = |f: &(u64, String, u32, Option<String>, u32)| {
            if f.4 > 0 {
                0
            } else if f.3.is_some() {
                1
            } else if f.2 != 0 {
                2
            } else {
                3
            }
        };
        // Favorites on top (after friends with new messages).
        let favs = self.settings.fav_friends.clone();
        list.sort_by(|a, b| {
            let fa = a.4 == 0 && favs.contains(&a.0);
            let fb = b.4 == 0 && favs.contains(&b.0);
            (a.4 == 0).cmp(&(b.4 == 0)).then(fb.cmp(&fa)).then(rank(a).cmp(&rank(b))).then(a.1.to_lowercase().cmp(&b.1.to_lowercase()))
        });
        // Avatars (hash from the friend status): avatars.fastly.steamstatic.com/<hash>.jpg - 32x32.
        let avatars: HashMap<u64, String> = net
            .roster
            .read()
            .unwrap()
            .values()
            .filter_map(|f| f.avatar_hash.as_ref().map(|h| (f.steam_id, format!("https://avatars.fastly.steamstatic.com/{h}.jpg"))))
            .collect();
        let notes = self.settings.friend_notes.clone();

        // Header with the menu, friend requests.
        self.friends_header(ui, net);
        if list.is_empty() {
            ui.horizontal(|ui| {
                crate::spin::spinner(ui);
                ui.weak(tr!("Загружаю список...", "Loading the list..."));
            });
            return;
        }
        // The "FRIENDS" bar, as in the Steam friends list: a darker strip with the count and the search.
        ui.add_space(4.0);
        egui::Frame::none().fill(egui::Color32::from_rgb(44, 49, 57)).inner_margin(egui::Margin::symmetric(8.0, 4.0)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let mut job = theme::spaced(&tr!("Друзья", "Friends").to_uppercase(), theme::semibold(11.5), theme::TEXT2, 1.0);
                job.append(&format!("  {online} / {}", list.len()), 0.0, egui::TextFormat { font_id: egui::FontId::proportional(11.0), color: theme::DIM, ..Default::default() });
                ui.add(egui::Label::new(job).selectable(false));
            });
            ui.add_space(3.0);
            ui.add(egui::TextEdit::singleline(&mut self.friend_filter).hint_text(tr!("поиск друзей", "search friends")).desired_width(f32::INFINITY));
        });
        ui.add_space(4.0);
        let filter = self.friend_filter.to_lowercase();
        let mut open = None;
        let mut friend_act = None;
        // Groups, as in Steam: favorites, in game, online, offline (collapsible).
        let group_of = |f: &(u64, String, u32, Option<String>, u32)| -> u8 {
            if favs.contains(&f.0) {
                3
            } else if f.2 == 0 {
                2
            } else if f.3.is_some() {
                0
            } else {
                1
            }
        };
        let mut toggle_group: Option<u8> = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for (gid, gname) in [(3u8, tr!("Избранные", "Favorites")), (0, tr!("В игре", "In-Game")), (1, tr!("Друзья в сети", "Online Friends")), (2, tr!("Не в сети", "Offline"))] {
            let members: Vec<&(u64, String, u32, Option<String>, u32)> = list
                .iter()
                .filter(|f| group_of(f) == gid)
                .filter(|f| {
                    let note = notes.get(&f.0).map(|n| n.to_lowercase()).unwrap_or_default();
                    filter.is_empty() || f.1.to_lowercase().contains(&filter) || note.contains(&filter)
                })
                .collect();
            if members.is_empty() {
                continue;
            }
            // Group header: a click collapses it (all groups are expanded while searching).
            let closed = filter.is_empty() && self.settings.friend_groups_closed.contains(&gid);
            let head = ui
                .horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 14.0), egui::Sense::hover());
                    let c = r.center();
                    let pts = if closed {
                        vec![c + egui::vec2(-2.5, -4.0), c + egui::vec2(3.0, 0.0), c + egui::vec2(-2.5, 4.0)]
                    } else {
                        vec![c + egui::vec2(-4.0, -2.0), c + egui::vec2(4.0, -2.0), c + egui::vec2(0.0, 3.0)]
                    };
                    ui.painter().add(egui::Shape::convex_polygon(pts, theme::WEAK, egui::Stroke::NONE));
                    let mut job = egui::text::LayoutJob::default();
                    job.append(gname, 0.0, egui::TextFormat { font_id: egui::FontId::proportional(12.5), color: theme::TEXT2, ..Default::default() });
                    job.append(&format!("  ({})", members.len()), 0.0, egui::TextFormat { font_id: egui::FontId::proportional(11.5), color: theme::DIM, ..Default::default() });
                    ui.add(egui::Label::new(job).selectable(false));
                })
                .response;
            let head = ui.interact(head.rect, ui.id().with(("friend_group", gid)), egui::Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
            if head.clicked() {
                toggle_group = Some(gid);
            }
            ui.add_space(2.0);
            if closed {
                ui.add_space(4.0);
                continue;
            }
            for &(id, name, state, game, unread) in members.iter().map(|f| (&f.0, &f.1, &f.2, &f.3, &f.4)).collect::<Vec<_>>().iter() {
                let note = notes.get(id).cloned().unwrap_or_default();
                let (color, status) = match (state, game) {
                    (_, Some(g)) => (theme::IN_GAME, g.clone()),
                    (0, _) => (theme::OFFLINE, tr!("не в сети", "Offline").into()),
                    (1, _) => (theme::ONLINE, tr!("в сети", "Online").into()),
                    (2, _) => (egui::Color32::from_rgb(220, 120, 110), tr!("не беспокоить", "do not disturb").into()),
                    (3 | 4, _) => (egui::Color32::from_rgb(110, 140, 170), tr!("отошел", "away").into()),
                    _ => (theme::ONLINE, tr!("в сети", "Online").into()),
                };
                let mut title = egui::RichText::new(crate::richtext::fix(ui, name)).size(13.0).color(if *state == 0 { theme::OFFLINE } else { color });
                if *unread > 0 {
                    title = title.strong();
                }
                // The whole row (avatar, name, status) is one button across the panel width.
                let block = ui
                    .horizontal(|ui| {
                    // Avatar with a status-colored border, as in Steam.
                    let (ar, _) = ui.allocate_exact_size(egui::vec2(32.0, 32.0), egui::Sense::hover());
                    match avatars.get(id) {
                        Some(u) => {
                            egui::Image::new(u.as_str()).rounding(2.0).paint_at(ui, ar);
                        }
                        None => {
                            ui.painter().rect_filled(ar, 2.0, theme::CARD);
                        }
                    }
                    ui.painter().rect_stroke(ar.expand(1.0), 2.0, egui::Stroke::new(1.5, if *state == 0 { egui::Color32::from_gray(90) } else { color }));
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.add(egui::Label::new(title).truncate());
                            if favs.contains(id) {
                                ui.label(egui::RichText::new(tr!("избранный", "favorite")).small().color(egui::Color32::from_rgb(230, 190, 80)));
                            }
                            if *unread > 0 {
                                ui.colored_label(egui::Color32::from_rgb(240, 200, 80), format!("● {unread}"));
                            }
                        });
                        ui.add(egui::Label::new(egui::RichText::new(&status).small().color(color)).truncate());
                        if !note.is_empty() {
                            ui.label(egui::RichText::new(&note).small().italics().weak());
                        }
                    });
                    })
                    .response;
                let rect = egui::Rect::from_min_max(
                    egui::pos2(ui.max_rect().left(), block.rect.top() - 2.0),
                    egui::pos2(ui.max_rect().right(), block.rect.bottom() + 2.0),
                );
                let resp = ui
                    .interact(rect, ui.id().with(("friend", *id)), egui::Sense::click())
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    // Mini profile on hover, as in Steam: large avatar, name, status, note.
                    .on_hover_ui(|ui| {
                        ui.horizontal(|ui| {
                            let (ar, _) = ui.allocate_exact_size(egui::vec2(64.0, 64.0), egui::Sense::hover());
                            match avatars.get(id) {
                                Some(u) => {
                                    egui::Image::new(u.replace(".jpg", "_medium.jpg")).rounding(3.0).paint_at(ui, ar);
                                }
                                None => {
                                    ui.painter().rect_filled(ar, 3.0, theme::CARD);
                                }
                            }
                            ui.painter().rect_stroke(ar.expand(1.0), 3.0, egui::Stroke::new(2.0, if *state == 0 { egui::Color32::from_gray(90) } else { color }));
                            ui.vertical(|ui| {
                                ui.label(egui::RichText::new(crate::richtext::fix(ui, name)).size(15.0).strong());
                                ui.label(egui::RichText::new(&status).color(color));
                                if !note.is_empty() {
                                    ui.label(egui::RichText::new(&note).italics().weak());
                                }
                                ui.weak(tr!("Щелчок - чат, правый щелчок - меню", "Click - chat, right click - menu"));
                            });
                        });
                    });
                if resp.hovered() {
                    ui.painter().rect_filled(
                        rect,
                        3.0,
                        egui::Color32::from_rgba_unmultiplied(255, 255, 255, 14),
                    );
                }
                if resp.clicked() {
                    open = Some((*id, name.clone()));
                }
                // Right click: profile, message, remove from friends.
                socialwin::friend_menu(&resp, *id, name, favs.contains(id), &mut friend_act);
                ui.add_space(4.0);
            }
            ui.add_space(4.0);
            }
        });
        if let Some(g) = toggle_group {
            if !self.settings.friend_groups_closed.remove(&g) {
                self.settings.friend_groups_closed.insert(g);
            }
        }
        let ctx = ui.ctx().clone();
        if let Some((id, name)) = open {
            self.open_chat(&ctx, id, name, net);
        }
        if let Some(a) = friend_act {
            self.friend_action(a, net, &ctx);
        }
    }

    pub(crate) fn open_chat(&mut self, ctx: &egui::Context, id: u64, name: String, net: &NetState) {
        if !net.chats.get(&id).is_some_and(|c| c.history_loaded) {
            let _ = self.tx.send(Cmd::OpenChat(id));
        }
        // The same friend - just keep the window; another one - switch the window to them.
        // The window is already open - a new conversation opens as a tab (as in Steam).
        let mut chat = self.chat.lock().unwrap();
        match chat.as_mut() {
            Some(c) => c.switch_to(id, name),
            None => *chat = Some(chatwin::ChatWin::new(id, name)),
        }
        drop(chat);
        // If the chat window is already open, bring it to the front (a command to a nonexistent window
        // would create an empty "egui window").
        if notify::chat_window_open() {
            ctx.send_viewport_cmd_to(chatwin::viewport_id(), egui::ViewportCommand::Focus);
        }
    }
    /// The friends list as a separate window (setting), as the Steam friends list.
    pub(crate) fn friends_window(&mut self, ctx: &egui::Context, net: &NetState) {
        if !self.settings.friends_window || !self.show_friends || net.account.is_none() || self.settings.games_only {
            return;
        }
        let builder = egui::ViewportBuilder::default()
            .with_title(tr!("Друзья - Vaporust", "Friends - Vaporust"))
            .with_inner_size([300.0, 620.0])
            .with_min_inner_size([240.0, 360.0])
            .with_icon(crate::icons::icon(crate::icons::Kind::Chat))
            .with_decorations(crate::winframe::system());
        let mut close = false;
        ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("vaporust_friends"), builder, |ctx, _| {
            crate::winframe::title_bar_auto(ctx);
            let frame = egui::Frame::none().fill(theme::SIDEBAR).inner_margin(egui::Margin::same(6.0));
            egui::CentralPanel::default().frame(frame).show(ctx, |ui| self.friends_panel(ui, net));
            if ctx.input(|i| i.viewport().close_requested()) {
                close = true;
            }
        });
        if close {
            self.show_friends = false;
        }
    }
}
