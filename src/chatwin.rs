//! Окно чата - отдельное окно Windows (отложенный viewport egui).
//! Перерисовывается само по себе, не трогая главное окно: анимации стикеров и загрузка
//! картинок не заставляют перерисовываться библиотеку.

use crate::net::{Cmd, NetState, Shared};
use crate::{anim, chatfmt};
use eframe::egui;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc::UnboundedSender;

pub fn viewport_id() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("vaporust_chat")
}

/// Состояние открытого чата.
pub struct ChatWin {
    pub id: u64,
    pub name: String,
    input: String,
    picker_open: bool,
    /// Вкладка панели: 0 смайлики, 1 стикеры, 2 эффекты.
    picker_tab: u8,
    /// Повторная загрузка сломанных значков эффектов уже была.
    icons_retried: bool,
    /// Когда последний раз просили список своих эффектов.
    effects_asked: Option<std::time::Instant>,
    emoticons_requested: bool,
    /// Кадры анимированных стикеров (живут, пока открыт чат).
    anims: anim::Cache,
    /// Была ли нажата V в прошлом кадре (для Ctrl+V с картинкой).
    v_was_down: bool,
    /// Поставить курсор в поле ввода (сразу после открытия).
    focus_input: bool,
    /// Идущий эффект комнаты и время последнего показанного (чтобы не повторять).
    effect: Option<crate::effects::Effect>,
    effect_seen: u64,
    /// Поиск по переписке (пусто - показывать все).
    search: String,
    /// Строка поиска открыта (лупа или Ctrl+F) и в нее надо поставить курсор.
    search_open: bool,
    focus_search: bool,
    /// Копия сетевого состояния и номер изменения, с которого она снята (не копировать каждый кадр).
    snap: Option<NetState>,
    snap_gen: u64,
}

impl ChatWin {
    pub fn new(id: u64, name: String) -> Self {
        Self {
            id,
            name,
            input: String::new(),
            picker_open: false,
            picker_tab: 0,
            icons_retried: false,
            effects_asked: None,
            emoticons_requested: false,
            anims: Default::default(),
            v_was_down: false,
            focus_input: true,
            effect: None,
            effect_seen: 0,
            search: String::new(),
            search_open: false,
            focus_search: false,
            snap: None,
            snap_gen: u64::MAX,
        }
    }
}

pub type SharedChat = Arc<Mutex<Option<ChatWin>>>;

/// Вызывается из кадра главного окна, пока чат открыт.
pub fn show(ctx: &egui::Context, chat: &SharedChat, net: &Shared, tx: &UnboundedSender<Cmd>) {
    let title = match chat.lock().unwrap().as_ref() {
        Some(c) => format!("Чат: {}", c.name),
        None => return,
    };
    let (chat, net, tx) = (chat.clone(), net.clone(), tx.clone());
    ctx.show_viewport_deferred(
        viewport_id(),
        egui::ViewportBuilder::default()
            .with_title(title)
            .with_inner_size([520.0, 580.0])
            .with_min_inner_size([360.0, 320.0])
            .with_icon(crate::icons::icon(crate::icons::Kind::Chat))
            .with_decorations(crate::winframe::system()),
        move |ctx, _class| {
            crate::winframe::title_bar_auto(ctx);
            let mut guard = chat.lock().unwrap();
            let Some(win) = guard.as_mut() else { return };
            // Снимок сетевого состояния: копируем заново, только когда оно поменялось.
            let gen = net.gen();
            if win.snap.is_none() || win.snap_gen != gen {
                win.snap = Some(net.lock().unwrap().clone());
                win.snap_gen = gen;
            }
            let snapshot = win.snap.take().unwrap_or_default();
            win.frame(ctx, &snapshot, &net, &tx);
            win.snap = Some(snapshot);
            if ctx.input(|i| i.viewport().close_requested()) {
                *guard = None;
                crate::notify::set_open_chat(0);
                // Крупные картинки чата больше не нужны (аватары остаются для главного окна).
                crate::imgload::forget_large(ctx);
                ctx.request_repaint_of(egui::ViewportId::ROOT);
            }
        },
    );
}

