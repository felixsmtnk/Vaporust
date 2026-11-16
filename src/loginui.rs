//! The Steam login page: a card in the middle of the main window, as in the Steam client
//! (login, password and Steam Guard code on the left, the QR code on the right). It replaces the
//! library while a login is needed; "Continue without login" leads to the no-login mode.

use crate::net::{Cmd, NetState};
use crate::theme;
use crate::LauncherApp;
use eframe::egui;

/// Card width with two columns; a narrower window gets one column (QR below the form).
const CARD_WIDE: f32 = 700.0;
thread_local! {
    /// A low window: smaller margins, fields and QR code, so the whole card fits without scrolling.
    static COMPACT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// A size for the normal or the compact card.
fn sz(normal: f32, compact: f32) -> f32 {
    if COMPACT.with(|c| c.get()) { compact } else { normal }
}

/// A small blue caption above a field ("LOGIN"), like in the Steam login window.
fn caption(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).size(11.5).strong().color(theme::ACCENT));
}

/// A text field in the program's style: a dark plate without a frame, full width.
fn field(ui: &mut egui::Ui, text: &mut String, password: bool, hint: &str) -> egui::Response {
    let w = ui.available_width();
    let mut out = None;
    egui::Frame::none().fill(theme::CARD).rounding(4.0).inner_margin(egui::Margin::symmetric(10.0, 0.0)).show(ui, |ui| {
        ui.set_width(w - 20.0);
        ui.set_height(sz(34.0, 28.0));
        ui.horizontal_centered(|ui| {
            out = Some(ui.add(egui::TextEdit::singleline(text).frame(false).password(password).hint_text(hint).desired_width(ui.available_width())));
        });
    });
    out.expect("field drawn")
}

impl LauncherApp {
    /// Whether the main window shows the login page instead of the library.
    pub(crate) fn login_screen(&self, net: &NetState) -> bool {
        net.need_login && net.account.is_none() && !self.settings.local_mode && !net.offline
    }

