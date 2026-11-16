//! "Settings" tab: library, game launch, window, store, about.
//! Everything is saved automatically (eframe persistence). Message status and sound are
//! in the friends panel, so they are not here.

use crate::net::{self, Cmd, NetState};
use crate::{local, portable, settings, tray, LauncherApp};
use eframe::egui;

const GITHUB: &str = "https://github.com/felixsmtnk/Vaporust";
/// Width of the setting names column.
const LABEL_W: f32 = 200.0;

/// Names column width: 200, and smaller in a narrow window (or at a large scale) so
/// values have enough room.
fn label_w(ui: &egui::Ui) -> f32 {
    (ui.available_width() * 0.36).clamp(110.0, LABEL_W)
}

/// Settings sections (on the left), in order.
#[allow(non_snake_case)]
pub fn SECTIONS() -> [&'static str; 9] {
    [tr!("Аккаунт", "Account"), tr!("Библиотека", "Library"), tr!("Загрузки", "Downloads"), tr!("Запуск игр", "Launching games"), tr!("Окно", "Window"), tr!("Производительность", "Performance"), tr!("Уведомления", "Notifications"), tr!("Сеть", "Network"), tr!("О программе", "About")]
}

thread_local! {
    /// Which section is open now and which headers are already drawn in this frame.
    static CURRENT: std::cell::RefCell<(String, Vec<String>)> = const { std::cell::RefCell::new((String::new(), Vec::new())) };
}

/// Part of section `title`: drawn only if this section is selected on the left. The header - once,
/// even if the section is assembled from several parts.
fn card(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui)) {
    let (show, first) = CURRENT.with(|c| {
        let mut c = c.borrow_mut();
        let show = c.0 == title;
        let first = show && !c.1.iter().any(|t| t == title);
        if first {
            c.1.push(title.to_string());
        }
        (show, first)
    });
    if !show {
        return;
    }
    if first {
        ui.label(egui::RichText::new(title).size(20.0).strong());
        ui.add_space(12.0);
    } else {
        ui.add_space(6.0);
        ui.separator();
        ui.add_space(6.0);
    }
    add(ui);
    ui.add_space(8.0);
}

/// A "name - value" row.
pub(crate) fn row(ui: &mut egui::Ui, label: &str, add: impl FnOnce(&mut egui::Ui)) {
    let lw = label_w(ui);
    // Values (scale buttons and the like) that do not fit wrap to the next line.
    ui.horizontal(|ui| {
        ui.add_sized([lw, 22.0], egui::Label::new(egui::RichText::new(label).strong()).wrap().halign(egui::Align::LEFT));
        // Wrapping happens inside the values column, not under the name.
        ui.horizontal_wrapped(|ui| add(ui));
    });
    ui.add_space(6.0);
}

/// Explanation under a row, indented by the names column width.
fn note(ui: &mut egui::Ui, text: &str) {
    let lw = label_w(ui);
    ui.horizontal(|ui| {
        ui.add_space(lw + ui.spacing().item_spacing.x);
        // A separate column: in a horizontal row long text does not wrap and gets cut off.
        ui.vertical(|ui| {
            ui.set_max_width(ui.available_width());
            ui.add(egui::Label::new(egui::RichText::new(text).weak().small()).wrap());
        });
    });
    ui.add_space(10.0);
}