impl ChatWin {
    fn frame(&mut self, ctx: &egui::Context, net: &NetState, shared: &Shared, tx: &UnboundedSender<Cmd>) {
        let id = self.id;
        // Для уведомлений: со звуком не беспокоим, если активен чат именно с этим другом.
        crate::notify::set_open_chat(id);
        // Окно открыто и активно - значит, все прочитано.
        if ctx.input(|i| i.focused) && net.chats.get(&id).is_some_and(|c| c.unread > 0) {
            if let Some(c) = shared.lock().unwrap().chats.get_mut(&id) {
                c.unread = 0;
            }
            ctx.request_repaint_of(egui::ViewportId::ROOT);
        }
        let conv = net.chats.get(&id).cloned().unwrap_or_default();
        let mut send = None;
        let mut send_image: Option<std::path::PathBuf> = None;

        // Нижняя часть: статус, панель смайликов, поле ввода.
        egui::TopBottomPanel::bottom("chat_input").show(ctx, |ui| {
            ui.add_space(4.0);
            if conv
                .typing_at
                .is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(6))
            {
                ui.weak(format!("{} печатает...", self.name));
            }
            if let Some(s) = &conv.status {
                ui.horizontal(|ui| {
                    crate::spin::spinner(ui);
                    ui.weak(s);
                });
            }
            if let Some(e) = &conv.error {
                ui.colored_label(egui::Color32::from_rgb(230, 110, 110), e);
            }
            if let Some(n) = &conv.status_note {
                ui.colored_label(egui::Color32::from_rgb(110, 200, 110), n);
            }
            // Вкладка «Эффекты» открыта - перечитать свои эффекты (могли купить), не чаще раза в 20 с.
            if self.picker_open
                && self.picker_tab == 2
                && self.effects_asked.is_none_or(|t| t.elapsed() > std::time::Duration::from_secs(20))
            {
                self.effects_asked = Some(std::time::Instant::now());
                let _ = tx.send(Cmd::LoadChatEffects);
            }
            if self.picker_open {
                if let Some(msg) = self.picker(ui, net) {
                    send = Some(msg);
                }
                ui.separator();
            }
            ui.horizontal(|ui| {
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut self.input)
                        .hint_text("Сообщение, Enter - отправить")
                        .desired_width(ui.available_width() - 150.0),
                );
                if std::mem::take(&mut self.focus_input) {
                    edit.request_focus();
                }
                let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if ui
                    .selectable_label(self.picker_open, ":)")
                    .on_hover_text("Смайлики и стикеры")
                    .clicked()
                {
                    self.picker_open = !self.picker_open;
                    if self.picker_open && net.emoticons.is_none() && !self.emoticons_requested {
                        self.emoticons_requested = true;
                        let _ = tx.send(Cmd::LoadEmoticons);
                    }
                }
                if ui
                    .button("🖼")
                    .on_hover_text("Отправить картинку (или перетащите файл в окно, или Ctrl+V)")
                    .clicked()
                {
                    if let Some(path) = rfd::FileDialog::new()
                        .set_title("Картинка для отправки")
                        .add_filter("Картинки", &["png", "jpg", "jpeg", "gif", "webp", "bmp"])
                        .pick_file()
                    {
                        send_image = Some(path);
                    }
                }
                let clicked = ui.button("Отправить").clicked();
                if (enter || clicked) && !self.input.trim().is_empty() {
                    let text = std::mem::take(&mut self.input).trim().to_string();
                    send = Some(crate::emoticons_to_bbcode(&text, net));
                    edit.request_focus();
                } else if enter {
                    edit.request_focus();
                }
            });
            ui.add_space(4.0);
        });

        // Шапка, как в Steam: аватар друга, имя и статус; справа - лупа (поиск, Ctrl+F).
        let friend = net.roster.read().unwrap().get(&id).cloned();
        let friend_avatar = friend
            .as_ref()
            .and_then(|f| f.avatar_hash.as_ref())
            .map(|h| format!("https://avatars.fastly.steamstatic.com/{h}_medium.jpg"));
        let my_avatar = net.my_id.and_then(|m| net.profiles.get(&m)).and_then(|p| p.as_ref().ok()).and_then(|p| p.avatar_url.clone());
        let (status, status_color) = match friend.as_ref() {
            Some(f) if f.game_name.as_ref().is_some_and(|g| !g.is_empty()) => {
                (format!("В игре: {}", f.game_name.clone().unwrap_or_default()), egui::Color32::from_rgb(140, 200, 80))
            }
            Some(f) if f.persona_state.unwrap_or(0) == 0 => ("не в сети".to_string(), egui::Color32::GRAY),
            Some(f) if matches!(f.persona_state, Some(3 | 4)) => ("отошел".to_string(), egui::Color32::from_rgb(110, 140, 170)),
            Some(_) => ("в сети".to_string(), egui::Color32::from_rgb(90, 170, 230)),
            None => (String::new(), egui::Color32::GRAY),
        };
        // Ctrl+F - поиск, Esc - закрыть поиск.
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::F)) {
            self.search_open = true;
            self.focus_search = true;
        }
        if self.search_open && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.search_open = false;
            self.search.clear();
            self.focus_input = true;
        }
        egui::TopBottomPanel::top("chat_head").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let (ar, _) = ui.allocate_exact_size(egui::vec2(36.0, 36.0), egui::Sense::hover());
                match &friend_avatar {
                    Some(u) => {
                        egui::Image::new(u.as_str()).rounding(3.0).paint_at(ui, ar);
                    }
                    None => {
                        ui.painter().rect_filled(ar, 3.0, crate::theme::CARD);
                    }
                }
                ui.painter().rect_stroke(ar.expand(1.0), 3.0, egui::Stroke::new(1.5, status_color));
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.label(egui::RichText::new(crate::richtext::fix(ui, &self.name)).size(15.0).strong());
                    ui.add(egui::Label::new(egui::RichText::new(&status).size(12.0).color(status_color)).truncate());
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let r = ui
                        .add(egui::Button::new("").frame(self.search_open).min_size(egui::vec2(30.0, 26.0)))
                        .on_hover_text("Поиск в переписке (Ctrl+F)");
                    draw_magnifier(ui.painter(), r.rect, if r.hovered() { egui::Color32::WHITE } else { crate::theme::TEXT });
                    if r.clicked() {
                        self.search_open = !self.search_open;
                        if self.search_open {
                            self.focus_search = true;
                        } else {
                            self.search.clear();
                        }
                    }
                    if conv.loading_older {
                        crate::spin::spinner(ui);
                    }
                });
            });
            if self.search_open {
                ui.horizontal(|ui| {
                    let edit = ui.add(
                        egui::TextEdit::singleline(&mut self.search)
                            .hint_text("Поиск в переписке, Esc - закрыть")
                            .desired_width(ui.available_width() - 30.0),
                    );
                    if std::mem::take(&mut self.focus_search) {
                        edit.request_focus();
                    }
                    if ui.small_button("✖").on_hover_text("Закрыть поиск").clicked() {
                        self.search_open = false;
                        self.search.clear();
                    }
                });
            }
            ui.add_space(4.0);
        });
        let query = self.search.trim().to_lowercase();
        // Переписка: сообщения группами по отправителю (аватар, имя, время), между днями - разделитель.
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .stick_to_bottom(true)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if !conv.history_loaded && conv.error.is_none() {
                        ui.horizontal(|ui| {
                            crate::spin::spinner(ui);
                            ui.weak("Загружаю переписку...");
                        });
                    }
                    if conv.history_loaded && conv.lines.is_empty() {
                        ui.add_space(20.0);
                        ui.vertical_centered(|ui| ui.weak("Переписки пока нет. Напишите первым."));
                    }
                    if conv.more_history && ui.add_enabled(!conv.loading_older, egui::Button::new("Показать более ранние сообщения")).clicked() {
                        let _ = tx.send(Cmd::OlderChat(id));
                    }
                    if !query.is_empty() {
                        let n = conv.lines.iter().filter(|l| l.text.to_lowercase().contains(&query)).count();
                        ui.weak(format!("Найдено: {n} (среди загруженных {} сообщений)", conv.lines.len()));
                    }
                    let mut last_day = String::new();
                    let mut last: Option<(bool, u64)> = None;
                    for l in &conv.lines {
                        if !query.is_empty() && !l.text.to_lowercase().contains(&query) {
                            continue;
                        }
                        if l.text.trim().is_empty() {
                            continue; // пустые (например, эффект, которого нет у отправителя) - как в Steam, не показываем
                        }
                        let day = crate::fmt_day(l.time);
                        if day != last_day {
                            last_day = day.clone();
                            last = None;
                            ui.add_space(6.0);
                            ui.vertical_centered(|ui| ui.label(egui::RichText::new(&day).size(11.5).color(crate::theme::WEAK)));
                            ui.add_space(2.0);
                        }
                        let who = if l.from_self { "Вы" } else { self.name.as_str() };
                        // Новая группа: другой отправитель или перерыв больше 5 минут.
                        let new_group = last.is_none_or(|(s, t)| s != l.from_self || l.time.saturating_sub(t) > 300);
                        last = Some((l.from_self, l.time));
                        let avatar = if l.from_self { my_avatar.as_ref() } else { friend_avatar.as_ref() };
                        let color = if l.from_self { egui::Color32::from_rgb(120, 170, 230) } else { egui::Color32::from_rgb(140, 200, 80) };
                        ui.horizontal_top(|ui| {
                            let (ar, _) = ui.allocate_exact_size(egui::vec2(32.0, if new_group { 32.0 } else { 1.0 }), egui::Sense::hover());
                            if new_group {
                                match avatar {
                                    Some(u) => {
                                        egui::Image::new(u.as_str()).rounding(3.0).paint_at(ui, ar);
                                    }
                                    None => {
                                        ui.painter().rect_filled(ar, 3.0, crate::theme::CARD);
                                    }
                                }
                            }
                            ui.vertical(|ui| {
                                if new_group {
                                    ui.horizontal(|ui| {
                                        ui.label(egui::RichText::new(crate::richtext::fix(ui, who)).color(color).strong());
                                        ui.label(egui::RichText::new(crate::fmt_time(l.time)).size(11.0).color(crate::theme::WEAK));
                                    });
                                }
                                ui.horizontal_wrapped(|ui| {
                                    for seg in chatfmt::parse(&l.text) {
                                        crate::draw_seg(ui, seg, &mut self.anims, who);
                                    }
                                });
                            });
                        });
                        if new_group {
                            ui.add_space(2.0);
                        }
                    }
                });
        });

        // Ctrl+V с картинкой в буфере обмена. egui сообщает о вставке только для текста,
        // поэтому нажатие V смотрим через Windows (кадр перерисовывается на каждое нажатие клавиши).
        let v_down = crate::key_v_down();
        if v_down && !self.v_was_down && ctx.input(|i| i.modifiers.command && i.focused) {
            match crate::clipboard_image_file() {
                Ok(Some(path)) => send_image = Some(path),
                Ok(None) => {} // в буфере текст - его вставит само поле ввода
                Err(e) => {
                    if let Some(c) = shared.lock().unwrap().chats.get_mut(&id) {
                        c.error = Some(e);
                    }
                }
            }
        }
        self.v_was_down = v_down;
        // Файл, перетащенный в окно чата, - отправляем как картинку.
        if let Some(path) = ctx.input(|i| i.raw.dropped_files.iter().find_map(|f| f.path.clone())) {
            send_image = Some(path);
        }
        // Эффект комнаты: свежий (не старше 20 с) из переписки или «показать еще раз».
        let now = crate::net::now_unix();
        if let Some(l) = conv.lines.iter().rev().take(5).find(|l| l.text.contains("[roomeffect")) {
            if l.time > self.effect_seen && now.saturating_sub(l.time) <= 20 {
                self.effect_seen = l.time;
                if let Some(chatfmt::Seg::RoomEffect(kind)) =
                    chatfmt::parse(&l.text).into_iter().find(|s| matches!(s, chatfmt::Seg::RoomEffect(_)))
                {
                    // Без анимаций (настройка) эффект сам не запускается; «Повторить эффект» работает.
                    if crate::imgload::animations() {
                        self.effect = crate::effects::Effect::new(&kind);
                    }
                }
            }
        }
        if let Some(kind) = crate::effects::take_replay() {
            self.effect = crate::effects::Effect::new(&kind);
        }
        if self.effect.as_mut().is_some_and(|e| !e.draw(ctx)) {
            self.effect = None;
        }
        if let Some(text) = send {
            let _ = tx.send(Cmd::SendChat { to: id, text });
        }
        if let Some(path) = send_image {
            let _ = tx.send(Cmd::SendImage { to: id, path });
        }
    }

    /// Панель смайликов и стикеров. Смайлик вставляется в поле ввода как :имя:,
    /// стикер отправляется сразу (возвращается текст сообщения).
    fn picker(&mut self, ui: &mut egui::Ui, net: &NetState) -> Option<String> {
        let mut send = None;
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.picker_tab, 0, "Смайлики");
            ui.selectable_value(&mut self.picker_tab, 1, "Стикеры");
            ui.selectable_value(&mut self.picker_tab, 2, "Эффекты");
        });
        let Some((emoticons, stickers)) = &net.emoticons else {
            ui.horizontal(|ui| {
                crate::spin::spinner(ui);
                ui.weak("Загружаю ваши смайлики и стикеры...");
            });
            return None;
        };
        egui::ScrollArea::vertical()
            .id_salt("picker")
            .max_height(170.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    if self.picker_tab == 2 {
                        // Только купленные эффекты (из инвентаря Steam; перечитываются при открытии вкладки).
                        let Some(list) = net.chat_effects.clone() else {
                            crate::spin::spinner(ui);
                            ui.weak("Загружаю ваши эффекты...");
                            return;
                        };
                        if list.is_empty() {
                            ui.vertical(|ui| {
                                ui.weak("У вас нет эффектов комнаты. Их получают за очки в магазине очков Steam.");
                                ui.hyperlink_to("Магазин очков Steam", "https://store.steampowered.com/points/shop/");
                            });
                        }
                        // Значок не загрузился (сбой сети) - один раз забыть ошибку, чтобы egui попробовал снова.
                        if !self.icons_retried {
                            self.icons_retried = true;
                            for name in &list {
                                let url = crate::effects::icon_url(name);
                                if ui.ctx().try_load_image(&url, Default::default()).is_err() {
                                    ui.ctx().forget_image(&url);
                                }
                            }
                        }
                        for name in &list {
                            let img = egui::Image::new(crate::effects::icon_url(name))
                                .fit_to_exact_size(egui::vec2(48.0, 48.0));
                            if ui
                                .add(egui::ImageButton::new(img))
                                .on_hover_text(format!("{} - отправить", crate::effects::short_name(name)))
                                .clicked()
                            {
                                // Как в клиенте Steam: команда /roomeffect, сервер превращает ее в [roomeffect].
                                send = Some(format!("/roomeffect {name}"));
                                self.picker_open = false;
                            }
                        }
                    } else if self.picker_tab == 0 {
                        if emoticons.is_empty() {
                            ui.weak("У вас нет смайликов Steam.");
                        }
                        for name in emoticons {
                            let img = egui::Image::new(chatfmt::emoticon_url(name))
                                .fit_to_exact_size(egui::vec2(32.0, 32.0));
                            if ui
                                .add(egui::ImageButton::new(img))
                                .on_hover_text(format!(":{name}:"))
                                .clicked()
                            {
                                if !self.input.is_empty() && !self.input.ends_with(' ') {
                                    self.input.push(' ');
                                }
                                self.input.push_str(&format!(":{name}: "));
                                self.focus_input = true;
                            }
                        }
                    } else {
                        if stickers.is_empty() {
                            ui.weak("У вас нет стикеров (или инвентарь Steam недоступен).");
                        }
                        for name in stickers {
                            let img = egui::Image::new(chatfmt::sticker_url(name))
                                .fit_to_exact_size(egui::vec2(72.0, 72.0));
                            if ui
                                .add(egui::ImageButton::new(img))
                                .on_hover_text(format!("{name} - отправить"))
                                .clicked()
                            {
                                // Стикер шлется командой /sticker: сервер сам превращает ее в [sticker type=...].
                                // Готовый тег [sticker] сервер вырезает (получатель видит пустое сообщение).
                                send = Some(format!("/sticker {name}"));
                                self.picker_open = false;
                            }
                        }
                    }
                });
            });
        send
    }
}

