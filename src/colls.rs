//! Коллекции Steam (категории библиотеки) через Aurelia: «Избранное», «Скрытые» и свои.
//!
//! В облаке Steam рядом с коллекциями лежат служебные записи (showcases, NewContentRollup…),
//! их не показываем: только встроенные favorite/hidden и пользовательские `uc-…`.
//! Динамические коллекции (по фильтру) не меняем — только показываем.
//! Перед каждым изменением забираем свежую версию из облака (pull), потом отправляем (push),
//! чтобы не затереть правки, сделанные в официальном Steam.

use aurelia::library::collections::{self, CollectionsStore};
use aurelia::steam_client::SteamClient;
use eframe::egui;

#[derive(Clone, Debug)]
pub struct Coll {
    pub id: String,
    /// Название для показа («Избранное» вместо Favorites и т.п.).
    pub name: String,
    pub apps: Vec<u32>,
    pub dynamic: bool,
}

pub enum CollCmd {
    Load,
    /// Добавить (true) или убрать игру из коллекции.
    Toggle { id: String, appid: u32, add: bool },
    /// Создать коллекцию и сразу добавить в неё игру (если есть).
    Create { name: String, appid: Option<u32> },
}

fn visible(store: &CollectionsStore) -> Vec<Coll> {
    let mut out: Vec<Coll> = store
        .collections
        .iter()
        .filter(|c| !c.deleted && (c.is_builtin() || c.id.starts_with("uc-")))
        .map(|c| Coll {
            id: c.id.clone(),
            name: match c.id.as_str() {
                "favorite" => "Избранное".into(),
                "hidden" => "Скрытые".into(),
                _ => c.name.clone(),
            },
            apps: c.added.iter().copied().filter(|a| c.contains(*a)).collect(),
            dynamic: c.is_dynamic(),
        })
        .collect();
    // Избранное первым, Скрытые последними, свои — по алфавиту между ними.
    let rank = |c: &Coll| match c.id.as_str() {
        "favorite" => 0,
        "hidden" => 2,
        _ => 1,
    };
    out.sort_by(|a, b| rank(a).cmp(&rank(b)).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    out
}

fn es<E: std::fmt::Display>(e: E) -> String {
    format!("{e:#}")
}

async fn run(client: &SteamClient, cmd: CollCmd) -> Result<Vec<Coll>, String> {
    let mut store = CollectionsStore::load().map_err(es)?;
    collections::pull(&mut store, client).await.map_err(es)?;
    match cmd {
        CollCmd::Load => {}
        CollCmd::Toggle { id, appid, add } => {
            if add {
                store.add(&id, &[appid]).map_err(es)?;
            } else {
                store.remove(&id, &[appid]).map_err(es)?;
            }
            collections::push(&mut store, client).await.map_err(es)?;
        }
        CollCmd::Create { name, appid } => {
            let id = store.create(&name).map_err(es)?;
            if let Some(a) = appid {
                store.add(&id, &[a]).map_err(es)?;
            }
            collections::push(&mut store, client).await.map_err(es)?;
        }
    }
    Ok(visible(&store))
}

pub fn handle(client: &SteamClient, cmd: CollCmd, st: &crate::net::Shared, ctx: &egui::Context) {
    let (c, st, ctx) = (client.clone(), st.clone(), ctx.clone());
    tokio::spawn(async move {
        let res = run(&c, cmd).await;
        let mut s = st.lock().unwrap();
        match res {
            Ok(list) => {
                s.collections = Some(list);
                s.collections_error = None;
            }
            Err(e) => s.collections_error = Some(format!("Коллекции: {e}")),
        }
        drop(s);
        ctx.request_repaint();
    });
}