impl LauncherApp {
    /// Interface scale: setting -> egui; Ctrl+plus/minus (egui changes it itself) -> setting.
    pub(crate) fn zoom_tick(&mut self, ctx: &egui::Context) {
        // Own scale steps instead of egui's (it uses 10% steps without a limit): Ctrl+plus/minus - the next
        // step from settings, Ctrl+0 - 100%.
        ctx.options_mut(|o| o.zoom_with_keyboard = false);
        let cur = SCALES.iter().position(|v| (v - self.settings.ui_scale).abs() < 0.01).unwrap_or(2);
        let (plus, minus, reset) = ctx.input_mut(|i| {
            let m = egui::Modifiers::COMMAND;
            (
                i.consume_key(m, egui::Key::Plus) || i.consume_key(m, egui::Key::Equals),
                i.consume_key(m, egui::Key::Minus),
                i.consume_key(m, egui::Key::Num0),
            )
        });
        if plus {
            self.settings.ui_scale = SCALES[(cur + 1).min(SCALES.len() - 1)];
        } else if minus {
            self.settings.ui_scale = SCALES[cur.saturating_sub(1)];
        } else if reset {
            self.settings.ui_scale = 1.0;
        }
        // A value not from the list (old settings) - to the nearest step.
        if !SCALES.iter().any(|v| (v - self.settings.ui_scale).abs() < 0.01) {
            let s = self.settings.ui_scale;
            self.settings.ui_scale = *SCALES.iter().min_by(|a, b| (*a - s).abs().total_cmp(&(*b - s).abs())).unwrap_or(&1.0);
        }
        let want = self.settings.ui_scale;
        if (want - ctx.zoom_factor()).abs() > 0.001 {
            ctx.set_zoom_factor(want);
            self.zoom_applied = want;
        }
        // The minimum window size grows with the scale: at 150% in a 760x460 window the sections on top
        // overlapped the window buttons. But not larger than the screen. (The size is in points, which already include
        // the scale, so - on the next frame, once the scale is applied.)
        let ppp = ctx.pixels_per_point();
        let screen = ctx.input(|i| i.viewport().monitor_size).unwrap_or(egui::vec2(1920.0, 1080.0));
        let min = egui::vec2(760.0f32.min(screen.x * 0.95), 460.0f32.min(screen.y * 0.9));
        if (ppp - self.min_ppp_sent).abs() > 0.001 {
            self.min_ppp_sent = ppp;
            ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(min));
        }
        // The window is below the minimum (the scale was just increased) - enlarge it. Checked every frame, but
        // the command is sent at most twice a second: Windows does not apply the size immediately.
        let (inner, minimized) = ctx.input(|i| (i.viewport().inner_rect, i.viewport().minimized == Some(true)));
        if let Some(r) = inner.filter(|_| !minimized && !crate::notify::in_tray()) {
            let small = r.width() < min.x - 1.0 || r.height() < min.y - 1.0;
            if small && self.min_grow_at.is_none_or(|t| t.elapsed() > std::time::Duration::from_millis(500)) {
                self.min_grow_at = Some(std::time::Instant::now());
                ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(min));
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(r.width().max(min.x), r.height().max(min.y))));
            }
        }
    }

    /// Pass the download speed limit and proxy to Aurelia.
    pub(crate) fn send_aurelia_config(&mut self) {
        let proxy = Some(self.settings.proxy.trim().to_string()).filter(|p| !p.is_empty());
        // With the schedule taken into account: in its hours a separate speed may apply.
        let kb = crate::schedule::effective_limit_kb(&self.settings);
        self.sent_limit = Some(kb);
        let _ = self.tx.send(Cmd::AureliaConfig { limit_kbit: kb * 8, proxy });
    }

    pub(crate) fn settings_contents(&mut self, ui: &mut egui::Ui, net: &NetState) {
        // Sections on the left (as in Steam settings), the selected section on the right.
        let current = SECTIONS().get(self.settings_section as usize).copied().unwrap_or(SECTIONS()[0]);
        CURRENT.with(|c| *c.borrow_mut() = (current.to_string(), Vec::new()));
        egui::SidePanel::left("settings_sections").resizable(false).exact_width(170.0).show_inside(ui, |ui| {
            ui.add_space(8.0);
            ui.label(egui::RichText::new(tr!("Настройки", "Settings")).size(20.0).strong());
            ui.add_space(10.0);
            for (i, s) in SECTIONS().iter().enumerate() {
                let sel = self.settings_section as usize == i;
                let text = if sel { egui::RichText::new(*s).strong() } else { egui::RichText::new(*s) };
                if ui.add_sized([150.0, 26.0], egui::SelectableLabel::new(sel, text)).clicked() {
                    self.settings_section = i as u8;
                }
            }
        });
        egui::CentralPanel::default().show_inside(ui, |ui| {
        egui::ScrollArea::vertical().id_salt(("settings", self.settings_section)).auto_shrink([false, false]).show(ui, |ui| {
            ui.add_space(8.0);
            // Not wider than the visible area: otherwise explanations wrap at 820 px and get cut off on the right.
            ui.set_max_width(ui.available_width().min(820.0));
            if current == tr!("Аккаунт", "Account") && net.account.is_none() {
                ui.label(egui::RichText::new(tr!("Аккаунт", "Account")).size(20.0).strong());
                ui.weak(tr!("Войдите в аккаунт Steam, чтобы увидеть сведения о нем.", "Log in to your Steam account to see its details."));
            }

            // ---------- Account ----------
            if let (Some(login), Some(me)) = (net.account.clone(), net.my_id) {
                if !self.account_asked {
                    self.account_asked = true;
                    let _ = self.tx.send(Cmd::AccountInfo);
                    if !net.profiles.contains_key(&me) {
                        let _ = self.tx.send(Cmd::Social(crate::social::SocialCmd::Profile(me)));
                    }
                    let has_wallet = net.inv.lock().unwrap().wallet.as_ref().is_some_and(|w| w.is_ok());
                    if !has_wallet {
                        let _ = self.tx.send(Cmd::Inv(crate::invent::InvCmd::Wallet));
                    }
                }
                let profile = net.profiles.get(&me).and_then(|p| p.as_ref().ok());
                card(ui, tr!("Аккаунт", "Account"), |ui| {
                    row(ui, tr!("Логин", "Login"), |ui| {
                        ui.label(&login);
                    });
                    if let Some(name) = profile.and_then(|p| p.persona_name.as_ref()) {
                        row(ui, tr!("Имя в Steam", "Steam name"), |ui| {
                            ui.label(name);
                        });
                    }
                    row(ui, "SteamID", |ui| {
                        ui.label(me.to_string());
                        if ui.small_button(tr!("Копировать", "Copy")).clicked() {
                            ui.ctx().copy_text(me.to_string());
                        }
                    });
                    match net.session_secure {
                        Some(true) => row(ui, tr!("Сохраненный вход", "Saved login"), |ui| {
                            ui.label(tr!("зашифрован", "encrypted"))
                                .on_hover_text(tr!("Ключ шифрования хранится в диспетчере учетных данных Windows и доступен только вашему пользователю Windows.", "The encryption key is kept in the Windows Credential Manager and is available only to your Windows user."));
                        }),
                        Some(false) => row(ui, tr!("Сохраненный вход", "Saved login"), |ui| {
                            ui.colored_label(egui::Color32::from_rgb(230, 170, 60), tr!("не зашифрован", "not encrypted"))
                                .on_hover_text(tr!("Хранилище ключей Windows недоступно, поэтому файл входа лежит открытым текстом. Не передавайте папку данных Vaporust другим людям.", "The Windows key storage is unavailable, so the login file is stored as plain text. Do not give the Vaporust data folder to other people."));
                        }),
                        None => {}
                    }
                    let code = me.saturating_sub(crate::social::STEAMID64_BASE);
                    row(ui, tr!("Код друга", "Friend code"), |ui| {
                        ui.label(code.to_string());
                        if ui.small_button(tr!("Копировать", "Copy")).clicked() {
                            ui.ctx().copy_text(code.to_string());
                        }
                    });
                    if let Some(c) = profile.and_then(|p| p.created) {
                        row(ui, tr!("В Steam с", "On Steam since"), |ui| {
                            ui.label(crate::socialwin::fmt_date(c));
                        });
                    }
                    if profile.is_some_and(|p| p.is_limited) {
                        row(ui, tr!("Ограничения", "Limitations"), |ui| {
                            ui.colored_label(egui::Color32::from_rgb(230, 170, 60), tr!("ограниченный аккаунт", "limited account"))
                                .on_hover_text(tr!("Steam ограничивает аккаунты, на которых потрачено меньше 5 $: нельзя добавлять друзей, торговать и т.п.", "Steam limits accounts that spent less than $5: you cannot add friends, trade and so on."));
                        });
                    }
                    if let Some(d) = &net.account_data {
                        if !d.country.is_empty() {
                            row(ui, tr!("Страна по подключению", "Country by connection"), |ui| {
                                ui.label(&d.country);
                            });
                        }
                    }
                    let wallet = net.inv.lock().unwrap().wallet.clone();
                    row(ui, tr!("Кошелек Steam", "Steam wallet"), |ui| match &wallet {
                        Some(Ok(w)) => {
                            ui.label(crate::invent::money(w.balance_cents, w.currency));
                        }
                        Some(Err(e)) => {
                            ui.weak(tr!("не удалось узнать", "could not find out")).on_hover_text(e);
                        }
                        None => {
                            crate::spin::spinner(ui);
                        }
                    });
                    row(ui, tr!("Экспериментальные функции", "Experimental features"), |ui| {
                        ui.checkbox(&mut self.settings.experimental, tr!("Включены", "On"));
                    });
                    note(
                        ui,
                        tr!("Продажа и обмены, закрепление и откат версии, комментарии, оценки, награды, группы, \
                         блокировка, правка профиля. Проверены мало, многие видны другим людям.", "Selling and trades, version pinning and rollback, comments, ratings, awards, groups, blocking, profile editing. Little tested, many are visible to other people."),
                    );
                    // Quick sell on the market: how much cheaper than the lowest listing.
                    let (cur, step) = match &wallet {
                        Some(Ok(w)) => (w.currency, w.fee_rules.map(|r| r.currency_increment).unwrap_or(1)),
                        _ => (1, 1),
                    };
                    row(ui, tr!("Быстрая продажа", "Quick sale"), |ui| {
                        ui.label(tr!("дешевле самого дешевого лота на", "cheaper than the cheapest listing by"));
                        let mut v = self.settings.quick_undercut as f64 / 100.0;
                        if ui.add(egui::DragValue::new(&mut v).range(0.0..=100000.0).speed(0.05).fixed_decimals(2)).changed() {
                            self.settings.quick_undercut = (v * 100.0).round() as i64;
                        }
                    });
                    note(
                        ui,
                        &trf!(
                            "0 - на один шаг цены вашей валюты ({}). Меньше шага Steam не разрешит: сумма округлится вверх до шага.", "0 - by one price step of your currency ({}). Steam does not allow less than a step: the amount is rounded up to the step.",
                            crate::invent::money(step, cur)
                        ),
                    );
                    row(ui, tr!("Игр", "Games"), |ui| {
                        let own = net.owned.len();
                        let fam = net.family.iter().filter(|f| !net.owned.iter().any(|o| o.app_id == f.app_id)).count();
                        let mut t = trf!("{own} на аккаунте", "{own} on the account");
                        if fam > 0 {
                            t.push_str(&trf!(", еще {fam} в семейной библиотеке", ", {fam} more in the family library"));
                        }
                        t.push_str(&trf!(", установлено {}", ", {} installed", self.games.len()));
                        ui.label(t);
                    });
                    row(ui, tr!("Где выполнен вход", "Where you are logged in"), |ui| {
                        if ui.button(if self.sessions_open { tr!("Скрыть", "Hide") } else { tr!("Показать", "Show") }).clicked() {
                            self.sessions_open = !self.sessions_open;
                            if self.sessions_open {
                                let _ = self.tx.send(Cmd::Sessions);
                            }
                        }
                        if ui.small_button(tr!("Управление на сайте", "Manage on the website")).on_hover_text(tr!("Выйти на устройстве - на сайте Steam", "Log out on a device - on the Steam website")).clicked() {
                            ui.ctx().open_url(egui::OpenUrl::new_tab("https://store.steampowered.com/account/authorizeddevices"));
                        }
                    });
                    if self.sessions_open {
                        match &net.sessions {
                            None => {
                                crate::spin::spinner(ui);
                            }
                            Some(Err(e)) => {
                                ui.weak(trf!("Не удалось узнать: {e}", "Could not find out: {e}"));
                            }
                            Some(Ok(list)) => {
                                egui::Grid::new("sessions").num_columns(4).striped(true).show(ui, |ui| {
                                    for s in list {
                                        let first = if s.first_time > 0 { trf!("Впервые: {}", "First seen: {}", crate::socialwin::fmt_date(s.first_time)) } else { String::new() };
                                        ui.label(if s.logged_in { trf!("{} (вход активен)", "{} (login active)", s.name) } else { s.name.clone() }).on_hover_text(first);
                                        ui.weak(&s.kind);
                                        ui.weak(if s.last_time > 0 { crate::socialwin::fmt_date(s.last_time) } else { String::new() });
                                        ui.weak(&s.last_place);
                                        ui.end_row();
                                    }
                                });
                                note(ui, tr!("Незнакомое устройство - смените пароль и выйдите на всех устройствах на сайте Steam.", "An unfamiliar device - change the password and log out on all devices on the Steam website."));
                            }
                        }
                    }
                    row(ui, "", |ui| {
                        if ui.button(tr!("Профиль", "Profile")).clicked() {
                            self.open_profile(me, net);
                        }
                        if ui.button(tr!("Аккаунт на сайте Steam", "Account on the Steam website")).on_hover_text(tr!("Почта, пароль, Steam Guard, история покупок", "Email, password, Steam Guard, purchase history")).clicked() {
                            ui.ctx().open_url(egui::OpenUrl::new_tab("https://store.steampowered.com/account/"));
                        }
                        if ui.button(tr!("Обновить", "Refresh")).clicked() {
                            self.account_asked = false;
                            net.inv.lock().unwrap().wallet = None;
                        }
                    });
                });

                // Licenses and purchase history - like "Account details" in Steam (view only).
                card(ui, tr!("Аккаунт", "Account"), |ui| {
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new(tr!("Покупки", "Purchases")).size(15.0).strong());
                    let lic = egui::CollapsingHeader::new(match &net.licenses {
                        Some(l) => trf!("Мои лицензии ({})", "My licenses ({})", l.len()),
                        None => tr!("Мои лицензии", "My licenses").to_string(),
                    })
                    .id_salt("licenses")
                    .show(ui, |ui| {
                        match &net.licenses {
                            None => {
                                crate::spin::spinner(ui);
                            }
                            Some(list) => {
                                ui.add(egui::TextEdit::singleline(&mut self.lic_filter).hint_text(tr!("поиск", "search")).desired_width(260.0));
                                let f = self.lic_filter.to_lowercase();
                                egui::ScrollArea::vertical().id_salt("lic_scroll").max_height(320.0).show(ui, |ui| {
                                    egui::Grid::new("lic_grid").num_columns(3).spacing([14.0, 3.0]).striped(true).show(ui, |ui| {
                                        for l in list.iter().filter(|l| f.is_empty() || l.name.to_lowercase().contains(&f)) {
                                            let when = chrono::DateTime::from_timestamp(l.date, 0)
                                                .map(|t| t.format("%d.%m.%Y").to_string())
                                                .unwrap_or_default();
                                            ui.weak(when);
                                            ui.label(&l.name);
                                            ui.weak(match l.method.as_str() {
                                                "Steam Store" => tr!("магазин Steam", "Steam store"),
                                                "Complimentary" => tr!("бесплатно", "free"),
                                                "Gift/Guest Pass" => tr!("подарок", "gift"),
                                                "Retail" => tr!("ключ", "key"),
                                                other => other,
                                            });
                                            ui.end_row();
                                        }
                                    });
                                });
                            }
                        }
                    });
                    if lic.fully_open() && net.licenses.is_none() && !self.licenses_asked {
                        self.licenses_asked = true;
                        let _ = self.tx.send(Cmd::Licenses);
                    }
                    let hist = egui::CollapsingHeader::new(tr!("История покупок и кошелька", "Purchase and wallet history")).id_salt("wallet_hist").show(ui, |ui| {
                        match &net.wallet_history {
                            None => {
                                crate::spin::spinner(ui);
                            }
                            Some(Err(e)) => {
                                ui.label(trf!("Не удалось загрузить: {e}", "Could not load: {e}"));
                            }
                            Some(Ok(rows)) => {
                                egui::ScrollArea::vertical().id_salt("wallet_scroll").max_height(320.0).show(ui, |ui| {
                                    egui::Grid::new("wallet_grid").num_columns(4).spacing([14.0, 3.0]).striped(true).show(ui, |ui| {
                                        for r in rows {
                                            ui.weak(&r.date);
                                            ui.add(egui::Label::new(&r.items).truncate());
                                            ui.weak(&r.kind);
                                            ui.label(if r.change.is_empty() { &r.total } else { &r.change });
                                            ui.end_row();
                                        }
                                    });
                                });
                            }
                        }
                    });
                    if hist.fully_open() && net.wallet_history.is_none() && !self.wallet_asked {
                        self.wallet_asked = true;
                        let _ = self.tx.send(Cmd::WalletHistory);
                    }
                });
            }

            // ---------- Library ----------
            card(ui, tr!("Библиотека", "Library"), |ui| {
                row(ui, tr!("Вид", "View"), |ui| {
                    ui.selectable_value(&mut self.settings.view, settings::View::Grid, tr!("Плитка", "Grid"));
                    ui.selectable_value(&mut self.settings.view, settings::View::List, tr!("Список", "List"));
                });
                note(ui, tr!("Плитка - обложки игр, список - компактные строки.", "Grid - game covers, list - compact rows."));

                row(ui, tr!("Сторонние игры", "Non-Steam games"), |ui| {
                    ui.checkbox(&mut self.settings.custom_to_steam, tr!("Добавлять и в Steam", "Add to Steam too"));
                });
                note(ui, tr!("Новая сторонняя игра появится и в настоящем Steam (если Steam открыт - после его закрытия).", "A new non-Steam game also appears in the real Steam (if Steam is open - after it closes)."));
                row(ui, tr!("Главная библиотеки", "Library home"), |ui| {
                    ui.checkbox(&mut self.settings.random_button, tr!("Показывать «Во что поиграть?» над библиотекой", "Show \"What to play?\" above the library"));
                    ui.checkbox(&mut self.settings.whats_new, tr!("Показывать «Что нового» (новости своих игр) на главной", "Show \"What's new\" (news of your games) on the home page"));
                });

                row(ui, tr!("Устанавливать новые игры в", "Install new games to"), |ui| {
                    if self.libraries.is_empty() {
                        ui.weak(tr!("папки библиотек Steam не найдены", "Steam library folders not found"));
                        return;
                    }
                    let label = |p: &std::path::PathBuf| {
                        let free = local::free_space(p)
                            .map(|f| trf!("  (свободно {})", "  ({} free)", net::fmt_bytes(f)))
                            .unwrap_or_default();
                        format!("{}{free}", crate::local::pretty_path(p))
                    };
                    let current = self.libraries.get(self.lib_idx).map(label).unwrap_or_default();
                    egui::ComboBox::from_id_salt("settings_library").selected_text(current).show_ui(ui, |ui| {
                        for (i, p) in self.libraries.iter().enumerate() {
                            ui.selectable_value(&mut self.lib_idx, i, label(p));
                        }
                    });
                    if let Some(p) = self.libraries.get(self.lib_idx) {
                        if ui.button(tr!("Открыть", "Open")).on_hover_text(tr!("Открыть папку в Проводнике", "Open the folder in Explorer")).clicked() {
                            local::open_folder(p);
                        }
                    }
                });
                row(ui, tr!("Папки библиотек", "Library folders"), |ui| {
                    let steam_open = self.steam_running();
                    if ui
                        .add_enabled(!steam_open && self.steam.is_some(), egui::Button::new(tr!("Добавить папку...", "Add folder...")))
                        .on_hover_text(tr!("Новая папка для игр, например на другом диске (как «Хранилище -> Добавить диск» в Steam)", "A new folder for games, for example on another drive (like \"Storage -> Add Drive\" in Steam)"))
                        .on_disabled_hover_text(tr!("Сначала закройте Steam", "Close Steam first"))
                        .clicked()
                    {
                        if let (Some(dir), Some(steam)) = (rfd::FileDialog::new().set_title(tr!("Папка для игр Steam", "Folder for Steam games")).pick_folder(), self.steam.clone()) {
                            let text = match local::add_library(&steam.dir, &dir) {
                                Ok(p) => {
                                    self.rescan();
                                    if let Some(i) = self.libraries.iter().position(|l| local::pretty_path(l) == local::pretty_path(&p)) {
                                        self.lib_idx = i;
                                    }
                                    trf!("Папка библиотеки добавлена: {}", "Library folder added: {}", local::pretty_path(&p))
                                }
                                Err(e) => trf!("Не удалось добавить папку библиотеки: {e}", "Could not add the library folder: {e}"),
                            };
                            *self.launch_status.lock().unwrap() = text;
                        }
                    }
                });
                note(ui, tr!("Steam увидит новую папку при следующем запуске. Переносить игры между папками - в «Свойства» -> «Установленные файлы».", "Steam sees the new folder the next time it starts. Move games between folders in \"Properties\" -> \"Installed files\"."));
            });

            // ---------- Downloads ----------
            card(ui, tr!("Загрузки", "Downloads"), |ui| {
                // Download region: the Steam server "cell" number (cell id). Steam keeps its choice in
                // config.vdf (CellIDServerOverride) - it can be taken from there.
                let steam_cell = self.steam.as_ref().and_then(|s| crate::local::steam_cell_override(&s.dir));
                // List of cities with Steam servers - once per session (querying the Steam server directory).
                thread_local! {
                    static REGIONS_ASKED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
                }
                if !REGIONS_ASKED.with(|a| a.replace(true)) {
                    let _ = self.tx.send(Cmd::DownloadRegions);
                }
                let regions: Vec<(u32, String)> = net.download_regions.as_ref().and_then(|r| r.as_ref().ok()).cloned().unwrap_or_default();
                row(ui, tr!("Регион загрузки", "Download region"), |ui| {
                    let cur = self.settings.download_cell;
                    let label = |c: u32| -> String {
                        if c == 0 {
                            tr!("Автоматически", "Automatic").into()
                        } else if let Some((_, n)) = regions.iter().find(|(id, _)| *id == c) {
                            n.clone()
                        } else if Some(c) == steam_cell {
                            trf!("Как в Steam (№{c})", "As in Steam (No. {c})")
                        } else {
                            trf!("Регион №{c}", "Region No. {c}")
                        }
                    };
                    egui::ComboBox::from_id_salt("download_region").width(260.0).selected_text(label(cur)).show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.settings.download_cell, 0, tr!("Автоматически", "Automatic"));
                        if let Some(c) = steam_cell {
                            ui.selectable_value(&mut self.settings.download_cell, c, trf!("Как в Steam (№{c})", "As in Steam (No. {c})"));
                        }
                        ui.separator();
                        if regions.is_empty() {
                            ui.weak(tr!("Загружаю список регионов...", "Loading the region list..."));
                        }
                        for (id, name) in &regions {
                            ui.selectable_value(&mut self.settings.download_cell, *id, name);
                        }
                    });
                });
                note(ui, tr!("С каких серверов Steam скачивать игры. «Как в Steam» - регион, выбранный в настройках Steam. Если загрузка медленная, выберите ближайший город.", "Which Steam servers to download games from. \"As in Steam\" - the region chosen in Steam settings. If downloads are slow, choose the nearest city."));
                let mut apply = false;
                row(ui, tr!("Ограничить скорость до", "Limit speed to"), |ui| {
                    apply |= limit_field(ui, &mut self.limit_text, &mut self.settings.download_limit_kb);
                });
                note(
                    ui,
                    tr!("Пусто или 0 - без ограничения. Действует сразу, даже на идущую загрузку. Только для загрузок \
                     через Vaporust (установка, откат); Steam при обновлении игр качает по своим настройкам.", "Empty or 0 - no limit. Applies right away, even to a running download. Only for downloads through Vaporust (install, rollback); Steam downloads game updates by its own settings."),
                );

                row(ui, tr!("Прерванные загрузки", "Interrupted downloads"), |ui| {
                    ui.checkbox(&mut self.settings.resume_downloads, tr!("Продолжать сами", "Resume by themselves"));
                });
                note(
                    ui,
                    tr!("Если Vaporust закрыли посреди загрузки, при следующем запуске она продолжится с того же \
                     места (без галочки - встанет в очередь на паузе). Загрузка, оборванная пропажей связи, \
                     продолжится, когда Steam снова будет доступен.", "If Vaporust was closed mid-download, it continues from the same place at the next start (unchecked - it is queued and paused). A download cut off by a lost connection continues when Steam is available again."),
                );

                row(ui, tr!("Когда Steam открыт", "When Steam is open"), |ui| {
                    ui.checkbox(&mut self.settings.download_with_steam, tr!("Все равно качать новые игры", "Download new games anyway"));
                });
                note(ui, tr!("Игра качается во временную папку VaporustStaging внутри библиотеки (Steam ее не видит) и переносится в библиотеку, когда докачается. Начатые раньше в самой библиотеке ждут закрытия Steam.", "The game downloads into a temporary VaporustStaging folder inside the library (Steam does not see it) and is moved to the library when finished. Downloads started earlier in the library itself wait for Steam to close."));
                row(ui, tr!("Кэш загрузок Steam", "Steam download cache"), |ui| {
                    let steam_open = self.steam_running();
                    if ui
                        .add_enabled(!steam_open && self.steam.is_some(), egui::Button::new(tr!("Очистить", "Clear")))
                        .on_hover_text(tr!("Помогает, если загрузки в Steam зависают. Игры и недокачанные загрузки не трогаются.", "Helps if downloads in Steam get stuck. Games and unfinished downloads are not touched."))
                        .on_disabled_hover_text(tr!("Сначала закройте Steam", "Close Steam first"))
                        .clicked()
                    {
                        if let Some(steam) = self.steam.clone() {
                            *self.launch_status.lock().unwrap() = match local::clear_download_cache(&steam.dir) {
                                Ok(freed) => trf!("Кэш загрузок Steam очищен, освобождено {}.", "Steam download cache cleared, {} freed.", net::fmt_bytes(freed)),
                                Err(e) => trf!("Не удалось очистить кэш загрузок: {e}", "Could not clear the download cache: {e}"),
                            };
                        }
                    }
                });

                // ---------- Download schedule ----------
                row(ui, tr!("Расписание загрузок", "Download schedule"), |ui| {
                    ui.label(tr!("с", "from"));
                    apply |= hour_box(ui, "sched_from", &mut self.settings.sched_from);
                    ui.label(tr!("до", "to"));
                    apply |= hour_box(ui, "sched_to", &mut self.settings.sched_to);
                    if crate::schedule::window_now(&self.settings) {
                        ui.weak(tr!("(сейчас эти часы)", "(it is these hours now)"));
                    }
                });
                row(ui, "", |ui| {
                    ui.checkbox(&mut self.settings.sched_only, tr!("Качать только в эти часы", "Download only during these hours"));
                });
                row(ui, "", |ui| {
                    apply |= ui.checkbox(&mut self.settings.sched_limit_on, tr!("В эти часы скорость до", "During these hours speed up to")).changed();
                    ui.add_enabled_ui(self.settings.sched_limit_on, |ui| {
                        apply |= limit_field(ui, &mut self.sched_text, &mut self.settings.sched_limit_kb);
                    });
                });
                note(
                    ui,
                    tr!("Например, с 01:00 до 07:00. «Качать только в эти часы»: в другое время очередь ждет, а начатая \
                     загрузка встает на паузу и сама продолжится в начале этих часов. «Скорость»: например, ночью \
                     без ограничения (пусто), а днем - по полю выше. Vaporust должен быть запущен (можно в трее). \
                     Если «с» и «до» совпадают - круглые сутки.", "For example, from 01:00 to 07:00. \"Download only during these hours\": at other times the queue waits, and a started download pauses and continues by itself when these hours begin. \"Speed\": for example, unlimited at night (empty) and by the field above during the day. Vaporust must be running (the tray is fine). If \"from\" and \"to\" are equal - all day."),
                );
                if apply {
                    self.send_aurelia_config();
                }
            });

            // ---------- Library (continued) ----------
            card(ui, tr!("Библиотека", "Library"), |ui| {
                row(ui, tr!("Список игр", "Game list"), |ui| {
                    if ui.button(tr!("Сохранить в таблицу (CSV)", "Save to a table (CSV)")).on_hover_text(tr!("Все игры: установлена ли, размер, часы, коллекции - для Excel", "All games: installed or not, size, hours, collections - for Excel")).clicked() {
                        if let Some(m) = self.export_library(net) {
                            self.export_note = Some(m);
                        }
                    }
                });
                if let Some(m) = &self.export_note {
                    note(ui, m);
                }
                row(ui, tr!("Хранилище", "Storage"), |ui| {
                    if ui.button(tr!("Открыть", "Open")).on_hover_text(tr!("Сколько места занимают игры на каждом диске и что осталось от удаленных игр", "How much space games take on each drive and what is left of removed games")).clicked() {
                        self.open_storage();
                    }
                });
                row(ui, tr!("Дубли установок", "Duplicate installs"), |ui| {
                    if ui.button(tr!("Найти", "Find")).clicked() {
                        self.duplicates = self.steam.as_ref().map(|s| local::find_duplicates(&s.dir));
                    }
                    match &self.duplicates {
                        Some(d) if d.is_empty() => {
                            ui.weak(tr!("дублей нет", "no duplicates"));
                        }
                        Some(d) => {
                            ui.colored_label(egui::Color32::from_rgb(240, 200, 80), trf!("найдено: {}", "found: {}", d.len()));
                        }
                        None => {}
                    }
                });
                if let Some(d) = self.duplicates.clone().filter(|d| !d.is_empty()) {
                    for (_, name, libs) in d {
                        ui.horizontal_wrapped(|ui| {
                            ui.add_space(label_w(ui) + ui.spacing().item_spacing.x);
                            ui.label(format!("{name}:"));
                            for l in libs {
                                if ui.small_button(crate::local::pretty_path(&l)).on_hover_text(tr!("Открыть папку", "Open folder")).clicked() {
                                    local::open_folder(&l.join("steamapps"));
                                }
                            }
                        });
                    }
                    note(ui, tr!("Игра лежит в двух библиотеках сразу - Steam из-за этого может без конца ее обновлять. Удалите лишнюю копию (папку steamapps\\common\\<игра> и файл appmanifest_<номер>.acf) при закрытом Steam.", "The game is in two libraries at once - because of this Steam may update it endlessly. Delete the extra copy (the steamapps\\common\\<game> folder and the appmanifest_<number>.acf file) while Steam is closed."));
                } else {
                    note(
                        ui,
                        tr!("Если на ПК несколько папок библиотек Steam (например, на дисках C: и D:), одна и та же игра \
                         могла оказаться установленной дважды и занимать место. Кнопка ищет такие копии; лишнюю \
                         можно удалить. Если библиотека одна, дублей не бывает.", "If the PC has several Steam library folders (for example on drives C: and D:), the same game may have been installed twice and take space. The button finds such copies; the extra one can be deleted. With a single library there are no duplicates."),
                    );
                }

                row(ui, tr!("Семейная библиотека", "Family library"), |ui| {
                    ui.checkbox(&mut self.settings.show_family, tr!("Показывать игры семьи в «Все игры»", "Show family games in \"All games\""));
                    let n = net.family.len();
                    if n > 0 {
                        ui.weak(trf!("доступно игр: {n}", "games available: {n}"));
                    }
                });
                note(
                    ui,
                    tr!("Игры, которыми с вами делятся члены семьи Steam. Их можно установить и запустить, но только \
                     пока в них не играет кто-то другой из семьи. Отдельно их можно открыть пунктом «Семейная \
                     библиотека» в списке коллекций.", "Games shared with you by Steam Family members. You can install and play them, but only while nobody else in the family plays them. They can be opened separately with \"Family library\" in the collection list."),
                );

                row(ui, tr!("Список игр аккаунта", "Account game list"), |ui| {
                    if ui
                        .add_enabled(net.account.is_some() && !net.busy, egui::Button::new(tr!("Обновить библиотеку", "Refresh library")))
                        .clicked()
                    {
                        let _ = self.tx.send(Cmd::RefreshLibrary);
                    }
                    if net.busy {
                        crate::spin::spinner(ui);
                    }
                });
                note(ui, tr!("Заново загрузить из Steam список купленных игр и время в игре (например, после покупки).", "Reload the list of owned games and playtime from Steam (for example after a purchase)."));
            });

            // ---------- Game launch ----------
            card(ui, tr!("Запуск игр", "Launching games"), |ui| {
                row(ui, tr!("Обновление игр", "Game updates"), |ui| {
                    ui.radio_value(&mut self.settings.update_via_steam, false, tr!("Без Steam", "Without Steam"));
                    ui.radio_value(&mut self.settings.update_via_steam, true, tr!("Через Steam", "Via Steam"));
                });
                note(
                    ui,
                    tr!("Без Steam - Vaporust сам скачивает обновления, файлы другого языка или беты (выбранных в \
                     «Свойствах»); Steam не запускается - легче для старого ПК. Настройки игры сохраняются. \
                     Через Steam - как раньше: скрытый Steam обновит игру и закроется.", "Without Steam - Vaporust downloads updates, files of another language or beta (chosen in \"Properties\") itself; Steam is not started - lighter for an old PC. Game settings are kept. Via Steam - as before: a hidden Steam updates the game and closes."),
                );

                row(ui, tr!("Экономия памяти", "Memory saving"), |ui| {
                    ui.add_enabled(
                        !self.launching.load(std::sync::atomic::Ordering::Relaxed),
                        egui::Checkbox::new(&mut self.freeze_webhelper, tr!("Замораживать steamwebhelper", "Freeze steamwebhelper")),
                    );
                });
                note(
                    ui,
                    tr!("Через 20 с после старта игры или обновления приостанавливает встроенный браузер Steam \
                     и сбрасывает его память на диск, потом размораживает. Пока он заморожен, оверлей Steam (Shift+Tab), настройка геймпада через Steam Input и покупки внутри игры могут не открыться - тогда выключите.", "20 s after a game or update starts, it suspends the Steam built-in browser and moves its memory to disk, then unfreezes it afterwards. While it is frozen, the Steam overlay (Shift+Tab), gamepad setup via Steam Input and in-game purchases may not open - then turn this off."),
                );

                row(ui, tr!("Флаги скрытого Steam", "Hidden Steam flags"), |ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.settings.steam_args).desired_width(330.0));
                    let is_default = self.settings.steam_args.trim() == local::DEFAULT_STEAM_ARGS;
                    if ui.add_enabled(!is_default, egui::Button::new(tr!("По умолчанию", "Default"))).clicked() {
                        self.settings.steam_args = local::DEFAULT_STEAM_ARGS.to_string();
                    }
                });
                note(
                    ui,
                    tr!("-silent - без окна; -no-browser и -cef-disable-gpu - облегченный встроенный браузер; \
                     -nofriendsui - без окна друзей Steam. Действует со следующего запуска Steam.", "-silent - no window; -no-browser and -cef-disable-gpu - a lighter built-in browser; -nofriendsui - no Steam friends window. Applies from the next Steam start."),
                );
            });

            // ---------- Performance ----------
            card(ui, tr!("Производительность", "Performance"), |ui| {
                row(ui, tr!("Легкий режим", "Light mode"), |ui| {
                    if ui.checkbox(&mut self.settings.light_mode, tr!("Меньше фоновой работы (для слабых ПК)", "Less background work (for weak PCs)")).changed()
                        && self.settings.light_mode
                    {
                        self.settings.animations = false;
                        self.settings.economy = true;
                        self.settings.trim_memory = true;
                        ui.ctx().forget_all_images();
                        self.covers.clear();
                    }
                });
                note(
                    ui,
                    tr!("Окно перерисовывается реже, статусы друзей обновляются раз в 15 с, связь со Steam проверяется \
                     раз в минуту, уведомления Steam - раз в 30 минут. Не загружаются полка «Что нового» и \
                     активность друзей на странице игры. Включение также выключает анимации и включает экономию памяти.", "The window repaints less often, friend statuses refresh every 15 s, the Steam connection is checked once a minute, Steam notifications every 30 minutes. The \"What's new\" shelf and friends activity on the game page are not loaded. Turning it on also turns animations off and memory saving on."),
                );

                row(ui, tr!("Анимации", "Animations"), |ui| {
                    ui.checkbox(&mut self.settings.animations, tr!("Стикеры, GIF, эффекты в чате, значки загрузки", "Stickers, GIFs, chat effects, loading icons"));
                });
                note(ui, tr!("Без анимаций стикеры и GIF показываются первым кадром, эффекты чата сами не запускаются. Меньше работы видеокарте.", "Without animations stickers and GIFs show their first frame, chat effects do not start by themselves. Less work for the graphics card."));

                row(ui, tr!("Обложки игр", "Game covers"), |ui| {
                    ui.checkbox(&mut self.settings.covers, tr!("Обложки, фоны и значки игр в библиотеке", "Covers, backgrounds and game icons in the library"));
                });
                note(ui, tr!("Без обложек библиотека - простой список, а у страницы игры нет большого фона. Так меньше всего памяти.", "Without covers the library is a simple list, and the game page has no big background. This uses the least memory."));

                row(ui, tr!("Экономия памяти", "Memory saving"), |ui| {
                    if ui.checkbox(&mut self.settings.economy, tr!("Картинки мельче, кэш картинок меньше", "Smaller images, smaller image cache")).changed() {
                        ui.ctx().forget_all_images();
                        self.covers.clear();
                    }
                });
                note(ui, tr!("Картинки в магазине, Мастерской и профилях хранятся уменьшенными и чаще выгружаются из памяти.", "Images in the store, Workshop and profiles are kept smaller and unloaded from memory more often."));

                row(ui, tr!("Память в фоне", "Memory in the background"), |ui| {
                    ui.checkbox(&mut self.settings.trim_memory, tr!("Отдавать память системе в трее и во время игры", "Give memory back to the system in the tray and during games"));
                });
                note(ui, tr!("Когда Vaporust свернут в трей или идет игра, он выгружает картинки и отдает свою память Windows: \
                     ее получит игра. При возврате в окно картинки загрузятся снова.", "When Vaporust is in the tray or a game runs, it unloads images and gives its memory back to Windows: the game gets it. When you return to the window, images load again."));

                row(ui, tr!("Сейчас", "Now"), |ui| {
                    let (ws, _) = crate::stats::memory();
                    ui.label(trf!(
                        "Занято {:.0} МБ (как в Диспетчере задач), из них картинки {:.0} МБ", "Using {:.0} MB (as in Task Manager), images {:.0} MB of it",
                        ws as f64 / 1048576.0,
                        crate::imgload::cache_bytes(ui.ctx()) as f64 / 1048576.0
                    ));
                    if ui.button(tr!("Освободить", "Free up")).on_hover_text(tr!("Выгрузить картинки и отдать память системе", "Unload images and give memory back to the system")).clicked() {
                        ui.ctx().forget_all_images();
                        self.covers.clear();
                        crate::imgload::trim_working_set();
                    }
                });
                note(ui, tr!("Часть памяти занимает сам драйвер видеокарты (на ПК с видеокартой NVIDIA до 30-40 МБ даже у пустого окна).", "Part of the memory is taken by the graphics driver itself (on PCs with an NVIDIA card up to 30-40 MB even for an empty window)."));
            });

            // ---------- Window ----------
            card(ui, tr!("Окно", "Window"), |ui| {
                row(ui, tr!("При запуске открывать", "Open at startup"), |ui| {
                    ui.radio_value(&mut self.settings.start_view, 0, tr!("Библиотеку", "Library"));
                    ui.radio_value(&mut self.settings.start_view, 1, tr!("Магазин", "Store"));
                });
                row(ui, tr!("Масштаб интерфейса", "Interface scale"), |ui| {
                    for (v, label) in SCALES.iter().copied().zip(["75%", "90%", "100%", "110%", "125%", "150%", "200%"]) {
                        let on = (self.settings.ui_scale - v).abs() < 0.01;
                        if ui.selectable_label(on, label).clicked() {
                            self.settings.ui_scale = v;
                        }
                    }
                });
                note(ui, tr!("Размер текста и кнопок во всех окнах. Также Ctrl+плюс / Ctrl+минус, Ctrl+0 - вернуть 100%.", "Size of text and buttons in all windows. Also Ctrl+plus / Ctrl+minus, Ctrl+0 - back to 100%."));
                row(ui, tr!("Рамка окон", "Window frame"), |ui| {
                    ui.radio_value(&mut self.settings.system_frame, false, tr!("Своя (Vaporust)", "Own (Vaporust)"));
                    ui.radio_value(&mut self.settings.system_frame, true, tr!("Обычная Windows", "Normal Windows"));
                });
                note(ui, tr!("Своя рамка - темный заголовок в стиле программы. Обычная - стандартная рамка Windows. Меняется сразу.", "Own frame - a dark title bar in the program's style. Normal - the standard Windows frame. Changes right away."));

                row(ui, tr!("Размер и положение", "Size and position"), |ui| {
                    ui.checkbox(&mut self.settings.remember_window, tr!("Запоминать", "Remember"));
                });
                note(ui, tr!("Окно откроется там же и такого же размера, как при закрытии. Если тот монитор отключен или окно не помещается, - обычное окно посередине экрана.", "The window opens in the same place and size as when it was closed. If that monitor is disconnected or the window does not fit - a normal window in the middle of the screen."));

                row(ui, tr!("Крестик окна", "Window close button"), |ui| {
                    let mut tray_on = tray::close_to_tray();
                    if ui.checkbox(&mut tray_on, tr!("Сворачивает Vaporust в трей", "Minimizes Vaporust to the tray")).changed() {
                        tray::set_close_to_tray(tray_on);
                    }
                });
                note(ui, tr!("Чат продолжает работать. Вернуть - щелчок по иконке у часов, выйти - правый щелчок -> «Выход».", "Chat keeps working. To bring it back - click the icon near the clock, to quit - right click -> \"Exit\"."));

                row(ui, tr!("Автозапуск", "Autostart"), |ui| {
                    let mut on = *self.autostart_on.get_or_insert_with(crate::autostart::enabled);
                    if ui.checkbox(&mut on, tr!("Запускать вместе с Windows (сразу в трей)", "Start with Windows (straight to the tray)")).changed() {
                        match crate::autostart::set(on) {
                            Ok(()) => self.autostart_on = Some(on),
                            Err(e) => {
                                self.autostart_on = None;
                                ui.colored_label(egui::Color32::from_rgb(230, 110, 110), e);
                            }
                        }
                    }
                });
                note(
                    ui,
                    tr!("Удобно для уведомлений и расписания загрузок. Если перенесете vaporust.exe в другую папку - \
                     включите галочку заново.", "Handy for notifications and the download schedule. If you move vaporust.exe to another folder, turn the checkbox on again."),
                );

                row(ui, tr!("Язык интерфейса", "Interface language"), |ui| {
                    // Language names are written in their own language: easy to find even in an unknown one.
                    let auto = tr!("Как в Windows", "As in Windows");
                    let current = match self.settings.language.as_str() {
                        "ru" => "Русский",
                        "en" => "English",
                        _ => auto,
                    };
                    egui::ComboBox::from_id_salt("settings_lang").selected_text(current).show_ui(ui, |ui| {
                        for (code, label) in [("", auto), ("ru", "Русский"), ("en", "English")] {
                            if ui.selectable_label(self.settings.language == code, label).clicked() {
                                self.settings.language = code.to_string();
                                crate::lang::set_en(crate::lang::resolve(code));
                            }
                        }
                    });
                });
                note(
                    ui,
                    tr!(
                        "Меняется сразу. Тексты Steam (описания, новости) приходят на выбранном языке после обновления раздела.",
                        "Changes right away. Steam texts (descriptions, news) come in the chosen language after the section reloads."
                    ),
                );
            });

            // ---------- Notifications ----------
            card(ui, tr!("Уведомления", "Notifications"), |ui| {
                row(ui, tr!("Уведомления", "Notifications"), |ui| {
                    ui.checkbox(&mut self.settings.notify_on, tr!("Включены", "On"));
                    ui.add_enabled(self.settings.notify_on, egui::Checkbox::new(&mut self.settings.notify_sound, tr!("Со звуком", "With sound")));
                    if ui.small_button(tr!("Проверить", "Test")).on_hover_text(tr!("Показать пробное уведомление", "Show a test notification")).clicked() {
                        crate::popup::push(crate::popup::Kind::Info, "Vaporust", tr!("Так будут выглядеть уведомления.", "This is how notifications will look."));
                    }
                });
                note(
                    ui,
                    tr!("Небольшие карточки в правом нижнем углу экрана на 6 секунд (пока над карточкой мышь - не \
                     исчезают). Щелчок открывает чат с другом или страницу игры, крестик - закрыть. У каждого вида \
                     свой звук. Работают и когда Vaporust свернут в трей.", "Small cards in the lower right corner of the screen for 6 seconds (they stay while the mouse is over a card). A click opens the chat with the friend or the game page, the cross closes it. Each kind has its own sound. They work when Vaporust is in the tray too."),
                );
                let s = &mut self.settings;
                ui.add_enabled_ui(s.notify_on, |ui| {
                    row(ui, tr!("Сообщения от друзей", "Messages from friends"), |ui| {
                        ui.checkbox(&mut s.notify_msg, tr!("Новое сообщение", "New message"));
                    });
                    row(ui, tr!("Друзья", "Friends"), |ui| {
                        ui.checkbox(&mut s.notify_friend_game, tr!("Друг зашел в игру", "A friend started a game"));
                        ui.checkbox(&mut s.notify_friend_online, tr!("Друг появился в сети", "A friend came online"));
                    });
                    row(ui, tr!("Магазин", "Store"), |ui| {
                        ui.checkbox(&mut s.notify_wishlist, tr!("Скидка на игру из списка желаемого", "A discount on a wishlist game"));
                    });
                    row(ui, tr!("Загрузки", "Downloads"), |ui| {
                        ui.checkbox(&mut s.notify_download, tr!("Игра скачана", "Game downloaded"));
                        ui.checkbox(&mut s.notify_steam, tr!("Уведомления Steam (подарки, предметы, обмены)", "Steam notifications (gifts, items, trades)"));
                    });
                });
                note(
                    ui,
                    tr!("О сообщении - если не открыт чат именно с этим другом. Скидки проверяются при входе и раз в 6 часов, о каждой скидке - один раз.", "About a message - unless the chat with exactly this friend is open. Discounts are checked at login and every 6 hours, about each discount - once."),
                );
            });

            // ---------- Network ----------
            card(ui, tr!("Сеть", "Network"), |ui| {
                row(ui, tr!("Прокси", "Proxy"), |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.settings.proxy)
                            .hint_text(tr!("http://хост:порт или socks5://хост:порт", "http://host:port or socks5://host:port"))
                            .desired_width(300.0),
                    );
                    if ui.button(tr!("Применить", "Apply")).clicked() {
                        self.send_aurelia_config();
                        self.proxy_note = true;
                    }
                });
                note(
                    ui,
                    if self.proxy_note {
                        tr!("Сохранено. Действует после перезапуска Vaporust.", "Saved. Applies after restarting Vaporust.")
                    } else {
                        tr!("Для запросов магазина, загрузок и сообщества. Пусто - без прокси. Подключение к самому \
                         Steam (вход, друзья, чат) через прокси не идет.", "For store, download and community requests. Empty - no proxy. The connection to Steam itself (login, friends, chat) does not go through the proxy.")
                    },
                );
            });

            // ---------- About ----------
            // (The price region is chosen in the store itself.)
            card(ui, tr!("О программе", "About"), |ui| {
                row(ui, tr!("Версия", "Version"), |ui| {
                    ui.label(format!("Vaporust {}{}", env!("CARGO_PKG_VERSION"), tr!(" (альфа)", " (alpha)")));
                    ui.label(if cfg!(target_pointer_width = "64") { tr!("64 бит", "64-bit") } else { tr!("32 бит", "32-bit") });
                    ui.hyperlink_to("GitHub", GITHUB);
                });
                row(ui, tr!("Обновления", "Updates"), |ui| {
                    ui.checkbox(&mut self.settings.check_updates, tr!("Проверять при запуске", "Check at startup"));
                    let checking = matches!(*self.upd.lock().unwrap(), crate::updater::State::Checking);
                    if ui.add_enabled(!checking, egui::Button::new(tr!("Проверить сейчас", "Check now"))).clicked() {
                        self.check_updates_now(ui.ctx());
                    }
                    if checking {
                        crate::spin::spinner(ui);
                    }
                });
                note(ui, tr!("Новые версии берутся со страницы Releases на GitHub; установка - одной кнопкой с перезапуском.", "New versions come from the Releases page on GitHub; installing is one button with a restart."));
                row(ui, tr!("Папки", "Folders"), |ui| {
                    if let Some(s) = &self.steam {
                        if ui.button("Steam").clicked() {
                            local::open_folder(&s.dir);
                        }
                    }
                    let dir = if portable::enabled() { portable::data_dir() } else { portable::normal_dir() };
                    if let Some(dir) = dir {
                        // Root folder (settings, cache, backups) and the program data folder.
                        if let Some(root) = dir.parent().filter(|r| r.ends_with("Vaporust")) {
                            if ui.button("Vaporust").on_hover_text(root.display().to_string()).clicked() {
                                local::open_folder(root);
                            }
                        }
                    }
                    let saves = crate::backup::root();
                    if saves.is_dir() && ui.button(tr!("Копии сохранений", "Save backups")).on_hover_text(saves.display().to_string()).clicked() {
                        local::open_folder(&saves);
                    }
                });
                row(ui, tr!("Ошибки", "Errors"), |ui| {
                    if let Some(p) = crate::errlog::path() {
                        if ui.button(tr!("Журнал", "Log")).on_hover_text(tr!("Ошибки Vaporust (без логина и SteamID)", "Vaporust errors (without login and SteamID)")).clicked() {
                            if !p.exists() {
                                crate::errlog::write(tr!("журнал", "log"), tr!("журнал создан", "log created"));
                            }
                            let _ = std::process::Command::new("notepad.exe").arg(&p).spawn();
                        }
                    }
                    if ui
                        .button(tr!("Скопировать диагностику", "Copy diagnostics"))
                        .on_hover_text(tr!("Версия, Windows, размер окна и масштаб, память и последние записи журнала - для сообщения об ошибке. Без логина, SteamID и ключей входа.", "Version, Windows, window size and scale, memory and the last log entries - for a bug report. Without login, SteamID and login keys."))
                        .clicked()
                    {
                        let c = ui.ctx().clone();
                        let (screen, inner, ppp) = c.input(|i| (i.viewport().monitor_size, i.viewport().inner_rect, i.pixels_per_point()));
                        let size = |v: Option<egui::Vec2>| v.map(|v| format!("{:.0}x{:.0}", v.x * ppp, v.y * ppp)).unwrap_or_else(|| "?".into());
                        let (taskmgr, _) = crate::stats::memory();
                        let text = crate::errlog::diagnostics(&[
                            (tr!("Экран (пикс.)", "Screen (px)"), size(screen)),
                            (tr!("Окно (пикс.)", "Window (px)"), size(inner.map(|r| r.size()))),
                            (tr!("Масштаб интерфейса", "Interface scale"), trf!("{:.0}% (точек на пиксель {:.2})", "{:.0}% ({:.2} points per pixel)", self.settings.ui_scale * 100.0, ppp)),
                            (tr!("Рамка окон", "Window frame"), if self.settings.system_frame { "Windows" } else { tr!("своя", "own") }.into()),
                            (tr!("Память (Диспетчер задач)", "Memory (Task Manager)"), trf!("{:.0} МБ", "{:.0} MB", taskmgr as f64 / 1048576.0)),
                            (tr!("Портативный режим", "Portable mode"), crate::portable::enabled().to_string()),
                            (tr!("Вход зашифрован", "Login encrypted"), format!("{:?}", net.session_secure)),
                            (tr!("Нет связи со Steam", "No connection to Steam"), net.offline.to_string()),
                            (tr!("Игр с запуском без Steam", "Games launched without Steam"), self.settings.no_steam.len().to_string()),
                        ]);
                        c.copy_text(text);
                        crate::popup::push(crate::popup::Kind::Info, tr!("Диагностика", "Diagnostics"), tr!("Скопировано: вставьте в сообщение об ошибке (Ctrl+V).", "Copied: paste it into the bug report (Ctrl+V)."));
                    }
                    if ui
                        .button(tr!("Сообщить об ошибке", "Report a bug"))
                        .on_hover_text(tr!("Страница GitHub: опишите, что случилось, и приложите журнал", "GitHub page: describe what happened and attach the log"))
                        .clicked()
                    {
                        let title = tr!("Ошибка в Vaporust", "Bug in Vaporust");
                        let body = trf!(
                            "Версия: {} ({})%0A%0AЧто случилось:%0A%0AЧто делали перед этим:%0A%0A(вставьте диагностику: Настройки -> О программе -> Скопировать диагностику)", "Version: {} ({})%0A%0AWhat happened:%0A%0AWhat you did before:%0A%0A(paste the diagnostics: Settings -> About -> Copy diagnostics)",
                            env!("CARGO_PKG_VERSION"),
                            if cfg!(target_pointer_width = "64") { tr!("64 бит", "64-bit") } else { tr!("32 бит", "32-bit") }
                        );
                        let url = format!("{GITHUB}/issues/new?title={title}&body={body}").replace(' ', "%20");
                        ui.ctx().open_url(egui::OpenUrl::new_tab(url));
                    }
                });
                self.restore_row(ui, net);
                ui.add_space(4.0);
                ui.weak(tr!("Неофициальный проект, не связан с Valve. Лицензия GPL-3.0-or-later. Основан на Aurelia (Drackrath).", "An unofficial project, not affiliated with Valve. License GPL-3.0-or-later. Based on Aurelia (Drackrath)."));
                ui.add_space(12.0);
                ui.label(egui::RichText::new(tr!("Благодарности", "Credits")).size(16.0).strong());
                ui.weak(tr!("Vaporust стал возможен благодаря работе этих людей и проектов. Спасибо!", "Vaporust is possible thanks to the work of these people and projects. Thank you!"));
                ui.add_space(4.0);
                let thanks: [(&str, &str, &str); 9] = [
                    ("Aurelia", "https://github.com/Drackrath/Aurelia", tr!("Drackrath: основа Vaporust - вход, загрузка игр, друзья, чат, достижения", "Drackrath: the base of Vaporust - login, game downloads, friends, chat, achievements")),
                    ("SteamFlow", "https://github.com/weter11/SteamFlow", tr!("weter11: проект, из которого выросла Aurelia", "weter11: the project Aurelia grew from")),
                    ("steam-vent", "https://codeberg.org/steam-vent/steam-vent", tr!("разбор и реализация сетевого протокола Steam", "analysis and implementation of the Steam network protocol")),
                    ("OpenSteamClient", "https://github.com/OpenSteamClient/OpenSteamClient", tr!("Rosentti и участники: открытый клиент Steam, один из вдохновителей", "Rosentti and contributors: an open Steam client, one of the inspirations")),
                    ("SteamDatabase / Protobufs", "https://github.com/SteamDatabase/Protobufs", tr!("описания сообщений Steam", "Steam message definitions")),
                    ("SteamKit2", "https://github.com/SteamRE/SteamKit", tr!("исследования протокола Steam", "Steam protocol research")),
                    ("node-steamcommunity", "https://github.com/DoctorMcKay/node-steamcommunity", tr!("DoctorMcKay: как отправлять картинки и стикеры в чат", "DoctorMcKay: how to send images and stickers to the chat")),
                    ("egui / eframe", "https://github.com/emilk/egui", tr!("emilk: библиотека интерфейса", "emilk: the interface library")),
                    (tr!("Rust и библиотеки", "Rust and libraries"), tr!("https://github.com/felixsmtnk/Vaporust#использованные-проекты", "https://github.com/felixsmtnk/Vaporust#credits"), tr!("tokio, image, windows-rs и другие - полный список в README", "tokio, image, windows-rs and others - the full list is in the README")),
                ];
                for (name, url, what) in thanks {
                    ui.horizontal_wrapped(|ui| {
                        ui.hyperlink_to(name, url);
                        ui.weak(what);
                    });
                }
            });
        });
        });
    }
}

