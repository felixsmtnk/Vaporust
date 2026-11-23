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
    /// The cart was loaded for the count on the store band.
    pub counted: bool,
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
        use crate::theme;
        if !self.cart_ui.asked {
            self.cart_ui.asked = true;
            let _ = self.tx.send(Cmd::CartLoad);
        }
        let (names, apps, bundles) = {
            let st = net.store.lock().unwrap();
            (st.package_names.clone(), st.package_apps.clone(), st.bundle_names.clone())
        };
        let mut remove: Option<u64> = None;
        let mut open_app: Option<u32> = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            theme::centered(ui, 940.0, |ui| {
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    theme::page_title(ui, tr!("Ваша корзина", "Your shopping cart"));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if theme::text_link(ui, tr!("Обновить", "Refresh")).clicked() {
                            let _ = self.tx.send(Cmd::CartLoad);
                        }
                    });
                });
                ui.add_space(12.0);
                let cart = match &net.cart {
                    None => {
                        crate::spin::spinner(ui);
                        return;
                    }
                    Some(Err(e)) => {
                        theme::block(ui, |ui| ui.label(trf!("Не удалось загрузить корзину: {e}", "Could not load the cart: {e}")));
                        return;
                    }
                    Some(Ok(c)) => c.clone(),
                };
                if cart.lines.is_empty() {
                    theme::block(ui, |ui| {
                        ui.label(egui::RichText::new(tr!("Ваша корзина пуста.", "Your cart is empty.")).size(15.0).color(egui::Color32::WHITE));
                        ui.label(egui::RichText::new(tr!("Положить игру - кнопка «В корзину» на ее странице в магазине.", "To add a game, use the \"Add to cart\" button on its store page.")).color(theme::WEAK));
                    });
                    return;
                }
                ui.spacing_mut().item_spacing.y = 6.0;
                for l in &cart.lines {
                    // A bundle line has bundleid and no packageid: its name and game come from the bundle.
                    let bundle = (l.packageid == 0 && l.bundleid > 0).then(|| bundles.get(&l.bundleid)).flatten();
                    let known = if l.packageid > 0 { names.get(&l.packageid).cloned() } else { bundle.map(|b| b.0.clone()) };
                    let name = known.unwrap_or_else(|| {
                        if l.packageid > 0 && self.cart_ui.names_asked.insert(l.packageid) {
                            let _ = self.tx.send(Cmd::Store(crate::store::StoreCmd::PackageName(l.packageid)));
                        } else if l.packageid == 0 && l.bundleid > 0 && self.cart_ui.names_asked.insert(0x8000_0000 | l.bundleid) {
                            let _ = self.tx.send(Cmd::Store(crate::store::StoreCmd::BundleName(l.bundleid)));
                        }
                        if l.bundleid > 0 { trf!("Набор {}", "Bundle {}", l.bundleid) } else { trf!("Пакет {}", "Package {}", l.packageid) }
                    });
                    let app = if l.packageid > 0 { apps.get(&l.packageid).copied() } else { bundle.and_then(|b| b.1) };
                    theme::block(ui, |ui| {
                        ui.horizontal(|ui| {
                            // The game capsule, as in the Steam cart (a grey plate while unknown).
                            let size = egui::vec2(184.0, 69.0);
                            match app {
                                Some(a) => {
                                    let url = format!("https://cdn.akamai.steamstatic.com/steam/apps/{a}/capsule_184x69.jpg");
                                    let r = ui.add(egui::Image::new(url).fit_to_exact_size(size).sense(egui::Sense::click()));
                                    if r.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                                        open_app = Some(a);
                                    }
                                }
                                None => {
                                    let (r, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                                    ui.painter().rect_filled(r, 2.0, theme::BUTTON);
                                }
                            }
                            ui.add_space(8.0);
                            ui.vertical(|ui| {
                                ui.add_space(4.0);
                                ui.label(egui::RichText::new(crate::richtext::fix(ui, &name)).size(16.0).color(egui::Color32::WHITE));
                                ui.label(egui::RichText::new(tr!("Для своего аккаунта", "For my account")).size(12.0).color(theme::DIM));
                            });
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.vertical(|ui| {
                                    ui.with_layout(egui::Layout::top_down(egui::Align::Max), |ui| {
                                        // Steam keeps the price of the moment the game was added (it can be in another
                                        // region currency); with one game the current total is its price.
                                        let price = if cart.lines.len() == 1 { cart.total.clone() } else { l.price.clone() };
                                        ui.label(egui::RichText::new(price).size(15.0).color(egui::Color32::WHITE))
                                            .on_hover_text(tr!("Цена на момент добавления в корзину", "The price when it was added to the cart"));
                                        ui.add_space(6.0);
                                        if theme::text_link(ui, tr!("Убрать", "Remove")).clicked() {
                                            remove = Some(l.line_id);
                                        }
                                    });
                                });
                            });
                        });
                    });
                }
                ui.add_space(6.0);
                // The total, as the Steam "Estimated total" box.
                theme::block(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(tr!("Итого", "Estimated total")).size(16.0).color(egui::Color32::WHITE));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(egui::RichText::new(&cart.total).font(theme::semibold(18.0)).color(egui::Color32::WHITE));
                        });
                    });
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new(tr!(
                            "Налоги, если есть, считаются при оплате. Оплата - на сайте Steam: Vaporust не вводит платежные данные.",
                            "Sales tax, if any, is calculated at checkout. Payment is on the Steam website: Vaporust never enters payment details."
                        ))
                        .size(12.0)
                        .color(theme::WEAK),
                    );
                    ui.add_space(8.0);
                    ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), 36.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if theme::buy_button(ui, tr!("Перейти к оплате", "Continue to payment"))
                            .on_hover_text(tr!("Откроется корзина Steam в браузере", "The Steam cart opens in the browser"))
                            .clicked()
                        {
                            ui.ctx().open_url(egui::OpenUrl::new_tab("https://store.steampowered.com/cart/"));
                        }
                    });
                });
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(tr!(
                        "Корзина общая с сайтом и клиентом Steam: что положено здесь, будет и там.",
                        "The cart is shared with the Steam website and client: what you add here is there too."
                    ))
                    .size(12.0)
                    .color(theme::DIM),
                );
                if let Some(m) = &net.action_msg {
                    ui.add_space(6.0);
                    ui.label(m);
                }
            });
        });
        if let Some(line) = remove {
            let _ = self.tx.send(Cmd::CartRemove(line));
        }
        if let Some(a) = open_app {
            self.open_store(Some(a), false);
        }
    }
}

impl LauncherApp {
    /// "Add to cart": if the game has several purchase options - choose one; if one - straight to the cart.
    pub(crate) fn cart_pick_dialog(&mut self, ctx: &egui::Context, net: &NetState) {
        let Some(appid) = self.cart_ui.pick else { return };
        let mut close = crate::theme::esc(ctx); // Esc - cancel
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
