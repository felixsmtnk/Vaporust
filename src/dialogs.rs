//! Windows for second-tier actions: friend nickname, points award, new group chat, own game
//! review, new discussion topic. All public actions happen only via a button with an explanation.

use crate::actions::MyReview;
use crate::net::{Cmd, NetState};
use crate::LauncherApp;
use eframe::egui;

#[derive(Default)]
pub struct Dialogs {
    /// Own review: (appid, game name, draft, draft filled from Steam).
    pub review: Option<(u32, String, MyReview, bool)>,
    /// New topic: (appid, game name, title, text).
    pub topic: Option<(u32, String, String, String)>,
    /// Last shown action message (a new one becomes a popup notification).
    pub last_msg: Option<String>,
}

fn window(title: &str) -> egui::Window<'_> {
    egui::Window::new(title)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
}

impl LauncherApp {
    pub(crate) fn open_review(&mut self, appid: u32, name: String) {
        let _ = self.tx.send(Cmd::ReviewLoad(appid));
        self.net.lock().unwrap().action_msg = None;
        self.dialogs.review = Some((appid, name, MyReview { up: true, public: true, ..Default::default() }, false));
    }

    pub(crate) fn open_topic(&mut self, appid: u32, name: String) {
        self.net.lock().unwrap().action_msg = None;
        self.dialogs.topic = Some((appid, name, String::new(), String::new()));
    }

