//! «Сообщество» игры: обсуждения (список тем, тема с ответами, свой ответ) и руководства.
//! Обсуждения - страницы сообщества Steam (как их видит браузер), ответы - через тот же
//! механизм комментариев, что и на сайте; руководства - Web API IPublishedFileService/QueryFiles.

use crate::net::{Cmd, NetState};
use crate::profile_extra::{parse_comments, Comment};
use crate::{webapi, LauncherApp};
use aurelia::steam_client::SteamClient;
use eframe::egui;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Topic {
    pub url: String,
    pub title: String,
    pub author: String,
    pub replies: u32,
    pub last: u64,
    pub sticky: bool,
    pub locked: bool,
}

#[derive(Clone, Debug, Default)]
pub struct TopicView {
    pub url: String,
    pub title: String,
    pub author: String,
    pub time: u64,
    /// Текст первого сообщения с абзацами и картинками.
    pub items: Vec<crate::richtext::Item>,
    /// Для ответов: владелец ветки, форум, тема, extended_data.
    pub owner: String,
    pub forum: String,
    pub topic: String,
    pub extended: String,
    pub can_reply: bool,
    pub total: u32,
    pub replies: Vec<Comment>,
}

#[derive(Clone, Debug, Default)]
pub struct Guide {
    pub id: u64,
    pub title: String,
    pub desc: String,
    pub preview: String,
    pub votes_up: u32,
    pub views: u64,
}

fn between<'a>(s: &'a str, a: &str, b: &str) -> Option<&'a str> {
    let i = s.find(a)? + a.len();
    let j = s[i..].find(b)? + i;
    Some(&s[i..j])
}

