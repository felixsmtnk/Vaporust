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
    key.trim().to_uppercase().split_whitespace().collect::<Vec<_>>().join("")
}

/// Похоже ли на ключ Steam (5-5-5 или 5-5-5-5-5 латиница и цифры).
pub fn looks_like_key(key: &str) -> bool {
    let parts: Vec<&str> = key.split('-').collect();
    (parts.len() == 3 || parts.len() == 5)
        && parts.iter().all(|p| p.len() == 5 && p.chars().all(|c| c.is_ascii_alphanumeric()))
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
        return KeyResult::Err("Это не похоже на ключ Steam (вид XXXXX-XXXXX-XXXXX).".into());
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
        assert!(!looks_like_key("AAAA-BBBBB-CCCCC"));
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
            crate::socialwin::viewport("Активировать ключ - Vaporust", [460.0, 240.0], [380.0, 200.0]),
            |ctx, _| {
                crate::winframe::title_bar_auto(ctx);
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.label("Ключ продукта Steam (например, из магазина или бандла):");
                    ui.horizontal(|ui| {
                        let edit = ui.add(
                            egui::TextEdit::singleline(&mut input).desired_width(280.0).hint_text("XXXXX-XXXXX-XXXXX"),
                        );
                        let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        let ok = !net.key_busy && !input.trim().is_empty();
                        if (ui.add_enabled(ok, egui::Button::new("Активировать")).clicked() || (enter && ok))
                            && net.account.is_some()
                        {
                            send = Some(input.clone());
                        }
                    });
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
