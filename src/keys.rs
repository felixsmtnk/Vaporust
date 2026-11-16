//! Steam product key activation (like "Games -> Activate a Product on Steam...").
//! The same request the page store.steampowered.com/account/registerkey makes.

use aurelia::steam_client::SteamClient;

/// Activation result.
#[derive(Clone, Debug, PartialEq)]
pub enum KeyResult {
    /// Activated: what was added to the account.
    Ok(Vec<String>),
    Err(String),
}

/// Normalize a key to XXXXX-XXXXX-XXXXX (spaces and case do not matter).
pub fn normalize(key: &str) -> String {
    key.trim().to_uppercase().split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Whether the key can be sent: not empty and only Latin letters, digits, dash and space. No
/// stricter check - Steam validates the key itself anyway.
pub fn looks_like_key(key: &str) -> bool {
    let k = key.trim();
    !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == ' ')
}

/// Clear explanation of a Steam refusal code (purchase_result_details).
pub fn explain(code: i64) -> String {
    match code {
        9 => tr!("Этот продукт уже есть на вашем аккаунте.", "This product is already on your account.").into(),
        13 => tr!("Ключ нельзя активировать в вашем регионе.", "The key cannot be activated in your region.").into(),
        14 => tr!("Неверный ключ: проверьте, что он введен без ошибок.", "Invalid key: check that it was entered without mistakes.").into(),
        15 => tr!("Ключ уже активирован на другом аккаунте.", "The key is already activated on another account.").into(),
        24 => tr!("Сначала нужна основная игра: это дополнение к ней.", "The base game is needed first: this is its DLC.").into(),
        36 | 50 => tr!("Это код для кошелька или другой платформы: активируйте его на странице кошелька Steam.", "This is a code for the wallet or another platform: redeem it on the Steam wallet page.").into(),
        53 => tr!("Слишком много попыток. Подождите час и попробуйте снова.", "Too many attempts. Wait an hour and try again.").into(),
        c => trf!("Steam отказал (код {c}).", "Steam refused (code {c})."),
    }
}

/// Parse the ajaxregisterkey response.
pub fn parse(body: &str) -> KeyResult {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(body) else {
        return KeyResult::Err(tr!("Steam ответил непонятно (сессия устарела? войдите заново).", "Steam gave an unclear answer (session expired? log in again).").into());
    };
    let names: Vec<String> = v
        .get("purchase_receipt_info")
        .and_then(|r| r.get("line_items"))
        .and_then(|l| l.as_array())
        .map(|a| a.iter().filter_map(|i| i.get("line_item_description")?.as_str().map(String::from)).collect())
        .unwrap_or_default();
    if v.get("success").and_then(|s| s.as_i64()) == Some(1) {
        return KeyResult::Ok(names);
    }
    let code = v.get("purchase_result_details").and_then(|c| c.as_i64()).unwrap_or(0);
    KeyResult::Err(explain(code))
}

