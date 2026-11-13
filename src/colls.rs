//! Коллекции Steam (категории библиотеки) через Aurelia: «Избранное», «Скрытые» и свои.
//!
//! В облаке Steam рядом с коллекциями лежат служебные записи (showcases, NewContentRollup…),
//! их не показываем: только встроенные favorite/hidden и пользовательские `uc-…`.
//! Динамические коллекции (по фильтру) не меняем - только показываем.
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
    /// Создать коллекцию и сразу добавить в нее игру (если есть).
    Create { name: String, appid: Option<u32> },
    /// Переименовать свою коллекцию.
    Rename { id: String, name: String },
    /// Удалить свою коллекцию (игры остаются в библиотеке).
    Delete { id: String },
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
    // Избранное первым, Скрытые последними, свои - по алфавиту между ними.
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
    // Aurelia 0.2.5: pull скачивает коллекции Steam и сохраняет их локально; правки делаются
    // в сохраненном хранилище (CollectionsStore::edit), push отправляет его в Steam.
    let mut store = collections::pull(client).await.map_err(es)?;
    let edit: Option<Box<dyn FnOnce(&mut CollectionsStore) -> anyhow::Result<()> + Send>> = match cmd {
        CollCmd::Load => None,
        CollCmd::Toggle { id, appid, add } => Some(Box::new(move |s| if add { s.add(&id, &[appid]) } else { s.remove(&id, &[appid]) })),
        CollCmd::Rename { id, name } => Some(Box::new(move |s| s.rename(&id, &name))),
        CollCmd::Delete { id } => Some(Box::new(move |s| s.delete(&id))),
        CollCmd::Create { name, appid } => Some(Box::new(move |s| {
            let id = s.create(&name)?;
            if let Some(a) = appid {
                s.add(&id, &[a])?;
            }
            Ok(())
        })),
    };
    if let Some(f) = edit {
        CollectionsStore::edit(f).map_err(es)?;
        store = collections::push(client, false).await.map_err(es)?;
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

#[cfg(test)]
mod live {
    use super::*;

    /// Живой тест на аккаунте: временная коллекция создается, переименовывается и удаляется.
    /// Остальные коллекции не меняются.
    #[test]
    #[ignore]
    fn rename_delete_live() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let before = run(&c, CollCmd::Load).await.unwrap();
            let names = |v: &[Coll]| v.iter().map(|c| (c.id.clone(), c.name.clone(), c.apps.len())).collect::<Vec<_>>();
            let list = run(&c, CollCmd::Create { name: "Vaporust test".into(), appid: Some(70) }).await.unwrap();
            let t = list.iter().find(|c| c.name == "Vaporust test").expect("создана").id.clone();
            let list = run(&c, CollCmd::Rename { id: t.clone(), name: "Vaporust test 2".into() }).await.unwrap();
            assert!(list.iter().any(|c| c.id == t && c.name == "Vaporust test 2"), "переименована");
            let list = run(&c, CollCmd::Delete { id: t.clone() }).await.unwrap();
            assert!(!list.iter().any(|c| c.id == t), "удалена");
            // Повторная загрузка из облака: временной нет, остальные как были.
            let after = run(&c, CollCmd::Load).await.unwrap();
            assert_eq!(names(&before), names(&after));
            println!("коллекций: {}, все на месте", after.len());
        });
    }
}
