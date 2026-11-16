//! Editing own profile inside the program (like "Edit profile" in Steam):
//! name, real name, custom URL, "about me", country; avatar; privacy settings.
//! And own nickname for a friend. All actions go through the community site with our web session.

use crate::net::{Cmd, NetState};
use crate::LauncherApp;
use aurelia::steam_client::SteamClient;
use eframe::egui;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Privacy {
    /// 1 - only me, 2 - friends, 3 - everyone.
    pub profile: u8,
    pub inventory: u8,
    pub gifts: u8,
    pub games: u8,
    pub playtime: u8,
    pub friends: u8,
    /// Comments: 1 - everyone, 0 - friends, 2 - nobody (only me).
    pub comments: u8,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProfileEdit {
    pub persona: String,
    pub real_name: String,
    pub custom_url: String,
    pub summary: String,
    pub country: String,
    pub country_name: String,
    pub state: String,
    pub city: String,
    pub avatar_hash: String,
    pub privacy: Privacy,
}

fn unescape_attr(s: &str) -> String {
    s.replace("&quot;", "\"").replace("&lt;", "<").replace("&gt;", ">").replace("&#39;", "'").replace("&amp;", "&")
}

/// Data from the data-profile-edit attribute of the /edit/info page.
pub fn parse(html: &str) -> Option<ProfileEdit> {
    let i = html.find("data-profile-edit=\"")? + "data-profile-edit=\"".len();
    let j = html[i..].find('"')? + i;
    let v: serde_json::Value = serde_json::from_str(&unescape_attr(&html[i..j])).ok()?;
    let s = |k: &str| v[k].as_str().unwrap_or("").to_string();
    let loc = &v["LocationData"];
    let l = |k: &str| loc[k].as_str().unwrap_or("").to_string();
    let p = &v["Privacy"]["PrivacySettings"];
    let n = |k: &str| p[k].as_u64().unwrap_or(3) as u8;
    Some(ProfileEdit {
        persona: s("strPersonaName"),
        real_name: s("strRealName"),
        custom_url: s("strCustomURL"),
        summary: s("strSummary"),
        country: l("locCountryCode"),
        country_name: l("locCountry"),
        state: l("locStateCode"),
        city: l("locCityCode"),
        avatar_hash: s("strAvatarHash"),
        privacy: Privacy {
            profile: n("PrivacyProfile"),
            inventory: n("PrivacyInventory"),
            gifts: n("PrivacyInventoryGifts"),
            games: n("PrivacyOwnedGames"),
            playtime: n("PrivacyPlaytime"),
            friends: n("PrivacyFriendsList"),
            comments: v["Privacy"]["eCommentPermission"].as_u64().unwrap_or(1) as u8,
        },
    })
}

pub async fn load(c: &SteamClient) -> Result<ProfileEdit, String> {
    let me = c.my_steam_id().ok_or("нет входа")?;
    let web = crate::webapi::web_session(c).await?;
    let t = web.get_text(&format!("https://steamcommunity.com/profiles/{me}/edit/info")).await.map_err(|e| format!("{e:#}"))?;
    parse(&t).ok_or_else(|| "Steam не отдал данные профиля.".to_string())
}

fn check(status: u16, body: &str) -> Result<(), String> {
    if status >= 400 {
        return Err(format!("Steam ответил {status}"));
    }
    match serde_json::from_str::<serde_json::Value>(body) {
        Ok(v) if v["success"].as_i64() == Some(1) || v["success"].as_bool() == Some(true) => Ok(()),
        Ok(v) => Err(v["errmsg"].as_str().or(v["message"].as_str()).map(String::from).unwrap_or_else(|| format!("Steam отказал (код {})", v["success"]))),
        Err(_) => Err("Steam ответил не так, как ожидалось".into()),
    }
}

/// Save name, URL, "about me", location.
pub async fn save(c: &SteamClient, e: &ProfileEdit) -> Result<(), String> {
    let me = c.my_steam_id().ok_or("нет входа")?;
    let web = crate::webapi::web_session(c).await?;
    let fields = [
        ("sessionID", web.sessionid().to_string()),
        ("type", "profileSave".into()),
        ("weblink_1_title", String::new()),
        ("weblink_1_url", String::new()),
        ("weblink_2_title", String::new()),
        ("weblink_2_url", String::new()),
        ("weblink_3_title", String::new()),
        ("weblink_3_url", String::new()),
        ("personaName", e.persona.clone()),
        ("real_name", e.real_name.clone()),
        ("customURL", e.custom_url.clone()),
        ("country", e.country.clone()),
        ("state", e.state.clone()),
        ("city", e.city.clone()),
        ("summary", e.summary.clone()),
        ("hide_profile_awards", "0".into()),
        ("json", "1".into()),
    ];
    let page = format!("https://steamcommunity.com/profiles/{me}/edit/info");
    let (st, body) = web.post_form(&format!("https://steamcommunity.com/profiles/{me}/edit/"), &fields, &page).await.map_err(|e| format!("{e:#}"))?;
    check(st, &body)
}

/// Privacy settings.
pub async fn save_privacy(c: &SteamClient, p: &Privacy) -> Result<(), String> {
    let me = c.my_steam_id().ok_or("нет входа")?;
    let web = crate::webapi::web_session(c).await?;
    let privacy = serde_json::json!({
        "PrivacyProfile": p.profile,
        "PrivacyInventory": p.inventory,
        "PrivacyInventoryGifts": p.gifts,
        "PrivacyOwnedGames": p.games,
        "PrivacyPlaytime": p.playtime,
        "PrivacyFriendsList": p.friends,
    });
    let fields = [
        ("sessionid", web.sessionid().to_string()),
        ("Privacy", privacy.to_string()),
        ("eCommentPermission", p.comments.to_string()),
    ];
    let page = format!("https://steamcommunity.com/profiles/{me}/edit/settings");
    let (st, body) = web.post_form(&format!("https://steamcommunity.com/profiles/{me}/ajaxsetprivacy/"), &fields, &page).await.map_err(|e| format!("{e:#}"))?;
    check(st, &body)
}

/// Upload a new avatar (jpg/png, like "Upload avatar" on the site).
pub async fn upload_avatar(c: &SteamClient, path: &std::path::Path) -> Result<(), String> {
    let me = c.my_steam_id().ok_or("нет входа")?;
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    if bytes.len() > 1024 * 1024 {
        return Err("Файл больше 1 МБ: Steam такой аватар не примет.".into());
    }
    let web = crate::webapi::web_session(c).await?;
    let boundary = format!("----vaporust{}", chrono::Utc::now().timestamp_millis());
    let mut body = Vec::new();
    let mut field = |name: &str, value: &str| {
        body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n").as_bytes());
    };
    field("MAX_FILE_SIZE", &(1024 * 1024).to_string());
    field("type", "player_avatar_image");
    field("sId", &me.to_string());
    field("sessionid", web.sessionid());
    field("doSub", "1");
    field("json", "1");
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "avatar.jpg".into());
    let mime = if name.to_lowercase().ends_with(".png") { "image/png" } else { "image/jpeg" };
    body.extend_from_slice(
        format!("--{boundary}\r\nContent-Disposition: form-data; name=\"avatar\"; filename=\"{name}\"\r\nContent-Type: {mime}\r\n\r\n").as_bytes(),
    );
    body.extend_from_slice(&bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    let page = format!("https://steamcommunity.com/profiles/{me}/edit/avatar");
    let (st, text) = web
        .post_raw(
            "https://steamcommunity.com/actions/FileUploader/",
            &format!("multipart/form-data; boundary={boundary}"),
            body,
            &page,
            &[],
        )
        .await
        .map_err(|e| format!("{e:#}"))?;
    check(st, &text)
}

/// Own nickname for a friend (visible only to you), empty - remove.
pub async fn set_nickname(c: &SteamClient, friend: u64, nick: &str) -> Result<(), String> {
    let web = crate::webapi::web_session(c).await?;
    let fields = [("nickname", nick.to_string()), ("sessionid", web.sessionid().to_string())];
    let page = format!("https://steamcommunity.com/profiles/{friend}/");
    let (st, body) = web
        .post_form(&format!("https://steamcommunity.com/profiles/{friend}/ajaxsetnickname/"), &fields, &page)
        .await
        .map_err(|e| format!("{e:#}"))?;
    if st >= 400 {
        return Err(format!("Steam ответил {st}"));
    }
    match serde_json::from_str::<serde_json::Value>(&body) {
        Ok(v) if v["success"].as_i64() == Some(1) || v["success"].as_bool() == Some(true) => Ok(()),
        Ok(v) => Err(format!("Steam отказал (код {})", v["success"])),
        Err(_) => Ok(()),
    }
}

#[derive(Default)]
pub struct EditUi {
    pub data: Option<ProfileEdit>,
    pub original: Option<ProfileEdit>,
    pub asked: bool,
    pub tab: u8,
}

const LEVELS: [(u8, &str); 3] = [(3, "Все"), (2, "Только друзья"), (1, "Скрыто (только я)")];

fn level(ui: &mut egui::Ui, label: &str, v: &mut u8) {
    ui.label(label);
    egui::ComboBox::from_id_salt(label).selected_text(LEVELS.iter().find(|(n, _)| n == v).map_or("?", |(_, l)| l)).show_ui(ui, |ui| {
        for (n, l) in LEVELS {
            ui.selectable_value(v, n, l);
        }
    });
    ui.end_row();
}

impl LauncherApp {
    pub(crate) fn open_profile_edit(&mut self) {
        self.close_extras(crate::View::ProfileEdit);
        self.view = crate::View::ProfileEdit;
        self.edit_ui = EditUi::default();
    }

    pub(crate) fn profile_edit_contents(&mut self, ui: &mut egui::Ui, net: &NetState) {
        if !self.edit_ui.asked {
            self.edit_ui.asked = true;
            let _ = self.tx.send(Cmd::ProfileEditLoad);
        }
        if self.edit_ui.data.is_none() {
            match &net.profile_edit {
                Some(Ok(e)) => {
                    self.edit_ui.data = Some(e.clone());
                    self.edit_ui.original = Some(e.clone());
                }
                Some(Err(e)) => {
                    ui.label(format!("Не удалось загрузить профиль: {e}"));
                    return;
                }
                None => {
                    crate::spin::spinner(ui);
                    return;
                }
            }
        }
        let Some(mut d) = self.edit_ui.data.take() else { return };
        let orig = self.edit_ui.original.clone().unwrap_or_default();
        let mut cmd = None;
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Редактирование профиля").size(20.0).strong());
            ui.add_space(12.0);
            for (i, t) in ["Основное", "Аватар", "Приватность"].into_iter().enumerate() {
                if ui.selectable_label(self.edit_ui.tab == i as u8, t).clicked() {
                    self.edit_ui.tab = i as u8;
                }
            }
        });
        ui.separator();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.set_max_width(ui.available_width().min(760.0));
            match self.edit_ui.tab {
                0 => {
                    egui::Grid::new("pe_main").num_columns(2).spacing([14.0, 8.0]).show(ui, |ui| {
                        ui.label("Имя профиля");
                        ui.add(egui::TextEdit::singleline(&mut d.persona).desired_width(360.0));
                        ui.end_row();
                        ui.label("Настоящее имя");
                        ui.add(egui::TextEdit::singleline(&mut d.real_name).desired_width(360.0).hint_text("необязательно"));
                        ui.end_row();
                        ui.label("Свой адрес");
                        ui.horizontal(|ui| {
                            ui.weak("steamcommunity.com/id/");
                            ui.add(egui::TextEdit::singleline(&mut d.custom_url).desired_width(200.0).hint_text("необязательно"));
                        });
                        ui.end_row();
                        ui.label("Страна");
                        ui.horizontal(|ui| {
                            ui.add(egui::TextEdit::singleline(&mut d.country).desired_width(40.0).hint_text("RU"));
                            ui.weak(if d.country_name.is_empty() { "код страны из двух латинских букв, пусто - не показывать".to_string() } else { d.country_name.clone() });
                        });
                        ui.end_row();
                        ui.label("О себе");
                        ui.add(egui::TextEdit::multiline(&mut d.summary).desired_width(460.0).desired_rows(6));
                        ui.end_row();
                    });
                    ui.add_space(8.0);
                    let changed = d.persona != orig.persona
                        || d.real_name != orig.real_name
                        || d.custom_url != orig.custom_url
                        || d.summary != orig.summary
                        || d.country != orig.country;
                    ui.horizontal(|ui| {
                        if ui.add_enabled(changed && !d.persona.trim().is_empty(), egui::Button::new("Сохранить")).clicked() {
                            if d.country != orig.country {
                                d.state.clear();
                                d.city.clear();
                            }
                            cmd = Some(Cmd::ProfileEditSave(d.clone()));
                        }
                        if changed && ui.button("Вернуть как было").clicked() {
                            d = orig.clone();
                        }
                    });
                    ui.weak("Изменения увидят все, кто открывает ваш профиль.");
                }
                1 => {
                    if !d.avatar_hash.is_empty() {
                        ui.add(
                            egui::Image::new(format!("https://avatars.fastly.steamstatic.com/{}_full.jpg", d.avatar_hash))
                                .fit_to_exact_size(egui::vec2(184.0, 184.0))
                                .rounding(4.0),
                        );
                    }
                    ui.add_space(6.0);
                    ui.label("Картинка JPG или PNG до 1 МБ; Steam сам сделает ее квадратной 184 x 184.");
                    if ui.button("Выбрать картинку и загрузить...").clicked() {
                        if let Some(p) = rfd::FileDialog::new().add_filter("Картинка", &["jpg", "jpeg", "png"]).pick_file() {
                            cmd = Some(Cmd::AvatarUpload(p));
                        }
                    }
                    ui.weak("Анимированные аватары и рамки покупаются в магазине очков.");
                }
                _ => {
                    egui::Grid::new("pe_priv").num_columns(2).spacing([14.0, 8.0]).show(ui, |ui| {
                        level(ui, "Мой профиль", &mut d.privacy.profile);
                        level(ui, "Список игр", &mut d.privacy.games);
                        level(ui, "Время в играх", &mut d.privacy.playtime);
                        level(ui, "Список друзей", &mut d.privacy.friends);
                        level(ui, "Инвентарь", &mut d.privacy.inventory);
                        level(ui, "Подарки в инвентаре", &mut d.privacy.gifts);
                        ui.label("Комментарии в профиле");
                        let names = [(1u8, "Все"), (0, "Только друзья"), (2, "Никто")];
                        egui::ComboBox::from_id_salt("pe_comments")
                            .selected_text(names.iter().find(|(n, _)| *n == d.privacy.comments).map_or("?", |(_, l)| l))
                            .show_ui(ui, |ui| {
                                for (n, l) in names {
                                    ui.selectable_value(&mut d.privacy.comments, n, l);
                                }
                            });
                        ui.end_row();
                    });
                    ui.add_space(8.0);
                    let changed = d.privacy != orig.privacy;
                    ui.horizontal(|ui| {
                        if ui.add_enabled(changed, egui::Button::new("Сохранить")).clicked() {
                            cmd = Some(Cmd::PrivacySave(d.privacy.clone()));
                        }
                        if changed && ui.button("Вернуть как было").clicked() {
                            d.privacy = orig.privacy.clone();
                        }
                    });
                    ui.weak("«Скрыто» у профиля прячет почти все: игры, друзей, комментарии.");
                }
            }
            if let Some(m) = &net.profile_edit_msg {
                ui.add_space(6.0);
                ui.label(m);
            }
        });
        if let Some(c) = cmd {
            // After saving we reread: "as it was" becomes the new state.
            self.edit_ui.original = None;
            self.edit_ui.data = None;
            self.edit_ui.asked = true;
            let _ = self.tx.send(c);
        } else {
            self.edit_ui.data = Some(d);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn parse_attr() {
        let html = r#"<div id="profile_edit_config" data-profile-edit="{&quot;strPersonaName&quot;:&quot;Имя&quot;,&quot;strCustomURL&quot;:&quot;&quot;,&quot;strRealName&quot;:&quot;&quot;,&quot;strSummary&quot;:&quot;a &amp; b&quot;,&quot;strAvatarHash&quot;:&quot;ab&quot;,&quot;LocationData&quot;:{&quot;locCountry&quot;:&quot;Россия&quot;,&quot;locCountryCode&quot;:&quot;RU&quot;,&quot;locStateCode&quot;:&quot;&quot;,&quot;locCityCode&quot;:&quot;&quot;},&quot;Privacy&quot;:{&quot;PrivacySettings&quot;:{&quot;PrivacyProfile&quot;:3,&quot;PrivacyInventory&quot;:2,&quot;PrivacyInventoryGifts&quot;:1,&quot;PrivacyOwnedGames&quot;:3,&quot;PrivacyPlaytime&quot;:3,&quot;PrivacyFriendsList&quot;:3},&quot;eCommentPermission&quot;:1}}"></div>"#;
        let e = super::parse(html).unwrap();
        assert_eq!(e.persona, "Имя");
        assert_eq!(e.summary, "a & b");
        assert_eq!(e.country, "RU");
        assert_eq!(e.privacy.inventory, 2);
        assert_eq!(e.privacy.gifts, 1);
        assert_eq!(e.privacy.comments, 1);
    }
}

#[cfg(test)]
mod live {
    /// Read only: own profile data for editing (name and privacy are not printed).
    #[test]
    #[ignore]
    fn profile_edit_load_live() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let e = super::load(&c).await.unwrap();
            println!("имя есть: {}, о себе: {} знаков, аватар: {}, приватность профиля: {}", !e.persona.is_empty(), e.summary.chars().count(), !e.avatar_hash.is_empty(), e.privacy.profile);
        });
    }
}
