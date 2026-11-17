//! Settings kept between runs (eframe persistence: %APPDATA%\Vaporust\data\app.ron).
//! eframe remembers the window size and position by itself.

use serde::{Deserialize, Serialize};

/// Order of games in the library.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Default, Debug)]
pub enum Sort {
    /// Installed on top, then by name.
    #[default]
    Name,
    /// By playtime (more - higher).
    Playtime,
    /// Recently played on top.
    LastPlayed,
    /// By size on disk (larger - higher).
    Size,
}

impl Sort {
    pub const ALL: [Sort; 4] = [Sort::Name, Sort::LastPlayed, Sort::Playtime, Sort::Size];

    pub fn label(self) -> &'static str {
        match self {
            Sort::Name => tr!("по имени", "by name"),
            Sort::Playtime => tr!("по времени в игре", "by playtime"),
            Sort::LastPlayed => tr!("недавно запущенные", "recently played"),
            Sort::Size => tr!("по размеру на диске", "by size on disk"),
        }
    }
}

/// Library view.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Default, Debug)]
pub enum View {
    /// Compact list (as in the first versions).
    #[default]
    List,
    /// Grid of covers.
    Grid,
}

/// Friends status (Steam EPersonaState).
#[allow(non_snake_case)]
pub fn PERSONAS() -> [(u32, &'static str); 4] {
    [(1, tr!("В сети", "Online")), (3, tr!("Отошел", "Away")), (2, tr!("Не беспокоить", "Do not disturb")), (7, tr!("Невидимка", "Invisible"))]
}

pub fn persona_label(state: u32) -> &'static str {
    PERSONAS().iter().find(|(s, _)| *s == state).map_or(tr!("Невидимка", "Invisible"), |(_, l)| l)
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Settings {
    pub freeze_webhelper: bool,
    /// Freeze softly (low priority, efficiency mode) instead of suspending steamwebhelper.
    pub freeze_soft: bool,
    /// Launch flags of the hidden Steam (local::DEFAULT_STEAM_ARGS by default).
    pub steam_args: String,
    pub only_installed: bool,
    pub show_friends: bool,
    /// Library folder for installing new games.
    pub library: Option<String>,
    pub sort: Sort,
    /// Friends status (1 online, 3 away, 2 busy, 7 invisible).
    pub persona: u32,
    /// The window close button minimizes to the tray instead of closing the program.
    pub close_to_tray: bool,
    /// Sound on a new message when the window is not active.
    pub notify_sound: bool,
    /// Notifications: the general checkbox and individual kinds.
    pub notify_on: bool,
    pub notify_msg: bool,
    pub notify_friend_game: bool,
    pub notify_friend_online: bool,
    pub notify_wishlist: bool,
    pub notify_download: bool,
    /// New Steam notifications (gifts, items, trades, comments).
    pub notify_steam: bool,
    /// Wishlist discounts already reported: appid -> discount end (unix).
    pub notified_sales: std::collections::BTreeMap<u32, u64>,
    /// Own room effects (the last known list - so the panel opens right away).
    pub chat_effects: Vec<String>,
    /// Update games via the hidden Steam (by default - without Steam, with the Vaporust downloader).
    pub update_via_steam: bool,
    /// Vaporust put Steam into offline mode (loginusers.vdf) - clear it when Steam closes.
    pub steam_offline_set: bool,
    /// Install queue; kept between runs.
    pub downloads: Vec<QueueItem>,
    /// Old queue format (alpha.2 before the fix): only read and moved into `downloads`.
    #[serde(skip_serializing)]
    pub queue: Vec<(u32, String)>,
    /// The download that was running when Vaporust was closed (or crashed): on the next run
    /// it goes first in the queue.
    pub current_download: Option<QueueItem>,
    /// Interrupted downloads resume by themselves (otherwise they are queued paused).
    pub resume_downloads: bool,
    /// The whole queue is held by the "Pause queue" button.
    pub queue_paused: bool,
    /// Store price region (country code); None - by the IP of the Steam connection.
    pub store_country: Option<String>,
    /// List or grid.
    pub view: View,
    /// Games Vaporust started downloading and has not finished: while the download is not running, their
    /// appmanifest is hidden from Steam, otherwise Steam would start downloading them itself.
    pub holding: Vec<u32>,
    /// Pinned game versions: appid -> real depot versions (depot -> manifest ID).
    /// In the appmanifest of such games Vaporust writes the latest versions so Steam does not update them.
    pub pinned: std::collections::BTreeMap<u32, std::collections::BTreeMap<u32, u64>>,
    /// Check for Vaporust updates on startup.
    pub check_updates: bool,
    /// Games launched without Steam (checkbox in "Properties").
    pub no_steam: std::collections::BTreeSet<u32>,
    /// Show family library games in "All games".
    pub show_family: bool,
    /// Download schedule: window hours [sched_from, sched_to).
    pub sched_from: u8,
    pub sched_to: u8,
    /// Download only during window hours.
    pub sched_only: bool,
    /// A separate speed limit during window hours (KB/s, 0 - unlimited).
    pub sched_limit_on: bool,
    pub sched_limit_kb: u64,
    /// Remembered launch tables for games without Steam (so they also start without internet).
    pub direct_specs: std::collections::BTreeMap<u32, crate::direct::LaunchSpec>,
    /// Game download speed limit, KB/s (0 - unlimited).
    pub download_limit_kb: u64,
    /// Proxy for Aurelia requests (http://host:port or socks5://...), empty - no proxy.
    pub proxy: String,
    /// Commands before launch and after exit for each game (game properties -> "General").
    pub scripts: std::collections::BTreeMap<u32, crate::scripts::GameScripts>,
    /// Favorite friends (at the top of the list) and own notes about friends.
    /// The "What to play?" button above the library.
    pub random_button: bool,
    /// The "What's new" shelf (news of own games) on the library home.
    pub whats_new: bool,
    /// Normal Windows window frame instead of our own.
    pub system_frame: bool,
    /// The GPU has already been checked for the window frame (Intel -> normal Windows frame).
    pub frame_checked: bool,
    /// Remember the main window size and position (and the size/position themselves).
    pub remember_window: bool,
    /// Quick sell: how much cheaper than the lowest listing (minimal currency units; 0 - by
    /// one currency price step).
    pub quick_undercut: i64,
    /// Accounts (SteamID) that answered "Not mine" to the old shared playtime history.
    pub legacy_declined: Vec<u64>,
    /// No-login mode: only installed games from Steam files and launching via Steam,
    /// no Steam password is entered in Vaporust and no Steam session is started.
    pub local_mode: bool,
    /// Experimental features (selling, trades, version rollback, public actions).
    pub experimental: bool,
    /// Interface language: "ru", "en" or "" (as in Windows).
    pub language: String,
    /// Light mode: less background work (see lite.rs).
    pub light_mode: bool,
    pub window: Option<crate::winpos::WinPos>,
    /// Performance: animations (stickers, GIFs, chat effects, loading icons).
    pub animations: bool,
    /// Game covers and backgrounds in the library.
    pub covers: bool,
    /// Memory saving: smaller images, smaller image cache.
    pub economy: bool,
    /// Give memory back to the system when the window is in the tray or a game is running.
    pub trim_memory: bool,
    pub fav_friends: std::collections::BTreeSet<u64>,
    pub friend_notes: std::collections::BTreeMap<u64, String>,
    /// Own notes on games (appid -> text).
    pub game_notes: std::collections::BTreeMap<u32, String>,
    /// Startup section: 0 - Library, 1 - Store.
    pub start_view: u8,
    /// Collapsed friend groups: 0 in game, 1 online, 2 offline.
    pub friend_groups_closed: std::collections::BTreeSet<u8>,
    /// Wishlisted games that are not released yet (to report when they come out).
    pub wish_upcoming: std::collections::BTreeSet<u32>,
    /// Interface scale (1.0 - 100%).
    pub ui_scale: f32,
    /// Saved library filters (dynamic collections).
    pub saved_filters: Vec<SavedFilter>,
    /// The first-run wizard is completed (or skipped).
    pub wizard_done: bool,
    /// Download region (Steam server cell id), 0 - automatic.
    pub download_cell: u32,
    /// Download even while Steam is open (into a temporary folder Steam does not see).
    pub download_with_steam: bool,
    /// New non-Steam games - right away into the real Steam too; they wait for Steam to close.
    pub custom_to_steam: bool,
    pub pending_shortcuts: Vec<u32>,
    /// Games downloading into the temporary folder: appid -> real library.
    pub staged: std::collections::BTreeMap<u32, String>,
    /// Non-Steam games added with the "Add a game" button.
    pub custom_games: Vec<crate::custom::CustomGame>,
}

/// Saved library filter: search, genre, collection, "installed only".
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct SavedFilter {
    pub name: String,
    pub search: String,
    pub genre: Option<String>,
    pub collection: Option<String>,
    pub only_installed: bool,
}

/// A game in the install queue.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct QueueItem {
    pub appid: u32,
    pub name: String,
    /// Paused: the queue skips it until ▶ is pressed.
    #[serde(default)]
    pub paused: bool,
}

