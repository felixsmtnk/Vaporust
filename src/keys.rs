//! Активация ключа продукта Steam (как «Игры -> Активировать в Steam...»).
//! Тот же запрос, что делает страница store.steampowered.com/account/registerkey.

use aurelia::steam_client::SteamClient;

/// Итог активации.
#[derive(Clone, Debug, PartialEq)]
pub enum KeyResult {
    /// Активирован: что добавилось на аккаунт.
    Ok(Vec<String>),
    Err(String),
}

/// Привести ключ к виду XXXXX-XXXXX-XXXXX (пробелы и регистр не важны).
pub fn normalize(key: &str) -> String {
    key.trim().to_uppercase().split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Можно ли отправлять ключ: не пустой и только латиница, цифры, дефис и пробел. Строже не
/// проверяем - правильность ключа все равно проверит сам Steam.
pub fn looks_like_key(key: &str) -> bool {
    let k = key.trim();
    !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == ' ')
}

/// Понятное объяснение кода отказа Steam (purchase_result_details).
pub fn explain(code: i64) -> String {
    match code {
        9 => "Этот продукт уже есть на вашем аккаунте.".into(),
        13 => "Ключ нельзя активировать в вашем регионе.".into(),
        14 => "Неверный ключ: проверьте, что он введен без ошибок.".into(),
        15 => "Ключ уже активирован на другом аккаунте.".into(),
        24 => "Сначала нужна основная игра: это дополнение к ней.".into(),
        36 | 50 => "Это код для кошелька или другой платформы: активируйте его на странице кошелька Steam.".into(),
        53 => "Слишком много попыток. Подождите час и попробуйте снова.".into(),
        c => format!("Steam отказал (код {c})."),
    }
}

/// Разобрать ответ ajaxregisterkey.
pub fn parse(body: &str) -> KeyResult {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(body) else {
        return KeyResult::Err("Steam ответил непонятно (сессия устарела? войдите заново).".into());
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
        return KeyResult::Err("В ключе Steam только латинские буквы, цифры и дефисы.".into());
    }
    let web = match crate::webapi::web_session(client).await {
        Ok(w) => w,
        Err(e) => return KeyResult::Err(format!("Нет входа на сайт Steam: {e}")),
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
        Ok((status, _)) => KeyResult::Err(format!("Steam ответил кодом {status} (войдите заново?).")),
        Err(e) => KeyResult::Err(format!("Нет связи со Steam: {e:#}")),
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
        assert!(!looks_like_key("АААА-BBBBB-CCCCC")); // кириллица
        assert_eq!(
            parse(r#"{"success":1,"purchase_receipt_info":{"line_items":[{"packageid":1,"line_item_description":"Half-Life"}]}}"#),
            KeyResult::Ok(vec!["Half-Life".into()])
        );
        assert!(matches!(parse(r#"{"success":2,"purchase_result_details":9}"#), KeyResult::Err(e) if e.contains("уже есть")));
    }

    /// Живой тест: заведомо неверный ключ (одна неудачная попытка). Ожидается «Неверный ключ».
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
    /// Окно «Активировать ключ» (Аккаунт -> Активировать ключ).
    pub(crate) fn key_window(&mut self, ctx: &egui::Context, net: &NetState) {
        let Some(mut input) = self.key_input.take() else { return };
        let mut open = true;
        let mut send = None;
        let mut refresh = false;
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("vaporust_key"),
            crate::socialwin::viewport("Активировать ключ - Vaporust", [520.0, 340.0], [420.0, 300.0]),
            |ctx, _| {
                crate::winframe::title_bar_auto(ctx);
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.label(egui::RichText::new("Введите ключ активации").size(18.0).strong());
                    ui.add_space(4.0);
                    ui.label(
                        "Ключ добавит продукт в вашу библиотеку Steam. Подойдет ключ с коробки, \
                         из магазина или бандла.",
                    );
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new("ПРИМЕРЫ КЛЮЧЕЙ АКТИВАЦИИ").size(11.0).strong().color(crate::theme::WEAK));
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
                        if ui.add_enabled(ok, egui::Button::new("Активировать")).clicked() || (enter && ok) {
                            send = Some(normalize(&input));
                        }
                    });
                    if !input.trim().is_empty() && !valid {
                        ui.colored_label(
                            egui::Color32::from_rgb(230, 170, 90),
                            "В ключе Steam только латинские буквы, цифры и дефисы.",
                        );
                    }
                    if net.account.is_none() {
                        ui.weak("Нужен вход в аккаунт.");
                    }
                    if net.key_busy {
                        ui.horizontal(|ui| {
                            crate::spin::spinner(ui);
                            ui.label("Активирую...");
                        });
                    }
                    match &net.key_result {
                        Some(KeyResult::Ok(items)) => {
                            let what = if items.is_empty() { "продукт".to_string() } else { items.join(", ") };
                            ui.colored_label(egui::Color32::from_rgb(110, 200, 110), format!("Готово: на аккаунт добавлено {what}."));
                            refresh = true;
                        }
                        Some(KeyResult::Err(e)) => {
                            ui.colored_label(egui::Color32::from_rgb(230, 110, 110), e);
                        }
                        None => {}
                    }
                    ui.add_space(6.0);
                    ui.weak("Ключ проверяет сам Steam. После нескольких неверных ключей подряд Steam на час запрещает попытки.");
                });
                if ctx.input(|i| i.viewport().close_requested()) {
                    open = false;
                }
            },
        );
        if let Some(k) = send {
            let _ = self.tx.send(Cmd::ActivateKey(k));
        }
        // Новая игра на аккаунте - обновить библиотеку один раз.
        if refresh && !self.key_refreshed {
            self.key_refreshed = true;
            let _ = self.tx.send(Cmd::RefreshLibrary);
        }
        if open {
            self.key_input = Some(input);
        }
    }
}
