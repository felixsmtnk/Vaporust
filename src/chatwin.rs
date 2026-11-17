//! Chat window - a separate Windows window (deferred egui viewport).
//! It repaints on its own without touching the main window: sticker animations and image
//! loading do not force the library to repaint.

use crate::net::{Cmd, NetState, Shared};
use crate::{anim, chatfmt};
use eframe::egui;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc::UnboundedSender;

pub fn viewport_id() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("vaporust_chat")
}

/// State of the open chat.
pub struct ChatWin {
    pub id: u64,
    pub name: String,
    input: String,
    picker_open: bool,
    /// Panel tab: 0 emoticons, 1 stickers, 2 effects.
    picker_tab: u8,
    /// Broken effect icons have already been reloaded once.
    icons_retried: bool,
    /// When we last asked for the list of own effects.
    effects_asked: Option<std::time::Instant>,
    emoticons_requested: bool,
    /// Animated sticker frames (live while the chat is open).
    anims: anim::Cache,
    /// Messages already parsed from BBCode (by text): parsing every line on every frame is a waste.
    parsed: std::collections::HashMap<String, Vec<chatfmt::Seg>>,
    /// Whether V was pressed in the previous frame (for Ctrl+V with an image).
    v_was_down: bool,
    /// Put the cursor into the input field (right after opening).
    focus_input: bool,
    /// Running room effect and time of the last one shown (to avoid repeats).
    effect: Option<crate::effects::Effect>,
    effect_seen: u64,
    /// Search in the conversation (empty - show everything).
    search: String,
    /// The search field is open (magnifier or Ctrl+F) and needs the cursor.
    search_open: bool,
    focus_search: bool,
    /// Copy of the network state and the change number it was taken at (do not copy every frame).
    snap: Option<NetState>,
    snap_gen: u64,
    /// Open conversations (tabs): friend and name; the current one - id.
    tabs: Vec<(u64, String)>,
    /// Unsent text of other tabs.
    drafts: std::collections::HashMap<u64, String>,
    /// Reply to a message: (author, text without formatting).
    reply: Option<(String, String)>,
}

impl ChatWin {
    /// Switch to a conversation (a new one is added as a tab).
    pub fn switch_to(&mut self, id: u64, name: String) {
        if !self.tabs.iter().any(|(t, _)| *t == id) {
            self.tabs.push((id, name.clone()));
        }
        if self.id != id {
            let old = std::mem::take(&mut self.input);
            self.drafts.insert(self.id, old);
            self.input = self.drafts.remove(&id).unwrap_or_default();
            self.id = id;
            self.name = name;
            self.reply = None;
            self.search.clear();
            self.search_open = false;
            self.focus_input = true;
        }
    }

    pub fn new(id: u64, name: String) -> Self {
        Self {
            tabs: vec![(id, name.clone())],
            drafts: Default::default(),
            reply: None,
            id,
            name,
            input: String::new(),
            picker_open: false,
            picker_tab: 0,
            icons_retried: false,
            effects_asked: None,
            emoticons_requested: false,
            anims: Default::default(),
            parsed: Default::default(),
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

/// Called from the main window's frame while the chat is open.
pub fn show(ctx: &egui::Context, chat: &SharedChat, net: &Shared, tx: &UnboundedSender<Cmd>) {
    let title = match chat.lock().unwrap().as_ref() {
        Some(c) => trf!("Чат: {}", "Chat: {}", c.name),
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
            // Network state snapshot: copied again only when it changed.
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
                // Large chat images are no longer needed (avatars stay for the main window).
                crate::imgload::forget_large(ctx);
                ctx.request_repaint_of(egui::ViewportId::ROOT);
            }
        },
    );
}

/// Images from non-Steam sites the user asked to show (this run only).
static REVEALED: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// A link to a non-Steam site waiting for "Open?" confirmation.
static PENDING_LINK: Mutex<Option<String>> = Mutex::new(None);

pub fn image_revealed(url: &str) -> bool {
    REVEALED.lock().map(|v| v.iter().any(|u| u == url)).unwrap_or(false)
}

pub fn reveal_image(url: &str) {
    if let Ok(mut v) = REVEALED.lock() {
        v.push(url.to_string());
    }
}

/// Open a link from a message: Steam sites at once, others after a warning
/// (a fake "Steam" link is the most common way accounts are stolen).
pub fn open_link(ctx: &egui::Context, url: &str) {
    let host = chatfmt::host_of(url);
    if host.is_empty() {
        return; // not http(s): file://, steam://... from a message are not opened
    }
    if url.trim().to_ascii_lowercase().starts_with("https://") && chatfmt::is_steam_host(&host) {
        ctx.open_url(egui::OpenUrl::new_tab(url));
    } else if let Ok(mut p) = PENDING_LINK.lock() {
        *p = Some(url.to_string());
    }
}

/// "You are leaving Steam" window over the chat.
pub(crate) fn link_warning(ctx: &egui::Context) {
    let Some(url) = PENDING_LINK.lock().ok().and_then(|p| p.clone()) else { return };
    let host = chatfmt::host_of(&url);
    let mut close = false;
    egui::Window::new(tr!("Ссылка на другой сайт", "Link to another site"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.set_max_width(380.0);
            ui.label(tr!(
                "Ссылка ведет не на сайт Steam. Не вводите на нем логин и пароль Steam.",
                "The link does not lead to a Steam site. Do not enter your Steam login and password there."
            ));
            ui.add_space(4.0);
            ui.label(egui::RichText::new(&host).strong().size(16.0));
            ui.add(egui::Label::new(egui::RichText::new(&url).small().weak()).wrap());
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button(tr!("Открыть", "Open")).clicked() {
                    ctx.open_url(egui::OpenUrl::new_tab(&url));
                    close = true;
                }
                if ui.button(tr!("Отмена", "Cancel")).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    close = true;
                }
            });
        });
    if close {
        if let Ok(mut p) = PENDING_LINK.lock() {
            *p = None;
        }
    }
}

