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
            .with_icon(crate::icons::icon(crate::icons::Kind::Chat)),
        move |ctx, _class| {
            let mut guard = chat.lock().unwrap();
            let Some(win) = guard.as_mut() else { return };
            // Снимок сетевого состояния на кадр.
            let snapshot = net.lock().unwrap().clone();
            win.frame(ctx, &snapshot, &net, &tx);
            if ctx.input(|i| i.viewport().close_requested()) {
                *guard = None;
                crate::notify::set_open_chat(0);
                // Картинки и кадры чата больше не нужны - освобождаем память.
                ctx.forget_all_images();
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
                    ui.spinner();
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

        // Переписка.
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .stick_to_bottom(true)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if !conv.history_loaded && conv.error.is_none() {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.weak("Загружаю переписку...");
                        });
                    }
                    if conv.history_loaded && conv.lines.is_empty() {
                        ui.weak("Сообщений пока нет.");
                    }
                    for l in &conv.lines {
                        if l.text.trim().is_empty() {
                            continue; // пустые (например, эффект, которого нет у отправителя) - как в Steam, не показываем
                        }
                        let who = if l.from_self { "Вы" } else { self.name.as_str() };
                        let color = if l.from_self {
                            egui::Color32::from_rgb(120, 170, 230)
                        } else {
                            egui::Color32::from_rgb(140, 200, 80)
                        };
                        ui.horizontal_wrapped(|ui| {
                            ui.weak(crate::fmt_time(l.time));
                            ui.colored_label(color, format!("{who}:"));
                            for seg in chatfmt::parse(&l.text) {
                                crate::draw_seg(ui, seg, &mut self.anims, who);
                            }
                        });
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
                    self.effect = crate::effects::Effect::new(&kind);
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
                ui.spinner();
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
                            ui.spinner();
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