/// KB/s -> "1.5" (MB/s, without trailing zeros).
fn fmt_mb(kb: u64) -> String {
    let s = format!("{:.2}", kb as f64 / 1000.0);
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// Field text (MB/s, a comma is allowed) -> KB/s. Empty = 0. None - not a number.
fn parse_mb(text: &str) -> Option<u64> {
    let t = text.trim().replace(',', ".");
    if t.is_empty() {
        return Some(0);
    }
    let v: f64 = t.parse().ok()?;
    if !v.is_finite() || v < 0.0 {
        return None;
    }
    Some((v * 1000.0).round() as u64)
}

#[cfg(test)]
mod tests {
    #[test]
    fn mb_field() {
        assert_eq!(super::parse_mb(""), Some(0));
        assert_eq!(super::parse_mb("1,5"), Some(1500));
        assert_eq!(super::parse_mb(" 0.25 "), Some(250));
        assert_eq!(super::parse_mb("abc"), None);
        assert_eq!(super::parse_mb("-1"), None);
        assert_eq!(super::fmt_mb(1000), "1");
        assert_eq!(super::fmt_mb(1500), "1.5");
        assert_eq!(super::fmt_mb(250), "0.25");
    }
}

/// "__ MB/s" field + "Apply". Returns true when the value is applied.
fn limit_field(ui: &mut egui::Ui, buf: &mut Option<String>, value: &mut u64) -> bool {
    let kb = *value;
    let text = buf.get_or_insert_with(|| if kb == 0 { String::new() } else { fmt_mb(kb) });
    let resp = ui.add(egui::TextEdit::singleline(text).desired_width(70.0).hint_text(tr!("нет", "none")));
    ui.label(tr!("МБ/с", "MB/s"));
    let parsed = parse_mb(text);
    let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    let changed = parsed.is_some_and(|v| v != kb);
    let mut applied = false;
    if ui.add_enabled(changed, egui::Button::new(tr!("Применить", "Apply"))).clicked() || (enter && changed) {
        *value = parsed.unwrap_or(0);
        applied = true;
    }
    if parsed.is_none() {
        ui.colored_label(egui::Color32::from_rgb(240, 120, 100), tr!("введите число, например 2 или 0.5", "enter a number, for example 2 or 0.5"));
    } else if *value == 0 {
        ui.weak(tr!("сейчас без ограничения", "no limit now"));
    } else {
        ui.weak(trf!("сейчас до {} МБ/с", "now up to {} MB/s", fmt_mb(*value)));
    }
    applied
}

/// Choosing an hour "01:00". Returns true on change.
fn hour_box(ui: &mut egui::Ui, id: &str, h: &mut u8) -> bool {
    let before = *h;
    egui::ComboBox::from_id_salt(id).width(64.0).selected_text(crate::schedule::hh(*h)).show_ui(ui, |ui| {
        for x in 0..24u8 {
            ui.selectable_value(h, x, crate::schedule::hh(x));
        }
    });
    *h != before
}

/// Interface scale steps (settings, wizard, Ctrl+plus/minus).
pub const SCALES: [f32; 7] = [0.75, 0.9, 1.0, 1.1, 1.25, 1.5, 2.0];
