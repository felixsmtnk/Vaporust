//! The library rows: owned, family, installed and non-Steam games merged into one list, filtered and sorted.

use crate::*;

impl LauncherApp {
    /// What the full game list depends on besides the network snapshot: installed games, last launch
    /// times, non-Steam games.
    pub(crate) fn row_map_key(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.snap_count.hash(&mut h);
        for g in &self.games {
            (g.appid, g.ready, g.needs_update, g.size, &g.name).hash(&mut h);
        }
        let mut lp: Vec<_> = self.last_played.iter().collect();
        lp.sort();
        lp.hash(&mut h);
        for g in &self.settings.custom_games {
            (g.id, &g.name, &g.exe, g.minutes, g.last_played).hash(&mut h);
        }
        h.finish()
    }

    /// All games (owned and installed) without filters and sorting. Built once per change
    /// (a few hundred names copied on every frame were a waste on a weak PC).
    pub(crate) fn row_map(&self, net: &NetState) -> std::sync::Arc<HashMap<u32, Row>> {
        let key = self.row_map_key();
        let mut c = self.row_cache.borrow_mut();
        if c.0 != key {
            c.0 = key;
            c.1 = std::sync::Arc::new(self.build_row_map(net));
            c.2 = u64::MAX;
        }
        c.1.clone()
    }

    pub(crate) fn build_row_map(&self, net: &NetState) -> HashMap<u32, Row> {
        let mut map: HashMap<u32, Row> = HashMap::new();
        for g in net.owned.iter() {
            map.insert(
                g.app_id,
                Row {
                    appid: g.app_id,
                    name: g.name.clone(),
                    installed: false,
                    ready: false,
                    needs_update: false,
                    playtime_min: Some(g.playtime_forever_minutes),
                    size: 0,
                    last_played: 0,
                    family: false,
                    owner: None,
                },
            );
        }
        for g in &self.games {
            map.entry(g.appid)
                .and_modify(|r| {
                    r.installed = true;
                    r.ready = g.ready;
                    r.needs_update = g.needs_update;
                    r.size = g.size;
                })
                .or_insert(Row {
                    appid: g.appid,
                    name: g.name.clone(),
                    installed: true,
                    ready: g.ready,
                    needs_update: g.needs_update,
                    playtime_min: None,
                    size: g.size,
                    last_played: 0,
                    family: false,
                    owner: None,
                });
        }
        // Family library: other members' games that are not among our own.
        let own: std::collections::HashSet<u32> = net.owned.iter().map(|g| g.app_id).collect();
        for f in net.family.iter() {
            if own.contains(&f.app_id) {
                continue;
            }
            let r = map.entry(f.app_id).or_insert_with(|| Row {
                appid: f.app_id,
                name: f.name.clone(),
                installed: false,
                ready: false,
                needs_update: false,
                playtime_min: None,
                size: 0,
                last_played: 0,
                family: false,
                owner: None,
            });
            r.family = true;
            r.owner = f.owner_steamid;
        }
        for r in map.values_mut() {
            r.last_played = self.last_played.get(&r.appid).copied().unwrap_or(0);
        }
        // Non-Steam games ("Add a game" button).
        for r in self.custom_rows() {
            map.insert(r.appid, r);
        }
        map
    }

    /// Library rows with filters (search, "installed only", collection) and sorting.
    pub(crate) fn rows(&self, net: &NetState) -> std::sync::Arc<Vec<Row>> {
        use std::hash::{Hash, Hasher};
        let map = self.row_map(net);
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (self.row_map_key(), &self.filter, self.only_installed, &self.collection, &self.genre, self.settings.show_family).hash(&mut h);
        (self.settings.sort as u8).hash(&mut h);
        net.collections.as_ref().map(|c| c.iter().map(|x| (x.id.clone(), x.apps.len())).collect::<Vec<_>>()).hash(&mut h);
        net.updates.as_ref().map(|u| u.len()).hash(&mut h);
        if self.genre.is_some() {
            net.store.lock().unwrap().apps.len().hash(&mut h);
        }
        let key = h.finish();
        {
            let c = self.row_cache.borrow();
            if c.2 == key {
                return c.3.clone();
            }
        }
        let rows = std::sync::Arc::new(self.build_rows(&map, net));
        let mut c = self.row_cache.borrow_mut();
        c.2 = key;
        c.3 = rows.clone();
        rows
    }