#[cfg(test)]
mod history_live {
    /// Живой тест (только чтение): подгрузка более старой переписки. Печатает только числа.
    #[test]
    #[ignore]
    fn older_history_live() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let friends = c.collect_friends(std::time::Duration::from_secs(5)).await.unwrap();
            for f in friends.iter().filter(|f| f.relationship == 3) {
                let (first, more) = c.chat_history(f.steam_id, 50, None).await.unwrap();
                if first.len() < 50 || !more {
                    continue;
                }
                let oldest = first.iter().min_by_key(|m| (m.timestamp, m.ordinal)).unwrap();
                let (older, more2) = c.chat_history(f.steam_id, 50, Some((oldest.timestamp, oldest.ordinal))).await.unwrap();
                let newest_older = older.iter().map(|m| (m.timestamp, m.ordinal)).max();
                println!("первая порция {}, еще {}, следующая {} (все старше: {}), еще {}", first.len(), more, older.len(), newest_older.is_some_and(|n| n < (oldest.timestamp, oldest.ordinal)), more2);
                assert!(!older.is_empty());
                return;
            }
            println!("нет переписки длиннее 50 сообщений");
        });
    }
}

/// Значок лупы (рисуется линиями, без шрифта).
fn draw_magnifier(p: &egui::Painter, r: egui::Rect, color: egui::Color32) {
    let c = r.center() + egui::vec2(-2.0, -2.0);
    let s = egui::Stroke::new(1.6, color);
    p.circle_stroke(c, 5.0, s);
    p.line_segment([c + egui::vec2(3.6, 3.6), c + egui::vec2(8.0, 8.0)], egui::Stroke::new(2.0, color));
}
