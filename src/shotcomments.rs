//! Оценки и комментарии к скриншотам в облаке Steam (своим и друзей): как на странице
//! скриншота в сообществе Steam (comment/PublishedFile_Public, sharedfiles/voteup).

use crate::net::{Cmd, NetState};
use crate::profile_extra::{parse_comments, Comment};
use crate::{webapi, LauncherApp};
use aurelia::steam_client::SteamClient;
use eframe::egui;

/// Комментарии к скриншоту: (всего, первые 30).
pub async fn load(client: &SteamClient, owner: u64, id: u64) -> Result<(u32, Vec<Comment>), String> {
    let web = webapi::web_session(client).await?;
    let t = web
        .get_text(&format!("https://steamcommunity.com/comment/PublishedFile_Public/render/{owner}/{id}/?start=0&count=30"))
        .await
        .map_err(|e| format!("{e:#}"))?;
    let j: serde_json::Value = serde_json::from_str(&t).map_err(|_| "Steam ответил непонятно.".to_string())?;
    Ok((j["total_count"].as_u64().unwrap_or(0) as u32, parse_comments(j["comments_html"].as_str().unwrap_or(""))))
}

pub async fn post(client: &SteamClient, owner: u64, id: u64, text: &str) -> Result<(), String> {
    let web = webapi::web_session(client).await?;
    let fields =
        [("comment", text.to_string()), ("count", "10".into()), ("sessionid", web.sessionid().to_string()), ("feature2", "-1".into())];
    let (status, body) = web
        .post_form(
            &format!("https://steamcommunity.com/comment/PublishedFile_Public/post/{owner}/{id}/"),
            &fields,
            &format!("https://steamcommunity.com/sharedfiles/filedetails/?id={id}"),
        )
        .await
        .map_err(|e| format!("{e:#}"))?;
    let v: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
    if status == 200 && v.get("success").and_then(|s| s.as_bool()) == Some(true) {
        Ok(())
    } else {
        Err(format!("Комментарий не отправлен (код {status})."))
    }
}

/// «Нравится» (или убрать оценку).
pub async fn vote(client: &SteamClient, id: u64, up: bool) -> Result<(), String> {
    let web = webapi::web_session(client).await?;
    let url = if up { "https://steamcommunity.com/sharedfiles/voteup" } else { "https://steamcommunity.com/sharedfiles/votedown" };
    let fields = [("id", id.to_string()), ("sessionid", web.sessionid().to_string())];
    let (status, body) = web
        .post_form(url, &fields, &format!("https://steamcommunity.com/sharedfiles/filedetails/?id={id}"))
        .await
        .map_err(|e| format!("{e:#}"))?;
    let v: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
    if status == 200 && v.get("success").and_then(|s| s.as_i64()) == Some(1) {
        Ok(())
    } else {
        Err(format!("Оценка не поставлена (код {status})."))
    }
}

/// Открытое окно комментариев: чей скриншот, номер, подпись, картинка.
#[derive(Clone, Default)]
pub struct ShotComments {
    pub owner: u64,
    pub id: u64,
    pub title: String,
    pub preview: String,
    pub draft: String,
}

impl LauncherApp {
    pub(crate) fn open_shot_comments(&mut self, owner: u64, id: u64, title: String, preview: String) {
        let _ = self.tx.send(Cmd::ShotComments { owner, id });
        self.net.lock().unwrap().shot_msg = None;
        self.shot_comments = Some(ShotComments { owner, id, title, preview, draft: String::new() });
    }