    pub(crate) fn build_rows(&self, map: &HashMap<u32, Row>, net: &NetState) -> Vec<Row> {
        let colls = net.collections.as_deref().unwrap_or(&[]);
        let hidden: std::collections::HashSet<u32> = colls
            .iter()
            .find(|c| c.id == "hidden")
            .map(|c| c.apps.iter().copied().collect())
            .unwrap_or_default();
        let selected = self.collection.as_ref().and_then(|id| colls.iter().find(|c| &c.id == id));
        let filter = self.filter.to_lowercase();
        let has_update = |r: &Row| r.needs_update || net.updates.as_ref().is_some_and(|u| u.contains(&r.appid));
        // A smart collection: its rules (the genre is checked below with the store tags).
        let smart = self.collection.as_deref().and_then(|id| id.strip_prefix(smartcoll::PREFIX)).and_then(|i| i.parse::<usize>().ok()).and_then(|i| self.settings.smart.get(i)).cloned();
        let smart_rules = smart.clone().map(|mut s| {
            s.genre = None;
            s
        });
        let mut rows: Vec<Row> = map
            .values()
            .cloned()
            .filter(|r| !self.only_installed || r.installed)
            // README screenshots (tour, private): the user's own non-Steam games are not shown.
            .filter(|r| !(custom::is_custom(r.appid) && std::env::var_os("VAPORUST_TOUR_PRIVATE").is_some()))
            // Collection: when selected - only its games; "all games" - without "Hidden", as in Steam.
            // "Family library" - only family games; in "All games" - by the checkbox in settings.
            .filter(|r| match (self.collection.as_deref(), selected) {
                (Some(FAMILY), _) => r.family,
                (Some(UNPLAYED), _) => !r.family && r.playtime_min == Some(0) && !hidden.contains(&r.appid),
                (Some(READY), _) => r.ready && !has_update(r) && !hidden.contains(&r.appid),
                (Some(UPDATES), _) => r.ready && has_update(r),
                (Some(id), _) if id.starts_with(smartcoll::PREFIX) => smart_rules.as_ref().is_some_and(|s| s.fits(r, None, has_update(r))) && !hidden.contains(&r.appid),
                (_, Some(c)) => c.apps.contains(&r.appid),
                _ => !hidden.contains(&r.appid) && (self.settings.show_family || !r.family),
            })
            .filter(|r| r.name.to_lowercase().contains(&filter))
            .collect();
        // Genre: the game's first 8 store tags (like the most prominent tags on the Steam page).
        for g in self.genre.iter().chain(smart.as_ref().and_then(|s| s.genre.as_ref())) {
            let st = net.store.lock().unwrap();
            rows.retain(|r| {
                st.apps.get(&r.appid).is_some_and(|a| a.tags.iter().take(8).any(|t| t.name.as_deref() == Some(g.as_str())))
            });
        }
        // Names in lower case once per row (not two new strings on every comparison).
        let lower: HashMap<u32, String> = rows.iter().map(|r| (r.appid, r.name.to_lowercase())).collect();
        let by_name = |a: &Row, b: &Row| lower[&a.appid].cmp(&lower[&b.appid]);
        match self.settings.sort {
            // As in Steam: just by name (installed only - by the checkbox).
            settings::Sort::Name => rows.sort_by(by_name),
            settings::Sort::LastPlayed => {
                rows.sort_by(|a, b| b.last_played.cmp(&a.last_played).then(b.installed.cmp(&a.installed)).then(by_name(a, b)))
            }
            settings::Sort::Playtime => rows.sort_by(|a, b| {
                b.playtime_min.unwrap_or(0).cmp(&a.playtime_min.unwrap_or(0)).then(by_name(a, b))
            }),
            settings::Sort::Size => rows.sort_by(|a, b| b.size.cmp(&a.size).then(by_name(a, b))),
        }
        rows
    }
}