    pub(crate) fn login_page(&mut self, ui: &mut egui::Ui, net: &NetState) {
        let avail = ui.available_size();
        let wide = avail.x >= CARD_WIDE + 40.0;
        let card_w = if wide { CARD_WIDE } else { (avail.x - 24.0).clamp(280.0, 420.0) };
        // The two-column card is about 470 points tall: lower windows get the compact one.
        COMPACT.with(|c| c.set(avail.y < if wide { 520.0 } else { 820.0 }));
        let qr = sz(196.0, 150.0);
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            let card_h = if wide { sz(470.0, 380.0) } else { sz(760.0, 640.0) };
            ui.add_space(((avail.y - card_h) / 2.0).max(sz(16.0, 6.0)));
            theme::centered(ui, card_w, |ui| {
                egui::Frame::none().fill(theme::SIDEBAR).rounding(8.0).inner_margin(egui::Margin::same(sz(28.0, 16.0))).show(ui, |ui| {
                    ui.set_width(card_w - 2.0 * sz(28.0, 16.0));
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(tr!("Вход в Steam", "Sign in to Steam")).size(sz(24.0, 20.0)).strong().color(egui::Color32::WHITE));
                        // Interface language before login: both names in their own language.
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.spacing_mut().item_spacing.x = 2.0;
                            let en = crate::lang::en();
                            for (code, label, on) in [("en", "English", en), ("ru", "Русский", !en)] {
                                let fill = if on { theme::CARD } else { egui::Color32::TRANSPARENT };
                                let color = if on { egui::Color32::WHITE } else { theme::WEAK };
                                let b = egui::Button::new(egui::RichText::new(label).size(12.0).color(color)).fill(fill).rounding(3.0);
                                if ui.add(b).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() && !on {
                                    self.settings.language = code.into();
                                    crate::lang::set_en(code == "en");
                                }
                            }
                        });
                    });
                    ui.label(
                        egui::RichText::new(tr!(
                            "Нужен для списка всех игр, времени в игре, друзей, магазина и загрузок.",
                            "Needed for the list of all games, playtime, friends, the store and downloads."
                        ))
                        .color(theme::WEAK),
                    );
                    ui.add_space(sz(18.0, 8.0));
                    if wide {
                        ui.horizontal_top(|ui| {
                            let col = (card_w - 2.0 * sz(28.0, 16.0) - 40.0 - qr).max(240.0);
                            ui.vertical(|ui| {
                                ui.set_width(col);
                                self.login_form(ui, net);
                            });
                            ui.add_space(24.0);
                            let (sep, _) = ui.allocate_exact_size(egui::vec2(1.0, sz(300.0, 230.0)), egui::Sense::hover());
                            ui.painter().rect_filled(sep, 0.0, theme::CARD);
                            ui.add_space(14.0);
                            ui.vertical(|ui| {
                                ui.set_width(qr);
                                self.qr_block(ui, net);
                            });
                        });
                    } else {
                        self.login_form(ui, net);
                        ui.add_space(sz(18.0, 8.0));
                        self.qr_block(ui, net);
                    }
                    if !net.message.is_empty() {
                        ui.add_space(14.0);
                        ui.label(egui::RichText::new(&net.message).color(egui::Color32::from_rgb(235, 190, 90)));
                    }
                    ui.add_space(sz(16.0, 6.0));
                    ui.label(
                        egui::RichText::new(tr!(
                            "Пароль уходит только на серверы Steam. Vaporust его не хранит: на диске остается только зашифрованный ключ входа.",
                            "The password goes only to Steam servers. Vaporust does not keep it: only an encrypted login key stays on disk."
                        ))
                        .size(11.5)
                        .color(theme::WEAK),
                    );
                });
            });
            ui.add_space(16.0);
        });
    }

    /// Login, password, Steam Guard code, the "Sign in" button and "Continue without login".
    fn login_form(&mut self, ui: &mut egui::Ui, net: &NetState) {
        caption(ui, tr!("ЛОГИН STEAM", "STEAM ACCOUNT NAME"));
        field(ui, &mut self.login_user, false, "");
        ui.add_space(sz(10.0, 4.0));
        caption(ui, tr!("ПАРОЛЬ", "PASSWORD"));
        let pass = field(ui, &mut self.login_pass, true, "");
        ui.add_space(sz(10.0, 4.0));
        caption(ui, tr!("КОД STEAM GUARD", "STEAM GUARD CODE"));
        let code = field(ui, &mut self.login_code, false, tr!("если Steam прислал код на почту или в приложение", "if Steam sent a code by email or in the app"));
        ui.add_space(sz(16.0, 8.0));
        // While waiting for the QR code the user can still sign in with a password (the QR wait stops).
        let waiting_qr = net.qr_url.is_some();
        let can = (!net.busy || waiting_qr) && !self.login_user.trim().is_empty() && !self.login_pass.is_empty();
        let enter = (pass.lost_focus() || code.lost_focus()) && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let w = ui.available_width();
        let b = egui::Button::new(egui::RichText::new(tr!("Войти", "Sign in")).size(15.0).strong().color(egui::Color32::WHITE))
            .fill(theme::ACCENT)
            .rounding(4.0)
            .min_size(egui::vec2(w, sz(38.0, 32.0)));
        let clicked = ui.add_enabled(can, b).clicked();
        if (clicked || (enter && can)) && can {
            let code = self.login_code.trim().to_string();
            let _ = self.tx.send(Cmd::Login {
                user: self.login_user.trim().to_string(),
                pass: std::mem::take(&mut self.login_pass),
                code: (!code.is_empty()).then_some(code),
            });
            self.login_code.clear();
        }
        if net.busy && !waiting_qr {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                crate::spin::spinner(ui);
                ui.label(egui::RichText::new(tr!("Вхожу...", "Signing in...")).color(theme::WEAK));
            });
        }
        ui.add_space(sz(14.0, 6.0));
        let r = ui
            .add(egui::Label::new(egui::RichText::new(tr!("Продолжить без входа", "Continue without login")).color(theme::ACCENT)).sense(egui::Sense::click()))
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(tr!(
                "Только установленные игры и запуск через Steam: пароль Steam вводить не нужно. Войти можно позже.",
                "Installed games only, launched through Steam: no Steam password needed. You can log in later."
            ));
        if r.clicked() {
            self.settings.local_mode = true;
            let mut s = self.net.lock().unwrap();
            s.need_login = false;
            s.qr_url = None;
            s.message.clear();
        }
    }

    /// The QR code column: a placeholder with a button, the code itself, or "scanned".
    fn qr_block(&mut self, ui: &mut egui::Ui, net: &NetState) {
        caption(ui, tr!("ИЛИ ПО QR-КОДУ", "OR WITH A QR CODE"));
        ui.add_space(4.0);
        match &net.qr_url {
            Some(url) => {
                crate::draw_qr(ui, url, sz(196.0, 150.0));
                ui.add_space(6.0);
                let text = if net.qr_scanned {
                    tr!("Код отсканирован. Подтвердите вход в приложении Steam.", "The code is scanned. Confirm the login in the Steam app.")
                } else {
                    tr!("Откройте мобильное приложение Steam и отсканируйте код.", "Open the Steam mobile app and scan the code.")
                };
                ui.label(egui::RichText::new(text).size(12.0).color(theme::WEAK));
                ui.add_space(4.0);
                if ui.button(tr!("Отменить", "Cancel")).clicked() {
                    let _ = self.tx.send(Cmd::CancelQr);
                }
            }
            None => {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(sz(196.0, 150.0), sz(196.0, 150.0)), egui::Sense::hover());
                ui.painter().rect_filled(rect, 6.0, theme::CARD);
                // The button is drawn by hand inside the square (a widget placed with `put` would move
                // the layout and pull the caption below into the square).
                let br = egui::Rect::from_center_size(rect.center(), egui::vec2(sz(196.0, 150.0) - 30.0, 34.0));
                let resp = ui
                    .interact(br, ui.id().with("show_qr"), egui::Sense::click())
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(tr!("Войти без пароля: через мобильное приложение Steam", "Sign in without a password: with the Steam mobile app"));
                let fill = if resp.hovered() { egui::Color32::from_rgb(70, 180, 255) } else { theme::ACCENT };
                ui.painter().rect_filled(br, 4.0, fill);
                ui.painter().text(br.center(), egui::Align2::CENTER_CENTER, tr!("Показать QR-код", "Show QR code"), egui::FontId::proportional(14.0), egui::Color32::WHITE);
                if resp.clicked() && !net.busy {
                    let _ = self.tx.send(Cmd::LoginQr);
                }
                ui.add_space(6.0);
                ui.label(egui::RichText::new(tr!("Вход через мобильное приложение Steam, без пароля.", "Sign in with the Steam mobile app, no password.")).size(12.0).color(theme::WEAK));
            }
        }
    }
}