    /// Окно «Комментарии к скриншоту».
    pub(crate) fn shot_comments_window(&mut self, ctx: &egui::Context, net: &NetState) {
        let Some(mut w) = self.shot_comments.take() else { return };
        let mut open = true;
        let mut send: Option<Cmd> = None;
        let mut url: Option<String> = None;
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("vaporust_shot_comments"),
            crate::socialwin::viewport("Скриншот - Vaporust", [520.0, 600.0], [380.0, 320.0]),
            |ctx, _| {
                crate::winframe::title_bar_auto(ctx);
                egui::CentralPanel::default().show(ctx, |ui| {
                    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                        if !w.preview.is_empty() {
                            ui.add(egui::Image::new(w.preview.as_str()).max_width(ui.available_width()).max_height(260.0));
                        }
                        if !w.title.is_empty() {
                            ui.strong(&w.title);
                        }
                        ui.horizontal(|ui| {
                            if ui.button("Нравится").on_hover_text("Поставить оценку, как кнопка «палец вверх» в Steam").clicked() {
                                send = Some(Cmd::ShotVote { id: w.id, up: true });
                            }
                            if ui.button("Страница в Steam").clicked() {
                                url = Some(format!("https://steamcommunity.com/sharedfiles/filedetails/?id={}", w.id));
                            }
                        });
                        if let Some(m) = &net.shot_msg {
                            ui.weak(m);
                        }
                        ui.separator();
                        match net.shot_comments.get(&w.id) {
                            None => {
                                crate::spin::spinner(ui);
                            }
                            Some(Err(e)) => {
                                ui.label(format!("Комментарии не загрузились: {e}"));
                            }
                            Some(Ok((total, list))) => {
                                ui.strong(format!("Комментарии ({total})"));
                                ui.horizontal(|ui| {
                                    ui.add(
                                        egui::TextEdit::singleline(&mut w.draft)
                                            .hint_text("Ваш комментарий")
                                            .desired_width(ui.available_width() - 100.0),
                                    );
                                    if ui.add_enabled(!w.draft.trim().is_empty(), egui::Button::new("Отправить")).clicked() {
                                        send = Some(Cmd::ShotComment { owner: w.owner, id: w.id, text: w.draft.trim().to_string() });
                                        w.draft.clear();
                                    }
                                });
                                if list.is_empty() {
                                    ui.weak("Комментариев нет.");
                                }
                                for c in list {
                                    ui.horizontal_top(|ui| {
                                        if !c.avatar.is_empty() {
                                            ui.add(egui::Image::new(c.avatar.as_str()).fit_to_exact_size(egui::vec2(28.0, 28.0)));
                                        }
                                        ui.vertical(|ui| {
                                            ui.horizontal(|ui| {
                                                ui.strong(&c.author);
                                                if c.time > 0 {
                                                    ui.weak(crate::socialwin::fmt_date(c.time));
                                                }
                                            });
                                            ui.add(egui::Label::new(&c.text).wrap());
                                        });
                                    });
                                }
                            }
                        }
                    });
                });
                if ctx.input(|i| i.viewport().close_requested()) {
                    open = false;
                }
            },
        );
        if let Some(c) = send {
            let _ = self.tx.send(c);
        }
        if let Some(u) = url {
            ctx.open_url(egui::OpenUrl::new_tab(u));
        }
        if open {
            self.shot_comments = Some(w);
        }
    }
}

#[cfg(test)]
mod tests {
    /// Живой тест (только чтение): комментарии к скриншотам друга.
    #[test]
    #[ignore]
    fn shot_comments_live() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let friends = c.collect_friends(std::time::Duration::from_secs(5)).await.unwrap();
            for f in friends.iter().filter(|f| f.relationship == 3).take(8) {
                let Ok((_, shots)) = c.user_screenshots(f.steam_id, 0, 1, 50).await else { continue };
                if let Some(s) = shots.iter().max_by_key(|s| s.comments) {
                    let r = super::load(&c, f.steam_id, s.id).await.unwrap();
                    println!("скриншот: оценок {}, комментариев {} (в выдаче {}), загружено {}", s.votes_up, s.comments, r.0, r.1.len());
                    if s.comments > 0 {
                        assert!(!r.1.is_empty());
                        return;
                    }
                }
            }
        });
    }
}