fn text(s: &str) -> String {
    crate::store::plain_text(s).split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Список тем со страницы обсуждений (l=english).
pub fn parse_topics(html: &str) -> Vec<Topic> {
    html.split("class=\"forum_topic ")
        .skip(1)
        .filter_map(|t| {
            let classes = t.split('"').next().unwrap_or("");
            let url = between(t, "class=\"forum_topic_overlay\" href=\"", "\"")?.to_string();
            let replies = between(t, "forum_topic_reply_count\">", "</div>")
                .and_then(|r| r.rsplit('>').next())
                .and_then(|n| n.trim().replace(',', "").parse().ok())
                .unwrap_or(0);
            let last = between(t, "forum_topic_lastpost\"", ">").and_then(|a| between(a, "data-timestamp=\"", "\"")).and_then(|n| n.parse().ok()).unwrap_or(0);
            let name_html = between(t, "class=\"forum_topic_name", "</div>").and_then(|n| n.split_once('>').map(|x| x.1)).unwrap_or("");
            // Метка «PINNED:» - в своем span, ее убираем.
            let name_html = match name_html.find("</span>") {
                Some(i) if name_html.contains("forum_topic_label") => &name_html[i + 7..],
                _ => name_html,
            };
            let author = between(t, "class=\"forum_topic_op\">", "</div>").map(text).unwrap_or_default();
            Some(Topic {
                url,
                title: text(name_html),
                author,
                replies,
                last,
                sticky: classes.contains("sticky"),
                locked: classes.contains("locked"),
            })
        })
        .collect()
}

/// Первый пост темы и данные для ответов.
pub fn parse_topic(html: &str, url: &str) -> TopicView {
    let op = html.split("class=\"forum_op ").nth(1).unwrap_or("");
    let title = between(op, "class=\"topic\"", "</div>").and_then(|t| t.split_once('>').map(|x| x.1)).map(text).unwrap_or_default();
    let author = between(op, "forum_op_author", "<span").and_then(|a| a.split_once('>').map(|x| x.1)).map(text).unwrap_or_default();
    let time = between(op, "commentthread_comment_timestamp\"", ">").and_then(|a| between(a, "data-timestamp=\"", "\"")).and_then(|n| n.parse().ok()).unwrap_or(0);
    let body = between(op, "class=\"content\"", "<div class=\"forum_op_footer\"").or_else(|| between(op, "class=\"content\"", "</div>")).unwrap_or("");
    let body = body.split_once('>').map(|x| x.1).unwrap_or(body);
    let items = crate::richtext::parse(body);
    // InitializeCommentThread( "ForumTopic", "ForumTopic_<owner>_<forum>_<topic>", {...})
    let mut v = TopicView { url: url.to_string(), title, author, time, items, ..Default::default() };
    if let Some(args) = between(html, "InitializeCommentThread( \"ForumTopic\", \"", "\n") {
        if let Some(json) = args.split_once(", {").map(|x| format!("{{{}", x.1)) {
            // Объект кончается перед вторым аргументом: «}, 'https://steamcommunity.com/comment/…', 40 );».
            let json = json.rsplit_once("}, '").map(|x| format!("{}}}", x.0)).unwrap_or(json);
            if let Ok(j) = serde_json::from_str::<serde_json::Value>(&json) {
                let s = |k: &str| j.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
                v.owner = s("owner");
                v.forum = s("feature");
                v.topic = s("feature2");
                v.extended = s("extended_data");
                v.total = j.get("total_count").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
            }
        }
    }
    v.can_reply = html.contains("\"can_reply\":1") && !v.owner.is_empty();
    v
}

pub async fn load_topics(client: &SteamClient, appid: u32, page: u32) -> Result<Vec<Topic>, String> {
    let web = webapi::web_session(client).await?;
    let html = web
        .get_text(&format!("https://steamcommunity.com/app/{appid}/discussions/?l=english&fp={page}"))
        .await
        .map_err(|e| format!("{e:#}"))?;
    Ok(parse_topics(&html))
}

pub async fn load_topic(client: &SteamClient, url: &str) -> Result<TopicView, String> {
    let web = webapi::web_session(client).await?;
    let html = web.get_text(&format!("{url}?l=english")).await.map_err(|e| format!("{e:#}"))?;
    let mut v = parse_topic(&html, url);
    // Первая страница ответов уже на странице темы.
    if let Some(area) = html.split("commentthread_area").nth(1) {
        v.replies = parse_comments(area);
    }
    // Остальные - как их догружает сайт (POST render с данными ветки).
    if !v.owner.is_empty() && (v.replies.len() as u32) < v.total {
        let fields = [
            ("start", "0".to_string()),
            ("count", "50".to_string()),
            ("sessionid", web.sessionid().to_string()),
            ("extended_data", v.extended.clone()),
            ("feature2", v.topic.clone()),
            ("oldestfirst", "true".to_string()),
            ("include_raw", "true".to_string()),
        ];
        let r = web
            .post_form(&format!("https://steamcommunity.com/comment/ForumTopic/render/{}/{}/", v.owner, v.forum), &fields, url)
            .await;
        if let Ok((200, t)) = r {
            if let Ok(j) = serde_json::from_str::<serde_json::Value>(&t) {
                let more = parse_comments(j["comments_html"].as_str().unwrap_or(""));
                if more.len() > v.replies.len() {
                    v.replies = more;
                }
            }
        }
    }
    Ok(v)
}

pub async fn reply(client: &SteamClient, t: &TopicView, msg: &str) -> Result<(), String> {
    let web = webapi::web_session(client).await?;
    let fields = [
        ("comment", msg.to_string()),
        ("count", "15".to_string()),
        ("sessionid", web.sessionid().to_string()),
        ("extended_data", t.extended.clone()),
        ("feature2", t.topic.clone()),
        ("oldestfirst", "true".to_string()),
        ("include_raw", "true".to_string()),
    ];
    let (status, body) = web
        .post_form(&format!("https://steamcommunity.com/comment/ForumTopic/post/{}/{}/", t.owner, t.forum), &fields, &t.url)
        .await
        .map_err(|e| format!("{e:#}"))?;
    let v: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
    if status == 200 && v.get("success").and_then(|s| s.as_bool()) == Some(true) {
        Ok(())
    } else {
        let why = v.get("error").and_then(|e| e.as_str()).map(text).unwrap_or_else(|| format!("код {status}"));
        Err(format!("Ответ не отправлен: {why}"))
    }
}

/// Руководства игры: лучшие или по тексту (по-русски, если есть, иначе все языки).
pub async fn load_guides(client: &SteamClient, appid: u32, search: &str) -> Result<Vec<Guide>, String> {
    let mut p = vec![
        ("appid", appid.to_string()),
        ("filetype", "10".to_string()),
        ("numperpage", "40".to_string()),
        ("return_short_description", "true".to_string()),
        ("return_vote_data", "true".to_string()),
        ("return_previews", "true".to_string()),
        ("language", "8".to_string()),
    ];
    if search.trim().is_empty() {
        p.push(("query_type", "0".to_string()));
    } else {
        p.push(("query_type", "12".to_string()));
        p.push(("search_text", search.trim().to_string()));
    }
    let v = webapi::get(client, "IPublishedFileService/QueryFiles/v1", &p).await?;
    let num = |x: &serde_json::Value| x.as_u64().or_else(|| x.as_str()?.parse().ok()).unwrap_or(0);
    Ok(v["publishedfiledetails"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|d| Guide {
            id: num(&d["publishedfileid"]),
            title: d["title"].as_str().unwrap_or("").to_string(),
            desc: text(d["short_description"].as_str().unwrap_or("")),
            preview: d["preview_url"].as_str().unwrap_or("").to_string(),
            votes_up: num(&d["vote_data"]["votes_up"]) as u32,
            views: num(&d["views"]),
        })
        .collect())
}

/// Руководство целиком: название и разделы (заголовок + текст с картинками).
#[derive(Clone, Debug, Default)]
pub struct GuideView {
    pub id: u64,
    pub title: String,
    pub intro: Vec<crate::richtext::Item>,
    pub sections: Vec<(String, Vec<crate::richtext::Item>)>,
}

/// Разобрать страницу руководства (sharedfiles/filedetails).
pub fn parse_guide(html: &str, id: u64) -> GuideView {
    let title = between(html, "class=\"workshopItemTitle\">", "<").map(text).unwrap_or_default();
    let intro = between(html, "class=\"guideTopDescription\">", "</div>").map(crate::richtext::parse).unwrap_or_default();
    let sections = html
        .split("class=\"subSectionTitle\">")
        .skip(1)
        .map(|s| {
            let name = s.split("</div>").next().map(text).unwrap_or_default();
            let desc = s
                .split("class=\"subSectionDesc\">")
                .nth(1)
                .map(|d| d.split("<div class=\"subSection detailBox\"").next().unwrap_or(d))
                .unwrap_or("");
            (name, crate::richtext::parse(desc))
        })
        .collect();
    GuideView { id, title, intro, sections }
}

pub async fn load_guide(client: &SteamClient, id: u64) -> Result<GuideView, String> {
    let web = webapi::web_session(client).await?;
    let html = web
        .get_text(&format!("https://steamcommunity.com/sharedfiles/filedetails/?id={id}&l=russian"))
        .await
        .map_err(|e| format!("{e:#}"))?;
    let g = parse_guide(&html, id);
    if g.sections.is_empty() && g.intro.is_empty() {
        return Err("Не удалось прочитать руководство - откройте его в браузере.".into());
    }
    Ok(g)
}

#[derive(Default)]
pub struct CommunityUi {
    pub appid: Option<(u32, String)>,
    /// 0 - обсуждения, 1 - руководства.
    pub tab: u8,
    pub page: u32,
    pub asked: Option<(u32, u8, u32)>,
    pub search: String,
    pub draft: String,
}

impl LauncherApp {
    pub(crate) fn open_community(&mut self, appid: u32, name: String) {
        self.close_extras(crate::View::Community);
        self.community_ui = CommunityUi { appid: Some((appid, name)), page: 1, ..Default::default() };
        let mut s = self.net.lock().unwrap();
        s.comm_topic = None;
        s.comm_guide = None;
        s.comm_msg = None;
        drop(s);
        self.view = crate::View::Community;
    }

    pub(crate) fn community_contents(&mut self, ui: &mut egui::Ui, net: &NetState) {
        let Some((appid, name)) = self.community_ui.appid.clone() else { return };
        let st = &mut self.community_ui;
        let want = (appid, st.tab, st.page);
        if st.asked != Some(want) && net.account.is_some() {
            st.asked = Some(want);
            let _ = self.tx.send(if st.tab == 0 {
                Cmd::CommTopics { appid, page: st.page }
            } else {
                Cmd::CommGuides { appid, search: st.search.clone() }
            });
        }
        let mut open_url: Option<String> = None;
        let mut open_guide: Option<u64> = None;
        let mut open_topic: Option<String> = None;
        let mut back = false;
        ui.horizontal(|ui| {
            ui.heading(format!("Сообщество: {name}"));
            ui.separator();
            if ui.selectable_label(st.tab == 0, "Обсуждения").clicked() {
                st.tab = 0;
                back = true;
            }
            if ui.selectable_label(st.tab == 1, "Руководства").clicked() {
                st.tab = 1;
            }
        });
        if let Some(m) = &net.comm_msg {
            ui.weak(m);
        }
        ui.separator();
        // Открытая тема.
        if st.tab == 0 {
            if let Some(t) = &net.comm_topic {
                if ui.button("< К списку тем").clicked() {
                    back = true;
                }
                match t {
                    Err(e) => {
                        ui.label(format!("Тема не загрузилась: {e}"));
                    }
                    Ok(t) => {
                        let mut send = None;
                        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                            ui.label(egui::RichText::new(crate::richtext::fix(ui, &t.title)).size(18.0).strong());
                            ui.horizontal(|ui| {
                                ui.strong(crate::richtext::fix(ui, &t.author));
                                if t.time > 0 {
                                    ui.weak(crate::socialwin::fmt_date(t.time));
                                }
                                if ui.small_button("Открыть в браузере").clicked() {
                                    open_url = Some(t.url.clone());
                                }
                            });
                            ui.add_space(6.0);
                            crate::richtext::show(ui, &t.items);
                            ui.separator();
                            ui.strong(format!("Ответы ({})", t.total));
                            for c in &t.replies {
                                ui.horizontal_top(|ui| {
                                    if !c.avatar.is_empty() {
                                        ui.add(egui::Image::new(c.avatar.as_str()).fit_to_exact_size(egui::vec2(28.0, 28.0)));
                                    }
                                    ui.vertical(|ui| {
                                        ui.horizontal(|ui| {
                                            ui.strong(crate::richtext::fix(ui, &c.author));
                                            if c.time > 0 {
                                                ui.weak(crate::socialwin::fmt_date(c.time));
                                            }
                                        });
                                        let txt = crate::richtext::fix(ui, &c.text);
                                        ui.add(egui::Label::new(txt).wrap());
                                    });
                                });
                                ui.add_space(4.0);
                            }
                            if (t.replies.len() as u32) < t.total {
                                ui.weak("Показаны первые 50 ответов - остальные в браузере.");
                            }
                            ui.add_space(6.0);
                            if t.can_reply {
                                ui.add(egui::TextEdit::multiline(&mut st.draft).hint_text("Ваш ответ").desired_rows(3).desired_width(f32::INFINITY));
                                if ui.add_enabled(!st.draft.trim().is_empty(), egui::Button::new("Ответить")).clicked() {
                                    send = Some(st.draft.trim().to_string());
                                }
                            } else {
                                ui.weak("Отвечать в этой теме нельзя (закрыта или нет прав).");
                            }
                        });
                        if let Some(m) = send {
                            st.draft.clear();
                            let _ = self.tx.send(Cmd::CommReply { topic: Box::new(t.clone()), text: m });
                        }
                    }
                }
                if back {
                    self.net.lock().unwrap().comm_topic = None;
                }
                if let Some(u) = open_url {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(u));
                }
                return;
            }
        }
        if st.tab == 0 {
            ui.horizontal(|ui| {
                if ui.add_enabled(st.page > 1, egui::Button::new("< Новее")).clicked() {
                    st.page -= 1;
                }
                ui.label(format!("страница {}", st.page));
                if ui.button("Дальше >").clicked() {
                    st.page += 1;
                }
                if ui.button("Обновить").clicked() {
                    st.asked = None;
                }
            });
            match &net.comm_topics {
                Some(Ok((a, p, list))) if *a == appid && *p == st.page => {
                    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                        for t in list {
                            ui.horizontal(|ui| {
                                if t.sticky {
                                    ui.colored_label(egui::Color32::from_rgb(230, 170, 60), "закреплено");
                                }
                                if ui.link(egui::RichText::new(crate::richtext::fix(ui, &t.title)).strong()).clicked() {
                                    open_topic = Some(t.url.clone());
                                }
                            });
                            ui.horizontal(|ui| {
                                ui.weak(crate::richtext::fix(ui, &t.author));
                                ui.weak(format!("ответов: {}", t.replies));
                                if t.last > 0 {
                                    ui.weak(format!("последний {}", crate::socialwin::fmt_date(t.last)));
                                }
                                if t.locked {
                                    ui.weak("закрыта");
                                }
                            });
                            ui.add_space(4.0);
                        }
                        if list.is_empty() {
                            ui.weak("Тем нет.");
                        }
                    });
                }
                Some(Err(e)) => {
                    ui.label(format!("Не удалось загрузить: {e}"));
                }
                _ => {
                    crate::spin::spinner(ui);
                }
            }
        } else if let Some(g) = &net.comm_guide {
            // Открытое руководство - целиком, в программе.
            let mut back = false;
            ui.horizontal(|ui| {
                if ui.button("< К руководствам").clicked() {
                    back = true;
                }
                if let Ok(g) = g {
                    if ui.button("Открыть в браузере").clicked() {
                        open_url = Some(format!("https://steamcommunity.com/sharedfiles/filedetails/?id={}", g.id));
                    }
                }
            });
            ui.separator();
            match g {
                Err(e) => {
                    ui.label(e);
                }
                Ok(g) => {
                    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                        ui.set_max_width(860.0);
                        ui.label(egui::RichText::new(crate::richtext::fix(ui, &g.title)).size(20.0).strong());
                        ui.add_space(6.0);
                        crate::richtext::show(ui, &g.intro);
                        // Оглавление.
                        if g.sections.len() > 1 {
                            egui::CollapsingHeader::new("Содержание").default_open(false).show(ui, |ui| {
                                for (name, _) in &g.sections {
                                    ui.label(crate::richtext::fix(ui, name));
                                }
                            });
                        }
                        for (name, items) in &g.sections {
                            ui.add_space(10.0);
                            ui.separator();
                            ui.label(egui::RichText::new(crate::richtext::fix(ui, name)).size(18.0).strong());
                            ui.add_space(4.0);
                            crate::richtext::show(ui, items);
                        }
                    });
                }
            }
            if back {
                self.net.lock().unwrap().comm_guide = None;
            }
        } else {
            ui.horizontal(|ui| {
                let edit = ui.add(egui::TextEdit::singleline(&mut st.search).hint_text("Поиск руководств").desired_width(260.0));
                if (edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) || ui.button("Найти").clicked() {
                    st.asked = None;
                }
            });
            match &net.comm_guides {
                Some(Ok((a, list))) if *a == appid => {
                    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                        for g in list {
                            ui.horizontal_top(|ui| {
                                if !g.preview.is_empty() {
                                    ui.add(egui::Image::new(crate::richtext::ugc_small(&g.preview, 128)).fit_to_exact_size(egui::vec2(64.0, 64.0)));
                                }
                                ui.vertical(|ui| {
                                    if ui.link(egui::RichText::new(&g.title).strong()).on_hover_text("Читать").clicked() {
                                        open_guide = Some(g.id);
                                    }
                                    if !g.desc.is_empty() {
                                        ui.add(egui::Label::new(egui::RichText::new(g.desc.chars().take(220).collect::<String>()).weak()).wrap());
                                    }
                                    ui.weak(format!("оценок: {}, просмотров: {}", g.votes_up, crate::store::group_digits(g.views)));
                                });
                            });
                            ui.add_space(6.0);
                        }
                        if list.is_empty() {
                            ui.weak("Руководств не найдено.");
                        }
                    });
                }
                Some(Err(e)) => {
                    ui.label(format!("Не удалось загрузить: {e}"));
                }
                _ => {
                    crate::spin::spinner(ui);
                }
            }
        }
        if let Some(u) = open_topic {
            let _ = self.tx.send(Cmd::CommTopic(u));
        }
        if let Some(id) = open_guide {
            let _ = self.tx.send(Cmd::CommGuide(id));
        }
        if let Some(u) = open_url {
            ui.ctx().open_url(egui::OpenUrl::new_tab(u));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore]
    fn community_live() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let topics = load_topics(&c, 550, 1).await.unwrap();
            println!("тем: {}, закрепленных {}, с ответами {}", topics.len(), topics.iter().filter(|t| t.sticky).count(), topics.iter().filter(|t| t.replies > 0).count());
            assert!(topics.iter().all(|t| !t.title.is_empty() && t.url.starts_with("https://")));
            let t = topics.iter().filter(|t| !t.sticky).max_by_key(|t| t.replies).unwrap();
            let v = load_topic(&c, &t.url).await.unwrap();
            println!(
                "тема: заголовок {}, автор {}, текст {} знаков, ответов {} (загружено {}), можно ответить {}, ветка {}",
                !v.title.is_empty(),
                !v.author.is_empty(),
                v.items.len(),
                v.total,
                v.replies.len(),
                v.can_reply,
                !v.owner.is_empty() && !v.extended.is_empty()
            );
            let g = load_guides(&c, 550, "").await.unwrap();
            println!("руководств: {}, первое с оценками {}", g.len(), g.first().map_or(0, |x| x.votes_up));
            let s = load_guides(&c, 550, "achievement").await.unwrap();
            println!("поиск руководств: {}", s.len());
        });
    }
}


#[cfg(test)]
mod guide_live {
    /// Живой тест (только чтение): лучшее руководство L4D2 открывается в программе.
    #[test]
    #[ignore]
    fn guide_live() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let g = super::load_guides(&c, 550, "").await.unwrap();
            let v = super::load_guide(&c, g[0].id).await.unwrap();
            let imgs: usize = v.sections.iter().map(|s| s.1.iter().filter(|i| matches!(i, crate::richtext::Item::Image(_))).count()).sum();
            println!("руководство «{}»: разделов {}, картинок {}", v.title, v.sections.len(), imgs);
            assert!(!v.sections.is_empty());
        });
    }
}