impl Settings {
    /// Migrate the old-format queue.
    pub fn migrate(&mut self) {
        // The previous run closed mid-download - resume it first.
        if let Some(mut c) = self.current_download.take() {
            if !self.downloads.iter().any(|q| q.appid == c.appid) {
                c.paused = !self.resume_downloads;
                self.downloads.insert(0, c);
            }
        }
        for (appid, name) in std::mem::take(&mut self.queue) {
            if !self.downloads.iter().any(|q| q.appid == appid) {
                self.downloads.push(QueueItem { appid, name, paused: false });
            }
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            freeze_webhelper: true,
            freeze_soft: false,
            steam_args: crate::local::DEFAULT_STEAM_ARGS.to_string(),
            only_installed: false,
            show_friends: true,
            library: None,
            sort: Sort::Name,
            persona: 7,
            close_to_tray: true,
            notify_sound: true,
            notify_on: true,
            notify_msg: true,
            notify_friend_game: true,
            notify_friend_online: false,
            notify_wishlist: true,
            notify_download: true,
            notify_steam: true,
            notified_sales: Default::default(),
            chat_effects: Vec::new(),
            update_via_steam: false,
            steam_offline_set: false,
            downloads: Vec::new(),
            queue: Vec::new(),
            queue_paused: false,
            current_download: None,
            resume_downloads: true,
            store_country: None,
            view: View::Grid,
            holding: Vec::new(),
            pinned: Default::default(),
            check_updates: true,
            no_steam: Default::default(),
            show_family: true,
            sched_from: 1,
            sched_to: 7,
            sched_only: false,
            sched_limit_on: false,
            sched_limit_kb: 0,
            direct_specs: Default::default(),
            download_limit_kb: 0,
            proxy: String::new(),
            scripts: Default::default(),
            random_button: true,
            whats_new: true,
            system_frame: false,
            frame_checked: false,
            remember_window: true,
            quick_undercut: 0,
            legacy_declined: Vec::new(),
            local_mode: false,
            experimental: false,
            light_mode: false,
            language: String::new(),
            window: None,
            animations: true,
            covers: true,
            economy: false,
            trim_memory: true,
            fav_friends: Default::default(),
            friend_notes: Default::default(),
            game_notes: Default::default(),
            start_view: 0,
            friend_groups_closed: Default::default(),
            wish_upcoming: Default::default(),
            ui_scale: 1.0,
            saved_filters: Vec::new(),
            wizard_done: false,
            download_cell: 0,
            download_with_steam: true,
            custom_to_steam: true,
            pending_shortcuts: Vec::new(),
            staged: Default::default(),
            custom_games: Vec::new(),
        }
    }
}

pub const KEY: &str = "vaporust_settings";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interrupted_download_resumes() {
        let mut s = Settings::default();
        s.downloads.push(QueueItem { appid: 2, name: "b".into(), paused: false });
        s.current_download = Some(QueueItem { appid: 1, name: "a".into(), paused: false });
        s.migrate();
        assert_eq!(s.downloads.iter().map(|q| q.appid).collect::<Vec<_>>(), vec![1, 2]);
        assert!(!s.downloads[0].paused && s.current_download.is_none());
        let mut s = Settings { resume_downloads: false, ..Default::default() };
        s.current_download = Some(QueueItem { appid: 1, name: "a".into(), paused: false });
        s.migrate();
        assert!(s.downloads[0].paused, "unchecked - paused");
    }
}