    pub(crate) fn second_plan_dialogs(&mut self, ctx: &egui::Context, net: &NetState) {
        self.drain_ui_req();
        if net.action_msg != self.dialogs.last_msg {
            if let Some(m) = &net.action_msg {
                crate::popup::push(crate::popup::Kind::Info, "Vaporust", m.clone());
            }
            self.dialogs.last_msg = net.action_msg.clone();
        }
        self.cart_pick_dialog(ctx, net);
        // Friend nickname.
        if let Some((id, name, mut nick)) = self.social_ui.nick_edit.take() {
            let mut keep = true;
            window(&trf!("Ник: {name}", "Nickname: {name}")).show(ctx, |ui| {
                ui.weak(tr!("Ваш ник для друга: виден только вам (в Steam и в Vaporust). Пусто - убрать.", "Your nickname for the friend: only you see it (in Steam and in Vaporust). Empty - remove."));
                ui.add(egui::TextEdit::singleline(&mut nick).desired_width(280.0));
                ui.horizontal(|ui| {
                    if ui.button(tr!("Сохранить", "Save")).clicked() {
                        let _ = self.tx.send(Cmd::SetNickname(id, nick.trim().to_string()));
                        keep = false;
                    }
                    if ui.button(tr!("Отмена", "Cancel")).clicked() {
                        keep = false;
                    }
                });
            });
            if keep {
                self.social_ui.nick_edit = Some((id, name, nick));
            }
        }

        // Steam points award.
        if let Some((kind, target, what)) = self.social_ui.award.clone() {
            let mut keep = true;
            if net.award_config.is_none() && !self.award_asked {
                self.award_asked = true;
                let _ = self.tx.send(Cmd::AwardConfig);
                let _ = self.tx.send(Cmd::PointsBalance);
            }
            window(tr!("Наградить", "Award")).show(ctx, |ui| {
                ui.label(trf!("Наградить: {what}", "Award: {what}"));
                ui.weak(tr!("Награда тратит ваши очки Steam; автор получит часть очков. Ее увидят все.", "An award spends your Steam Points; the author gets part of them. Everyone can see it."));
                if let Some(p) = net.points_balance {
                    ui.label(trf!("У вас очков: {}", "Your points: {}", crate::store::group_digits(p)));
                }
                match &net.award_config {
                    None => {
                        crate::spin::spinner(ui);
                    }
                    Some(Err(e)) => {
                        ui.label(trf!("Не удалось загрузить награды: {e}", "Could not load awards: {e}"));
                    }
                    Some(Ok(list)) => {
                        ui.horizontal_wrapped(|ui| {
                            ui.set_max_width(420.0);
                            for (id, cost) in list {
                                let url = format!("https://store.fastly.steamstatic.com/public/images/loyalty/reactions/still/{id}.png");
                                let r = ui
                                    .add(egui::ImageButton::new(egui::Image::new(url).fit_to_exact_size(egui::vec2(40.0, 40.0))))
                                    .on_hover_text(trf!("{cost} очков", "{cost} points"));
                                if r.clicked() {
                                    self.award_pick = Some((*id, *cost));
                                }
                            }
                        });
                    }
                }
                if let Some((id, cost)) = self.award_pick {
                    ui.separator();
                    ui.label(trf!("Выбрана награда за {cost} очков. Отправить?", "Award for {cost} points selected. Send?"));
                    ui.horizontal(|ui| {
                        if ui.button(tr!("Да, наградить", "Yes, award")).clicked() {
                            let _ = self.tx.send(Cmd::Award { target_type: kind, target: target.clone(), reaction: id });
                            self.award_pick = None;
                        }
                        if ui.button(tr!("Нет", "No")).clicked() {
                            self.award_pick = None;
                        }
                    });
                }
                if let Some(m) = &net.action_msg {
                    ui.label(m);
                }
                if ui.button(tr!("Закрыть", "Close")).clicked() {
                    keep = false;
                }
            });
            if !keep {
                self.social_ui.award = None;
                self.award_pick = None;
            }
        }

        // New group chat.
        if let Some((mut name, mut picked)) = self.social_ui.group_create.take() {
            let mut keep = true;
            let mut friends: Vec<(u64, String)> = net
                .roster
                .read()
                .unwrap()
                .values()
                .filter(|f| (f.relationship == 3 && (f.steam_id >> 52) & 0xF == 1))
                .map(|f| (f.steam_id, f.persona_name.clone().unwrap_or_else(|| f.steam_id.to_string())))
                .collect();
            friends.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()));
            window(tr!("Новый групповой чат", "New group chat")).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(tr!("Название", "Name"));
                    ui.add(egui::TextEdit::singleline(&mut name).desired_width(240.0).hint_text(tr!("необязательно", "optional")));
                });
                ui.label(tr!("Пригласить друзей:", "Invite friends:"));
                egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                    for (id, n) in &friends {
                        let mut on = picked.contains(id);
                        if ui.checkbox(&mut on, crate::richtext::fix(ui, n)).changed() {
                            if on {
                                picked.insert(*id);
                            } else {
                                picked.remove(id);
                            }
                        }
                    }
                });
                ui.weak(tr!("Друзьям придут приглашения; чат появится в «Групповых чатах».", "Friends get invitations; the chat appears in \"Group chats\"."));
                ui.horizontal(|ui| {
                    if ui.add_enabled(!picked.is_empty(), egui::Button::new(tr!("Создать", "Create"))).clicked() {
                        let _ = self.tx.send(Cmd::CreateChatGroup { name: name.trim().to_string(), invitees: picked.iter().copied().collect() });
                        keep = false;
                    }
                    if ui.button(tr!("Отмена", "Cancel")).clicked() {
                        keep = false;
                    }
                });
            });
            if keep {
                self.social_ui.group_create = Some((name, picked));
            }
        }

        // Own game review.
        if let Some((appid, game, mut r, mut filled)) = self.dialogs.review.take() {
            let mut keep = true;
            let existing = net.my_reviews.get(&appid);
            if !filled {
                if let Some(Ok(Some(old))) = existing {
                    r = old.clone();
                    filled = true;
                } else if matches!(existing, Some(Ok(None)) | Some(Err(_))) {
                    filled = true;
                }
            }
            window(&trf!("Обзор: {game}", "Review: {game}")).show(ctx, |ui| {
                match existing {
                    None => {
                        ui.horizontal(|ui| {
                            crate::spin::spinner(ui);
                            ui.weak(tr!("Проверяю, писали ли вы уже обзор...", "Checking whether you already reviewed this game..."));
                        });
                    }
                    Some(Ok(Some(_))) => {
                        ui.weak(tr!("У вас уже есть обзор этой игры - сохранение заменит его.", "You already have a review of this game - saving replaces it."));
                    }
                    _ => {}
                }
                ui.horizontal(|ui| {
                    ui.radio_value(&mut r.up, true, tr!("Рекомендую", "Recommended"));
                    ui.radio_value(&mut r.up, false, tr!("Не рекомендую", "Not recommended"));
                });
                ui.add(egui::TextEdit::multiline(&mut r.text).desired_rows(8).desired_width(460.0).hint_text(tr!("Что понравилось, что нет", "What you liked and what you did not")));
                ui.checkbox(&mut r.public, tr!("Виден всем (иначе - только друзьям)", "Visible to everyone (otherwise only to friends)"));
                ui.weak(tr!("Обзор появится на странице игры в магазине и в вашем профиле.", "The review appears on the game's store page and in your profile."));
                ui.horizontal(|ui| {
                    if ui.add_enabled(!r.text.trim().is_empty(), egui::Button::new(tr!("Опубликовать", "Publish"))).clicked() {
                        let _ = self.tx.send(Cmd::ReviewSave { appid, review: MyReview { text: r.text.trim().to_string(), ..r.clone() } });
                    }
                    if ui.button(tr!("Закрыть", "Close")).clicked() {
                        keep = false;
                    }
                });
                if let Some(m) = &net.action_msg {
                    ui.label(m);
                }
            });
            if keep {
                self.dialogs.review = Some((appid, game, r, filled));
            }
        }

        // New topic in the game's discussions.
        if let Some((appid, game, mut title, mut text)) = self.dialogs.topic.take() {
            let mut keep = true;
            window(&trf!("Новая тема: {game}", "New topic: {game}")).show(ctx, |ui| {
                ui.add(egui::TextEdit::singleline(&mut title).desired_width(460.0).hint_text(tr!("Заголовок", "Title")));
                ui.add(egui::TextEdit::multiline(&mut text).desired_rows(8).desired_width(460.0).hint_text(tr!("Текст", "Text")));
                ui.weak(tr!("Тема появится в «Общем обсуждении» игры, ее увидят все.", "The topic appears in the game's \"General Discussions\", everyone can see it."));
                ui.horizontal(|ui| {
                    let ok = !title.trim().is_empty() && !text.trim().is_empty();
                    if ui.add_enabled(ok, egui::Button::new(tr!("Создать тему", "Create topic"))).clicked() {
                        let _ = self.tx.send(Cmd::CreateTopic { appid, title: title.trim().to_string(), text: text.trim().to_string() });
                    }
                    if ui.button(tr!("Закрыть", "Close")).clicked() {
                        keep = false;
                    }
                });
                if let Some(m) = &net.action_msg {
                    ui.label(m);
                }
            });
            if keep {
                self.dialogs.topic = Some((appid, game, title, text));
            }
        }
    }
}