pub async fn activate(client: &SteamClient, key: &str) -> KeyResult {
    let key = normalize(key);
    if !looks_like_key(&key) {
        return KeyResult::Err(tr!("В ключе Steam только латинские буквы, цифры и дефисы.", "A Steam key has only Latin letters, digits and dashes.").into());
    }
    let web = match crate::webapi::web_session(client).await {
        Ok(w) => w,
        Err(e) => return KeyResult::Err(trf!("Нет входа на сайт Steam: {e}", "Not logged in to the Steam website: {e}")),
    };
    let fields = [("product_key", key.clone()), ("sessionid", web.sessionid().to_string())];
    match web
        .post_form(
            "https://store.steampowered.com/account/ajaxregisterkey/",
            &fields,
            "https://store.steampowered.com/account/registerkey",
        )
        .await
    {
        Ok((200, body)) => parse(&body),
        Ok((status, _)) => KeyResult::Err(trf!("Steam ответил кодом {status} (войдите заново?).", "Steam answered with code {status} (log in again?).")),
        Err(e) => KeyResult::Err(trf!("Нет связи со Steam: {e:#}", "No connection to Steam: {e:#}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys() {
        assert_eq!(normalize(" aaaaa-bbbbb-c1ccc \n"), "AAAAA-BBBBB-C1CCC");
        assert!(looks_like_key("AAAAA-BBBBB-C1CCC"));
        assert!(looks_like_key("AAAAA-BBBBB-CCCCC-DDDDD-EEEEE"));
        assert!(!looks_like_key("АААА-BBBBB-CCCCC")); // Cyrillic
        assert_eq!(
            parse(r#"{"success":1,"purchase_receipt_info":{"line_items":[{"packageid":1,"line_item_description":"Half-Life"}]}}"#),
            KeyResult::Ok(vec!["Half-Life".into()])
        );
        assert!(matches!(parse(r#"{"success":2,"purchase_result_details":9}"#), KeyResult::Err(e) if e.contains("уже есть")));
    }

    /// Live test: a deliberately wrong key (one failed attempt). Expected "Неверный ключ".
    #[test]
    #[ignore]
    fn activate_invalid_live() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let r = activate(&c, "AAAAA-BBBBB-CCCCC").await;
            println!("{r:?}");
        });
    }
}


#[cfg(test)]
mod key_format_tests {
    use super::{looks_like_key, normalize};

    #[test]
    fn formats() {
        assert!(looks_like_key("AAAAA-BBBBB-CCCCC"));
        assert!(looks_like_key("dfgdfg435"));
        assert!(looks_like_key(&normalize(" 237abcdghjlprst   23 ")));
        assert_eq!(normalize(" 237abcdghjlprst   23 "), "237ABCDGHJLPRST 23");
        assert!(!looks_like_key("2342342фываываыва"));
        assert!(!looks_like_key("   "));
    }
}

use crate::net::{Cmd, NetState};
use crate::LauncherApp;
use eframe::egui;

impl LauncherApp {
    /// "Activate a key" window (Account -> Activate a key).
    pub(crate) fn key_window(&mut self, ctx: &egui::Context, net: &NetState) {
        let Some(mut input) = self.key_input.take() else { return };
        let mut open = true;
        let mut send = None;
        let mut refresh = false;
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("vaporust_key"),
            crate::socialwin::viewport(tr!("Активировать ключ - Vaporust", "Activate a key - Vaporust"), [520.0, 340.0], [420.0, 300.0]),
            |ctx, _| {
                crate::winframe::title_bar_auto(ctx);
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.label(egui::RichText::new(tr!("Введите ключ активации", "Enter the activation key")).size(18.0).strong());
                    ui.add_space(4.0);
                    ui.label(
                        tr!("Ключ добавит продукт в вашу библиотеку Steam. Подойдет ключ с коробки, \
                         из магазина или бандла.", "The key adds a product to your Steam library. A key from a box, a store or a bundle works."),
                    );
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new(tr!("ПРИМЕРЫ КЛЮЧЕЙ АКТИВАЦИИ", "PRODUCT KEY EXAMPLES")).size(11.0).strong().color(crate::theme::WEAK));
                    for ex in ["AAAAA-BBBBB-CCCCC", "AAAAA-BBBBB-CCCCC-DDDDD-EEEEE", "237ABCDGHJLPRST 23"] {
                        ui.label(egui::RichText::new(ex).monospace().color(crate::theme::WEAK));
                    }
                    ui.add_space(8.0);
                    let valid = looks_like_key(&input);
                    ui.horizontal(|ui| {
                        let edit = ui.add(
                            egui::TextEdit::singleline(&mut input)
                                .desired_width(ui.available_width() - 110.0)
                                .hint_text("AAAAA-BBBBB-CCCCC"),
                        );
                        let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        let ok = !net.key_busy && valid && net.account.is_some();
                        if ui.add_enabled(ok, egui::Button::new(tr!("Активировать", "Activate"))).clicked() || (enter && ok) {
                            send = Some(normalize(&input));
                        }
                    });
                    if !input.trim().is_empty() && !valid {
                        ui.colored_label(
                            egui::Color32::from_rgb(230, 170, 90),
                            tr!("В ключе Steam только латинские буквы, цифры и дефисы.", "A Steam key has only Latin letters, digits and dashes."),
                        );
                    }
                    if net.account.is_none() {
                        ui.weak(tr!("Нужен вход в аккаунт.", "An account login is needed."));
                    }
                    if net.key_busy {
                        ui.horizontal(|ui| {
                            crate::spin::spinner(ui);
                            ui.label(tr!("Активирую...", "Activating..."));
                        });
                    }
                    match &net.key_result {
                        Some(KeyResult::Ok(items)) => {
                            let what = if items.is_empty() { tr!("продукт", "product").to_string() } else { items.join(", ") };
                            ui.colored_label(egui::Color32::from_rgb(110, 200, 110), trf!("Готово: на аккаунт добавлено {what}.", "Done: {what} added to the account."));
                            refresh = true;
                        }
                        Some(KeyResult::Err(e)) => {
                            ui.colored_label(egui::Color32::from_rgb(230, 110, 110), e);
                        }
                        None => {}
                    }
                    ui.add_space(6.0);
                    ui.weak(tr!("Ключ проверяет сам Steam. После нескольких неверных ключей подряд Steam на час запрещает попытки.", "Steam checks the key itself. After several invalid keys in a row Steam blocks attempts for an hour."));
                });
                if ctx.input(|i| i.viewport().close_requested()) {
                    open = false;
                }
            },
        );
        if let Some(k) = send {
            let _ = self.tx.send(Cmd::ActivateKey(k));
        }
        // A new game on the account - refresh the library once.
        if refresh && !self.key_refreshed {
            self.key_refreshed = true;
            let _ = self.tx.send(Cmd::RefreshLibrary);
        }
        if open {
            self.key_input = Some(input);
        }
    }
}