impl ChatWin {
    fn frame(&mut self, ctx: &egui::Context, net: &NetState, shared: &Shared, tx: &UnboundedSender<Cmd>) {
        link_warning(ctx);
        let id = self.id;
        // For notifications: no sound if the chat with this very friend is active.
        crate::notify::set_open_chat(id);
        // The window is open and active - so everything is read.
        if ctx.input(|i| i.focused) && net.chats.get(&id).is_some_and(|c| c.unread > 0) {
            if let Some(c) = std::sync::Arc::make_mut(&mut shared.lock().unwrap().chats).get_mut(&id) {
                c.unread = 0;
            }
            ctx.request_repaint_of(egui::ViewportId::ROOT);
        }
        let empty = crate::net::Conversation::default();
        // A reference, not a copy: the whole conversation was copied on every frame.
        let conv = net.chats.get(&id).unwrap_or(&empty);
        let mut send = None;
        let mut send_image: Option<std::path::PathBuf> = None;

        // Bottom part: status, emoticon panel, input field.
        egui::TopBottomPanel::bottom("chat_input")
            .frame(egui::Frame::none().fill(crate::theme::TOP).inner_margin(egui::Margin::symmetric(10.0, 8.0)))
            .show(ctx, |ui| {
            if conv
                .typing_at
                .is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(6))
            {
                ui.weak(trf!("{} печатает...", "{} is typing...", self.name));
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
            // "Effects" tab open - reread own effects (may have been bought), at most once per 20 s.
            if self.picker_open
                && self.picker_tab == 2
                && self.effects_asked.is_none_or(|t| t.elapsed() > std::time::Duration::from_secs(20))
            {
                self.effects_asked = Some(std::time::Instant::now());
                let _ = tx.send(Cmd::LoadChatEffects);
            }
            if self.picker_open {
                egui::Frame::none().fill(crate::theme::SIDEBAR).rounding(6.0).inner_margin(egui::Margin::same(8.0)).show(ui, |ui| {
                    if let Some(msg) = self.picker(ui, net) {
                        send = Some(msg);
                    }
                });
                ui.add_space(6.0);
            }
            // Reply to a message: above the input field - who we reply to (sent as a quote).
            if let Some((who, text)) = self.reply.clone() {
                ui.horizontal(|ui| {
                    let (bar, _) = ui.allocate_exact_size(egui::vec2(3.0, 18.0), egui::Sense::hover());
                    ui.painter().rect_filled(bar, 1.0, crate::theme::ACCENT);
                    ui.label(egui::RichText::new(trf!("Ответ {who}:", "Reply to {who}:")).color(crate::theme::WEAK));
                    let short: String = text.chars().take(60).collect();
                    ui.add(egui::Label::new(egui::RichText::new(short).italics()).truncate());
                    if icon_button(ui, egui::vec2(22.0, 22.0), false, tr!("Не отвечать", "Cancel reply"), draw_cross).clicked() {
                        self.reply = None;
                    }
                });
                ui.add_space(4.0);
            }
            ui.horizontal(|ui| {
                const H: f32 = 34.0;
                let send_w = H + 6.0;
                // The field with the emoticon and picture buttons inside, as in Steam.
                let field_w = ui.available_width() - send_w;
                let mut enter = false;
                let mut edit_resp = None;
                egui::Frame::none()
                    .fill(crate::theme::CARD)
                    .rounding(6.0)
                    .inner_margin(egui::Margin { left: 10.0, right: 4.0, top: 0.0, bottom: 0.0 })
                    .show(ui, |ui| {
                        ui.set_width(field_w - 14.0);
                        ui.set_height(H);
                        ui.horizontal_centered(|ui| {
                            let edit = ui.add(
                                egui::TextEdit::singleline(&mut self.input)
                                    .frame(false)
                                    .hint_text(tr!("Сообщение, Enter - отправить", "Message, Enter - send"))
                                    .desired_width(ui.available_width() - 64.0),
                            );
                            if std::mem::take(&mut self.focus_input) {
                                edit.request_focus();
                            }
                            enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.spacing_mut().item_spacing.x = 2.0;
                                let pic = icon_button(
                                    ui,
                                    egui::vec2(28.0, 28.0),
                                    false,
                                    tr!("Отправить картинку (или перетащите файл в окно, или Ctrl+V)", "Send an image (or drag a file into the window, or Ctrl+V)"),
                                    draw_picture,
                                );
                                if pic.clicked() {
                                    if let Some(path) = rfd::FileDialog::new()
                                        .set_title(tr!("Картинка для отправки", "Image to send"))
                                        .add_filter(tr!("Картинки", "Images"), &["png", "jpg", "jpeg", "gif", "webp", "bmp"])
                                        .pick_file()
                                    {
                                        send_image = Some(path);
                                    }
                                }
                                let smile = icon_button(
                                    ui,
                                    egui::vec2(28.0, 28.0),
                                    self.picker_open,
                                    tr!("Смайлики, стикеры и эффекты", "Emoticons, stickers and effects"),
                                    draw_smiley,
                                );
                                if smile.clicked() {
                                    self.picker_open = !self.picker_open;
                                    if self.picker_open && net.emoticons.is_none() && !self.emoticons_requested {
                                        self.emoticons_requested = true;
                                        let _ = tx.send(Cmd::LoadEmoticons);
                                    }
                                }
                            });
                            edit_resp = Some(edit);
                        });
                    });
                let has_text = !self.input.trim().is_empty();
                let (sr, sresp) = ui.allocate_exact_size(egui::vec2(H, H), egui::Sense::click());
                let sresp = sresp.on_hover_text(tr!("Отправить (Enter)", "Send (Enter)"));
                let fill = match (has_text, sresp.hovered()) {
                    (true, true) => egui::Color32::from_rgb(70, 180, 255),
                    (true, false) => crate::theme::ACCENT,
                    _ => crate::theme::CARD,
                };
                ui.painter().rect_filled(sr, 6.0, fill);
                draw_send(ui.painter(), sr, if has_text { egui::Color32::WHITE } else { crate::theme::WEAK });
                if has_text {
                    sresp.clone().on_hover_cursor(egui::CursorIcon::PointingHand);
                }
                let clicked = sresp.clicked();
                let edit = edit_resp.expect("input field drawn");
                if (enter || clicked) && !self.input.trim().is_empty() {
                    let text = std::mem::take(&mut self.input).trim().to_string();
                    let mut msg = crate::emoticons_to_bbcode(&text, net);
                    // A reply is a quote with the author before the text (Steam shows it as a block).
                    if let Some((who, quoted)) = self.reply.take() {
                        let q: String = quoted.chars().take(300).collect();
                        // The name in quotes: otherwise a space cuts it ("John Doe" -> "John"); quotes
                        // and brackets in the name itself would break the tag.
                        let who: String = who.chars().filter(|c| !matches!(c, '"' | '[' | ']')).collect();
                        msg = format!("[quote author=\"{who}\"]{q}[/quote]\n{msg}");
                    }
                    send = Some(msg);
                    edit.request_focus();
                } else if enter {
                    edit.request_focus();
                }
            });
        });

        // Header, as in Steam: friend's avatar, name and status; on the right - magnifier (search, Ctrl+F).
        let friend = net.roster.read().unwrap().get(&id).cloned();
        let friend_avatar = friend
            .as_ref()
            .and_then(|f| f.avatar_hash.as_ref())
            .map(|h| format!("https://avatars.fastly.steamstatic.com/{h}_medium.jpg"));
        let my_avatar = net.my_id.and_then(|m| net.profiles.get(&m)).and_then(|p| p.as_ref().ok()).and_then(|p| p.avatar_url.clone());
        let (status, status_color) = match friend.as_ref() {
            Some(f) if f.game_name.as_ref().is_some_and(|g| !g.is_empty()) => {
                (trf!("В игре: {}", "In game: {}", f.game_name.clone().unwrap_or_default()), egui::Color32::from_rgb(140, 200, 80))
            }
            Some(f) if f.persona_state.unwrap_or(0) == 0 => (tr!("не в сети", "offline").to_string(), egui::Color32::GRAY),
            Some(f) if matches!(f.persona_state, Some(3 | 4)) => (tr!("отошел", "away").to_string(), egui::Color32::from_rgb(110, 140, 170)),
            Some(_) => (tr!("в сети", "online").to_string(), egui::Color32::from_rgb(90, 170, 230)),
            None => (String::new(), egui::Color32::GRAY),
        };
        // Ctrl+F - search, Esc - close search.
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::F)) {
            self.search_open = true;
            self.focus_search = true;
        }
        if self.search_open && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.search_open = false;
            self.search.clear();
            self.focus_input = true;
        }
        // Conversation tabs (if more than one is open): name, unread, close cross.
        let mut switch: Option<(u64, String)> = None;
        let mut close_tab: Option<u64> = None;
        if self.tabs.len() > 1 {
            egui::TopBottomPanel::top("chat_tabs").frame(egui::Frame::none().fill(crate::theme::TOP).inner_margin(egui::Margin::symmetric(4.0, 3.0))).show(ctx, |ui| {
                egui::ScrollArea::horizontal().id_salt("chat_tabs_scroll").show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for (tid, tname) in &self.tabs {
                            let unread = net.chats.get(tid).map_or(0, |c| c.unread);
                            let label = if unread > 0 && *tid != id { format!("{tname} ({unread})") } else { tname.clone() };
                            let mut text = egui::RichText::new(crate::richtext::fix(ui, &label));
                            if unread > 0 && *tid != id {
                                text = text.color(egui::Color32::from_rgb(240, 200, 80));
                            }
                            if ui.selectable_label(*tid == id, text).clicked() && *tid != id {
                                switch = Some((*tid, tname.clone()));
                            }
                            if icon_button(ui, egui::vec2(18.0, 18.0), false, tr!("Закрыть беседу", "Close conversation"), draw_cross).clicked() {
                                close_tab = Some(*tid);
                            }
                            ui.add_space(6.0);
                        }
                    });
                });
            });
        }
        if let Some((t, n)) = switch {
            self.switch_to(t, n);
        }
        if let Some(t) = close_tab {
            self.tabs.retain(|(x, _)| *x != t);
            self.drafts.remove(&t);
            if t == self.id {
                if let Some((n, name)) = self.tabs.first().cloned() {
                    // The current one was closed - go to the first remaining (the closed draft is not kept).
                    self.input.clear();
                    self.switch_to(n, name);
                    // switch_to kept the (empty) text of the closed tab as its draft - not needed.
                    self.drafts.remove(&t);
                }
            }
        }
        egui::TopBottomPanel::top("chat_head")
            .frame(egui::Frame::none().fill(crate::theme::TOP).inner_margin(egui::Margin::symmetric(10.0, 8.0)))
            .show(ctx, |ui| {
            ui.horizontal(|ui| {
                let (ar, _) = ui.allocate_exact_size(egui::vec2(38.0, 38.0), egui::Sense::hover());
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
                    let r = icon_button(
                        ui,
                        egui::vec2(32.0, 30.0),
                        self.search_open,
                        tr!("Поиск в переписке (Ctrl+F)", "Search the conversation (Ctrl+F)"),
                        |p, r, c| draw_magnifier(p, r, c),
                    );
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
            // Search: closed by the magnifier or Esc, so no separate close button.
            if self.search_open {
                ui.add_space(6.0);
                egui::Frame::none().fill(crate::theme::CARD).rounding(6.0).inner_margin(egui::Margin::symmetric(8.0, 5.0)).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let (ir, _) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
                        draw_magnifier(ui.painter(), ir.translate(egui::vec2(-1.0, -1.0)), crate::theme::WEAK);
                        let edit = ui.add(
                            egui::TextEdit::singleline(&mut self.search)
                                .frame(false)
                                .hint_text(tr!("Поиск в переписке, Esc - закрыть", "Search the conversation, Esc - close"))
                                .desired_width(ui.available_width()),
                        );
                        if std::mem::take(&mut self.focus_search) {
                            edit.request_focus();
                        }
                    });
                });
            }
        });
        let query = self.search.trim().to_lowercase();
        // Conversation: messages grouped by sender (avatar, name, time), a separator between days.
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .stick_to_bottom(true)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if !conv.history_loaded && conv.error.is_none() {
                        ui.horizontal(|ui| {
                            crate::spin::spinner(ui);
                            ui.weak(tr!("Загружаю переписку...", "Loading the conversation..."));
                        });
                    }
                    if conv.history_loaded && conv.lines.is_empty() {
                        ui.add_space(20.0);
                        ui.vertical_centered(|ui| ui.weak(tr!("Переписки пока нет. Напишите первым.", "No messages yet. Write first.")));
                    }
                    if conv.more_history && ui.add_enabled(!conv.loading_older, egui::Button::new(tr!("Показать более ранние сообщения", "Show earlier messages"))).clicked() {
                        let _ = tx.send(Cmd::OlderChat(id));
                    }
                    if !query.is_empty() {
                        let n = conv.lines.iter().filter(|l| l.text.to_lowercase().contains(&query)).count();
                        ui.weak(trf!("Найдено: {n} (среди загруженных {} сообщений)", "Found: {n} (among {} loaded messages)", conv.lines.len()));
                    }
                    let mut last_day = String::new();
                    let mut last: Option<(bool, u64)> = None;
                    for l in &conv.lines {
                        if !query.is_empty() && !l.text.to_lowercase().contains(&query) {
                            continue;
                        }
                        if l.text.trim().is_empty() {
                            continue; // empty ones (for example an effect the sender does not have) - not shown, as in Steam
                        }
                        let day = crate::fmt_day(l.time);
                        if day != last_day {
                            last_day = day.clone();
                            last = None;
                            ui.add_space(6.0);
                            ui.vertical_centered(|ui| ui.label(egui::RichText::new(&day).size(11.5).color(crate::theme::WEAK)));
                            ui.add_space(2.0);
                        }
                        let who = if l.from_self { tr!("Вы", "You") } else { self.name.as_str() };
                        // New group: another sender or a break longer than 5 minutes.
                        let new_group = last.is_none_or(|(s, t)| s != l.from_self || l.time.saturating_sub(t) > 300);
                        last = Some((l.from_self, l.time));
                        let avatar = if l.from_self { my_avatar.as_ref() } else { friend_avatar.as_ref() };
                        let color = if l.from_self { egui::Color32::from_rgb(120, 170, 230) } else { egui::Color32::from_rgb(140, 200, 80) };
                        // Message right-click menu: register the area from the previous frame BEFORE
                        // the contents so that links and spoilers inside are still clickable.
                        let msg_key = egui::Id::new(("chat_msg", id, l.time, l.ordinal, l.text.len()));
                        if let Some(rect) = ui.data(|d| d.get_temp::<egui::Rect>(msg_key)) {
                            let r = ui.interact(rect, msg_key, egui::Sense::click());
                            r.context_menu(|ui| {
                                if ui.button(tr!("Ответить", "Reply")).clicked() {
                                    self.reply = Some((who.to_string(), chatfmt::preview(&l.text).0));
                                    self.focus_input = true;
                                    ui.close_menu();
                                }
                                if ui.button(tr!("Копировать текст", "Copy text")).clicked() {
                                    ui.ctx().copy_text(chatfmt::copy_text(&l.text));
                                    ui.close_menu();
                                }
                                // Reactions: own Steam emoticons (needs the message number - history messages have it).
                                let mine = net.emoticons.as_ref().map(|e| e.0.clone()).unwrap_or_default();
                                if l.ordinal > 0 || l.time > 0 {
                                    ui.menu_button(tr!("Реакция", "Reaction"), |ui| {
                                        if mine.is_empty() {
                                            ui.weak(tr!("Нет своих смайликов Steam", "You have no Steam emoticons"));
                                            if net.emoticons.is_none() && !self.emoticons_requested {
                                                self.emoticons_requested = true;
                                                let _ = tx.send(Cmd::LoadEmoticons);
                                            }
                                        }
                                        ui.horizontal_wrapped(|ui| {
                                            ui.set_max_width(260.0);
                                            for e in mine.iter().take(60) {
                                                let url = format!("https://community.fastly.steamstatic.com/economy/emoticonlarge/{e}");
                                                if ui.add(egui::ImageButton::new(egui::Image::new(url).fit_to_exact_size(egui::vec2(22.0, 22.0)))).on_hover_text(e).clicked() {
                                                    let _ = tx.send(Cmd::ChatReaction { friend: id, time: l.time as u32, ordinal: l.ordinal, kind: 1, name: e.clone(), add: true });
                                                    ui.close_menu();
                                                }
                                            }
                                        });
                                    });
                                }
                            });
                        }
                        let msg_resp = ui.horizontal_top(|ui| {
                            // Message text does not capture the right click (message menu); copy via the menu.
                            ui.style_mut().interaction.selectable_labels = false;
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
                                    if self.parsed.len() > 3000 {
                                        self.parsed.clear();
                                    }
                                    let segs = self.parsed.entry(l.text.clone()).or_insert_with(|| chatfmt::parse(&l.text)).clone();
                                    for seg in segs {
                                        crate::draw_seg(ui, seg, &mut self.anims, who);
                                    }
                                });
                                // Reactions under a message: icon and count; click - add/remove own.
                                if !l.reactions.is_empty() {
                                    let me = net.my_id.map(|m| (m & 0xFFFF_FFFF) as u32).unwrap_or(0);
                                    ui.horizontal_wrapped(|ui| {
                                        for (kind, name, who_ids) in &l.reactions {
                                            let mine = who_ids.contains(&me);
                                            let url = if *kind == 2 {
                                                format!("https://community.fastly.steamstatic.com/economy/sticker/{}", name.replace(' ', "%20"))
                                            } else {
                                                format!("https://community.fastly.steamstatic.com/economy/emoticonlarge/{name}")
                                            };
                                            let fill = if mine { egui::Color32::from_rgb(40, 70, 100) } else { crate::theme::CARD };
                                            let r = egui::Frame::none().fill(fill).rounding(8.0).inner_margin(egui::Margin::symmetric(5.0, 1.0)).show(ui, |ui| {
                                                ui.horizontal(|ui| {
                                                    ui.add(egui::Image::new(url).fit_to_exact_size(egui::vec2(16.0, 16.0)));
                                                    ui.label(egui::RichText::new(who_ids.len().to_string()).size(11.5));
                                                });
                                            });
                                            let r = r.response.interact(egui::Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(if mine {
                                                trf!("{name}: ваша реакция, щелчок - убрать", "{name}: your reaction, click to remove")
                                            } else {
                                                trf!("{name}: щелчок - поставить такую же", "{name}: click to add the same")
                                            });
                                            if r.clicked() {
                                                let _ = tx.send(Cmd::ChatReaction {
                                                    friend: id,
                                                    time: l.time as u32,
                                                    ordinal: l.ordinal,
                                                    kind: *kind,
                                                    name: name.clone(),
                                                    add: !mine,
                                                });
                                            }
                                        }
                                    });
                                }
                            });
                        });
                        ui.data_mut(|d| d.insert_temp(msg_key, msg_resp.response.rect));
                        if new_group {
                            ui.add_space(2.0);
                        }
                    }
                });
        });

        // Ctrl+V with an image in the clipboard. egui reports a paste only for text,
        // so the V key is checked via Windows (the frame repaints on every key press).
        let v_down = crate::key_v_down();
        if v_down && !self.v_was_down && ctx.input(|i| i.modifiers.command && i.focused) {
            match crate::clipboard_image_file() {
                Ok(Some(path)) => send_image = Some(path),
                Ok(None) => {} // the clipboard has text - the input field pastes it itself
                Err(e) => {
                    if let Some(c) = std::sync::Arc::make_mut(&mut shared.lock().unwrap().chats).get_mut(&id) {
                        c.error = Some(e);
                    }
                }
            }
        }
        self.v_was_down = v_down;
        // A file dragged into the chat window is sent as an image.
        if let Some(path) = ctx.input(|i| i.raw.dropped_files.iter().find_map(|f| f.path.clone())) {
            send_image = Some(path);
        }
        // Room effect: a fresh one (no older than 20 s) from the conversation or "show again".
        let now = crate::net::now_unix();
        if let Some(l) = conv.lines.iter().rev().take(5).find(|l| l.text.contains("[roomeffect")) {
            if l.time > self.effect_seen && now.saturating_sub(l.time) <= 20 {
                self.effect_seen = l.time;
                if let Some(chatfmt::Seg::RoomEffect(kind)) =
                    chatfmt::parse(&l.text).into_iter().find(|s| matches!(s, chatfmt::Seg::RoomEffect(_)))
                {
                    // Without animations (setting) the effect does not start by itself; "Replay effect" works.
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

    /// Emoticon and sticker panel. An emoticon is inserted into the input as :name:,
    /// a sticker is sent right away (returns the message text).
    fn picker(&mut self, ui: &mut egui::Ui, net: &NetState) -> Option<String> {
        let mut send = None;
        // Tabs as in the program's top bar: text, the chosen one in white with an accent underline.
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 16.0;
            for (i, label) in [tr!("Смайлики", "Emoticons"), tr!("Стикеры", "Stickers"), tr!("Эффекты", "Effects")].into_iter().enumerate() {
                let on = self.picker_tab == i as u8;
                let text = egui::RichText::new(label.to_uppercase()).size(12.0).strong().color(if on { egui::Color32::WHITE } else { crate::theme::WEAK });
                let r = ui.add(egui::Label::new(text).sense(egui::Sense::click())).on_hover_cursor(egui::CursorIcon::PointingHand);
                if on {
                    let y = r.rect.bottom() + 3.0;
                    ui.painter().line_segment([egui::pos2(r.rect.left(), y), egui::pos2(r.rect.right(), y)], egui::Stroke::new(2.0, crate::theme::ACCENT));
                }
                if r.clicked() {
                    self.picker_tab = i as u8;
                }
            }
        });
        ui.add_space(8.0);
        let Some((emoticons, stickers)) = &net.emoticons else {
            ui.horizontal(|ui| {
                crate::spin::spinner(ui);
                ui.weak(tr!("Загружаю ваши смайлики и стикеры...", "Loading your emoticons and stickers..."));
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
                        // Only purchased effects (from the Steam inventory; reread when the tab is opened).
                        let Some(list) = net.chat_effects.clone() else {
                            crate::spin::spinner(ui);
                            ui.weak(tr!("Загружаю ваши эффекты...", "Loading your effects..."));
                            return;
                        };
                        if list.is_empty() {
                            ui.vertical(|ui| {
                                ui.weak(tr!("У вас нет эффектов комнаты. Их получают за очки в магазине очков Steam.", "You have no room effects. They are bought for points in the Steam Points Shop."));
                                ui.hyperlink_to(tr!("Магазин очков Steam", "Steam Points Shop"), "https://store.steampowered.com/points/shop/");
                            });
                        }
                        // Icon failed to load (network error) - forget the error once so egui tries again.
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
                            let tip = trf!("{} - отправить", "{} - send", crate::effects::short_name(name));
                            if picker_tile(ui, crate::effects::icon_url(name), 48.0, &tip).clicked()
                            {
                                // As in the Steam client: the /roomeffect command, the server turns it into [roomeffect].
                                send = Some(format!("/roomeffect {name}"));
                                self.picker_open = false;
                            }
                        }
                    } else if self.picker_tab == 0 {
                        if emoticons.is_empty() {
                            ui.weak(tr!("У вас нет смайликов Steam.", "You have no Steam emoticons."));
                        }
                        for name in emoticons {
                            if picker_tile(ui, chatfmt::emoticon_url(name), 30.0, &format!(":{name}:")).clicked()
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
                            ui.weak(tr!("У вас нет стикеров (или инвентарь Steam недоступен).", "You have no stickers (or the Steam inventory is unavailable)."));
                        }
                        for name in stickers {
                            if picker_tile(ui, chatfmt::sticker_url(name), 72.0, &trf!("{name} - отправить", "{name} - send")).clicked()
                            {
                                // A sticker is sent with the /sticker command: the server turns it into [sticker type=...].
                                // The server strips a ready-made [sticker] tag (the recipient sees an empty message).
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
    /// Live test (read only): loading older messages. Prints numbers only.
    #[test]
    #[ignore]
    fn older_history_live() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let friends = c.collect_friends(std::time::Duration::from_secs(5)).await.unwrap();
            for f in friends.iter().filter(|f| (f.relationship == 3 && (f.steam_id >> 52) & 0xF == 1)) {
                let (first, more) = c.chat_history(f.steam_id, 50, None).await.unwrap();
                if first.len() < 50 || !more {
                    continue;
                }
                let oldest = first.iter().min_by_key(|m| (m.timestamp, m.ordinal)).unwrap();
                let (older, more2) = c.chat_history(f.steam_id, 50, Some((oldest.timestamp, oldest.ordinal))).await.unwrap();
                let newest_older = older.iter().map(|m| (m.timestamp, m.ordinal)).max();
                println!("first batch {}, more {}, next {} (all older: {}), more {}", first.len(), more, older.len(), newest_older.is_some_and(|n| n < (oldest.timestamp, oldest.ordinal)), more2);
                assert!(!older.is_empty());
                return;
            }
            println!("no conversation longer than 50 messages");
        });
    }
}

/// Magnifier icon (drawn with lines, no font).
fn draw_magnifier(p: &egui::Painter, r: egui::Rect, color: egui::Color32) {
    let c = r.center() + egui::vec2(-2.0, -2.0);
    let s = egui::Stroke::new(1.6, color);
    p.circle_stroke(c, 5.0, s);
    p.line_segment([c + egui::vec2(3.6, 3.6), c + egui::vec2(8.0, 8.0)], egui::Stroke::new(2.0, color));
}

/// Flat icon button of the chat: no frame, a soft background on hover, accent background when active.
fn icon_button(
    ui: &mut egui::Ui,
    size: egui::Vec2,
    active: bool,
    tip: &str,
    draw: impl FnOnce(&egui::Painter, egui::Rect, egui::Color32),
) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(tip);
    let p = ui.painter();
    if active {
        p.rect_filled(r, 4.0, egui::Color32::from_rgb(38, 72, 104));
    } else if resp.hovered() {
        p.rect_filled(r, 4.0, egui::Color32::from_rgb(52, 60, 72));
    }
    let color = if active || resp.hovered() { egui::Color32::WHITE } else { crate::theme::WEAK };
    draw(p, r, color);
    resp
}

/// Smiley icon: a circle, two eyes and a smile.
fn draw_smiley(p: &egui::Painter, r: egui::Rect, color: egui::Color32) {
    let c = r.center();
    let s = egui::Stroke::new(1.5, color);
    p.circle_stroke(c, 7.5, s);
    p.circle_filled(c + egui::vec2(-2.6, -2.0), 1.1, color);
    p.circle_filled(c + egui::vec2(2.6, -2.0), 1.1, color);
    let smile: Vec<egui::Pos2> = (0..=8)
        .map(|i| {
            let a = std::f32::consts::PI * (0.15 + 0.7 * i as f32 / 8.0);
            c + egui::vec2(a.cos() * 4.2, a.sin() * 4.2)
        })
        .collect();
    p.add(egui::Shape::line(smile, s));
}

/// Picture icon: a frame with a sun and a mountain.
fn draw_picture(p: &egui::Painter, r: egui::Rect, color: egui::Color32) {
    let b = egui::Rect::from_center_size(r.center(), egui::vec2(16.0, 13.0));
    let s = egui::Stroke::new(1.5, color);
    p.rect_stroke(b, 2.0, s);
    p.circle_filled(b.left_top() + egui::vec2(5.0, 4.5), 1.6, color);
    p.add(egui::Shape::line(
        vec![
            b.left_bottom() + egui::vec2(2.0, -2.0),
            b.left_bottom() + egui::vec2(6.5, -6.5),
            b.left_bottom() + egui::vec2(9.5, -3.5),
            b.left_bottom() + egui::vec2(11.5, -5.5),
            b.left_bottom() + egui::vec2(14.0, -2.0),
        ],
        s,
    ));
}

/// Send icon: a paper plane pointing right.
fn draw_send(p: &egui::Painter, r: egui::Rect, color: egui::Color32) {
    let c = r.center();
    let pts = vec![c + egui::vec2(-7.0, -6.5), c + egui::vec2(8.0, 0.0), c + egui::vec2(-7.0, 6.5), c + egui::vec2(-4.0, 0.0)];
    p.add(egui::Shape::convex_polygon(pts, color, egui::Stroke::NONE));
}

/// Cross icon (close, cancel).
fn draw_cross(p: &egui::Painter, r: egui::Rect, color: egui::Color32) {
    let c = r.center();
    let s = egui::Stroke::new(1.5, color);
    let d = 3.5;
    p.line_segment([c + egui::vec2(-d, -d), c + egui::vec2(d, d)], s);
    p.line_segment([c + egui::vec2(-d, d), c + egui::vec2(d, -d)], s);
}

/// A picker tile: the picture with a soft background on hover, no button frame.
fn picker_tile(ui: &mut egui::Ui, url: String, size: f32, tip: &str) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(egui::vec2(size + 8.0, size + 8.0), egui::Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(tip);
    let fill = if resp.hovered() { egui::Color32::from_rgb(58, 68, 82) } else { crate::theme::CARD };
    ui.painter().rect_filled(r, 5.0, fill);
    // The chat window redraws only on input: while the picture is still loading, look again soon.
    if matches!(ui.ctx().try_load_image(&url, Default::default()), Ok(egui::load::ImagePoll::Pending { .. })) {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(300));
    }
    egui::Image::new(url).fit_to_exact_size(egui::vec2(size, size)).paint_at(ui, egui::Rect::from_center_size(r.center(), egui::vec2(size, size)));
    resp
}