impl LauncherApp {
    /// Requests from store and Workshop pages (award, review, cart).
    pub(crate) fn drain_ui_req(&mut self) {
        for c in std::mem::take(&mut *crate::trade::MARKET_REQ.lock().unwrap()) {
            if let crate::trade::MarketCmd::Sell { appid, hash, .. } = &c {
                let inv = self.net.lock().unwrap().inv.clone();
                let mut s = inv.lock().unwrap();
                s.trade.busy = true;
                s.trade.msg = Some(tr!("Выставляю на продажу...", "Listing for sale...").into());
                s.trade.msg_item = Some((*appid, hash.clone()));
                s.trade.msg_at = Some(std::time::Instant::now());
            }
            let _ = self.tx.send(Cmd::Market(c));
        }
        use crate::storewin::UiReq;
        for r in std::mem::take(&mut *crate::storewin::UI_REQ.lock().unwrap()) {
            match r {
                UiReq::Award(kind, target, what) => {
                    self.net.lock().unwrap().action_msg = None;
                    self.social_ui.award = Some((kind, target, what));
                }
                UiReq::Review(appid, name) => self.open_review(appid, name),
                UiReq::Cart(appid) => {
                    self.net.lock().unwrap().action_msg = None;
                    let _ = self.tx.send(Cmd::AppPackages(appid));
                    self.cart_ui.pick = Some(appid);
                }
            }
        }
    }
}
