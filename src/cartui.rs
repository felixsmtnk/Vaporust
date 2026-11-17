//! The Steam store cart inside the program: what is in it, removing, total. Adding a game -
//! the "Add to cart" button on the game page. Payment only on the Steam site (Vaporust never
//! enters payment details): the "Check out on the Steam site" button.

use crate::net::{Cmd, NetState};
use crate::LauncherApp;
use eframe::egui;

#[derive(Default)]
pub struct CartUi {
    /// Choosing a purchase option for "Add to cart" (appid).
    pub pick: Option<u32>,
    pub asked: bool,
    /// Package names already requested from the store.
    pub names_asked: std::collections::HashSet<u32>,
}

impl LauncherApp {
    pub(crate) fn open_cart(&mut self) {
        self.close_extras(crate::View::Cart);
        self.view = crate::View::Cart;
        self.cart_ui.asked = false;
    }

    pub(crate) fn cart_contents(&mut self, ui: &mut egui::Ui, net: &NetState) {
        if !self.cart_ui.asked {
            self.cart_ui.asked = true;
            let _ = self.tx.send(Cmd::CartLoad);
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(tr!("Корзина", "Cart")).font(crate::theme::semibold(20.0)).color(egui::Color32::WHITE));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(tr!("Обновить", "Refresh")).clicked() {
                    let _ = self.tx.send(Cmd::CartLoad);
                }
            });
        });
        ui.separator();
        let cart = match &net.cart {
            None => {
                crate::spin::spinner(ui);
                return;
            }
            Some(Err(e)) => {
                ui.label(trf!("Не удалось загрузить корзину: {e}", "Could not load the cart: {e}"));
                return;
            }
            Some(Ok(c)) => c.clone(),
        };
        if cart.lines.is_empty() {
            ui.weak(tr!("Корзина пуста. Положить игру - кнопка «В корзину» на ее странице в магазине.", "The cart is empty. To add a game, use the \"Add to cart\" button on its store page."));
        }
        let names = net.store.lock().unwrap().package_names.clone();
        for l in &cart.lines {
            let name = names.get(&l.packageid).cloned().unwrap_or_else(|| {
                if l.packageid > 0 && self.cart_ui.names_asked.insert(l.packageid) {
                    let _ = self.tx.send(Cmd::Store(crate::store::StoreCmd::PackageName(l.packageid)));
                }
                if l.bundleid > 0 { trf!("Набор {}", "Bundle {}", l.bundleid) } else { trf!("Пакет {}", "Package {}", l.packageid) }
            });
            egui::Frame::none().fill(crate::theme::CARD).rounding(3.0).inner_margin(egui::Margin::same(8.0)).show(ui, |ui| {
                ui.set_width(ui.available_width().min(760.0));
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(crate::richtext::fix(ui, &name)).size(15.0).color(egui::Color32::WHITE));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(tr!("Убрать", "Remove")).clicked() {
                            let _ = self.tx.send(Cmd::CartRemove(l.line_id));
                        }
                        ui.label(egui::RichText::new(&l.price).strong());
                    });
                });
            });
            ui.add_space(4.0);
        }
        if !cart.lines.is_empty() {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(trf!("Итого: {}", "Total: {}", cart.total)).size(16.0).strong());
                ui.add_space(16.0);
                if ui
                    .add(egui::Button::new(egui::RichText::new(tr!("Оформить на сайте Steam", "Check out on the Steam website")).color(egui::Color32::WHITE)).fill(crate::theme::PLAY))
                    .on_hover_text(tr!("Откроется корзина Steam в браузере: оплата - там", "The Steam cart opens in the browser: payment is there"))
                    .clicked()
                {
                    ui.ctx().open_url(egui::OpenUrl::new_tab("https://store.steampowered.com/cart/"));
                }
            });
            ui.weak(tr!("Корзина общая с сайтом и клиентом Steam: что положено здесь, будет и там.", "The cart is shared with the Steam website and client: what you add here is there too."));
        }
        if let Some(m) = &net.action_msg {
            ui.add_space(6.0);
            ui.label(m);
        }
    }
}

impl LauncherApp {
    /// "Add to cart": if the game has several purchase options - choose one; if one - straight to the cart.
    pub(crate) fn cart_pick_dialog(&mut self, ctx: &egui::Context, net: &NetState) {
        let Some(appid) = self.cart_ui.pick else { return };
        let mut close = false;
        egui::Window::new(tr!("В корзину", "Add to cart"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                match net.app_packages.get(&appid) {
                    None => {
                        crate::spin::spinner(ui);
                    }
                    Some(Err(e)) => {
                        ui.label(trf!("Не удалось узнать варианты покупки: {e}", "Could not get purchase options: {e}"));
                    }
                    Some(Ok(list)) if list.is_empty() => {
                        ui.label(tr!("У этой игры нет вариантов покупки для корзины.", "This game has no purchase options for the cart."));
                    }
                    Some(Ok(list)) => {
                        ui.label(tr!("Что положить в корзину:", "What to add to the cart:"));
                        for (pkg, name, _) in list.iter().take(8) {
                            let label = if name.is_empty() { trf!("Пакет {pkg}", "Package {pkg}") } else { name.clone() };
                            if ui.button(crate::richtext::fix(ui, &label)).clicked() {
                                let _ = self.tx.send(Cmd::CartAdd(*pkg));
                            }
                        }
                    }
                }
                if let Some(m) = &net.action_msg {
                    ui.label(m);
                }
                ui.horizontal(|ui| {
                    if ui.button(tr!("Открыть корзину", "Open cart")).clicked() {
                        close = true;
                        self.open_cart();
                    }
                    if ui.button(tr!("Закрыть", "Close")).clicked() {
                        close = true;
                    }
                });
            });
        if close {
            self.cart_ui.pick = None;
        }
    }
}
