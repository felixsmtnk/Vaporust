#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[macro_use]
mod lang;
mod activity;
mod anim;
mod autostart;
mod backup;
mod badges;
mod chatfmt;
mod chatwin;
mod colls;
mod community;
#[cfg(all(windows, target_arch = "x86"))]
mod compat32;
mod covers;
mod effects;
mod groupchat;
mod errlog;
mod gamemisc;
mod export;
mod family;
mod feed;
mod failtests;
mod fixtures;
mod coplay;
mod shortcuts;
mod grouppage;
mod wizard;
mod probe;
mod actions;
mod profileedit;
mod cartui;
mod music;
mod dialogs;
mod accounts;
mod trade;
mod tradeui;
mod direct;
mod icons;
mod fonts;
mod imgload;
mod winframe;
mod winpos;
mod invent;
mod invwin;
mod exp;
mod lite;
mod restore;
mod logo;
mod launchopts;
mod libview;
mod local;
mod net;
mod newsfeed;
mod notices;
mod notify;
mod pinning;
mod points;
mod portable;
mod schedule;
mod scripts;
mod profile_extra;
mod props;
mod refund;
mod richtext;
mod sessions;
mod settings;
mod settingsui;
mod shotcomments;
mod shots;
mod popup;
mod keys;
mod social;
mod socialwin;
mod stats;
mod stats_ui;
mod steamnotif;
mod storage;
mod store;
mod spin;
mod storewin;
mod tour;
mod theme;
mod nav;
mod custom;
mod friendspage;
mod downloads;
mod tray;
mod uninstall;
mod wallet;
mod updater;
mod webapi;
mod workshopui;

/// What is shown in the center of the main window.
#[derive(PartialEq, Eq, Hash, Clone, Copy)]
pub enum View {
    Library,
    Profile,
    Friends,
    Downloads,
    Store,
    Inventory,
    Workshop,
    Settings,
    Screenshots,
    Storage,
    News,
    Badges,
    Points,
    Family,
    Activity,
    Stats,
    Community,
    Groups,
    /// Community group page.
    Group,
    /// Editing own profile.
    ProfileEdit,
    /// Game achievements and game properties - as pages of the main window.
    Achievements,
    Props,
    /// Store cart.
    Cart,
    /// Soundtracks from the library.
    Music,
}

use eframe::egui;
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
};

use local::{Game, SteamInfo};
use net::{Cmd, NetState, Shared};

/// Special "filter collection": games from the Steam family library.
const FAMILY: &str = "__family";
/// Special "collection": own games that were never launched.
const UNPLAYED: &str = "__unplayed";
/// Built-in filters, as in Steam: installed and ready to play; has an update.
const READY: &str = "__ready";
const UPDATES: &str = "__updates";

/// One library row: installed and owned games combined.
#[derive(Clone)]
struct Row {
    appid: u32,
    name: String,
    /// Present on disk (possibly not finished downloading).
    installed: bool,
    /// Fully installed, playable.
    ready: bool,
    /// Needs an update (by Steam's mark or a check via Aurelia).
    needs_update: bool,
    playtime_min: Option<u32>,
    /// Size on disk, bytes (0 - not installed).
    size: u64,
    /// Last launch, unix time (0 - unknown).
    last_played: u64,
    /// Game from the Steam family library (not owned by this account).
    family: bool,
    /// SteamID64 of the family game owner, if known.
    owner: Option<u64>,
}

struct LauncherApp {
    // Local Steam
    steam: Option<SteamInfo>,
    games: Vec<Game>,
    launch_status: Arc<Mutex<String>>,
    launching: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
    /// A Steam task has finished: rescan the disk and check for updates.
    rescan_needed: Arc<AtomicBool>,
    /// The update check has already been sent for the current login.
    updates_checked: bool,
    /// Pinned games whose version masking is already refreshed for launch.
    pin_ok: std::collections::HashSet<u32>,
    /// Pinned games whose outdated masking was already refreshed (once per session).
    pin_stale_sent: std::collections::HashSet<u32>,
    /// Vaporust updates: check/install state; the bar is hidden with the "Later" button.
    upd: updater::Shared,
    upd_hidden: bool,
    /// The check was started by the user (then "no updates" and errors are shown too).
    upd_manual: bool,
    /// Found duplicate installs ("Settings" tab).
    duplicates: Option<Vec<(u32, String, Vec<std::path::PathBuf>)>>,
    /// Proxy saved (show "takes effect after restart").
    proxy_note: bool,
    /// Text of the "Download speed" field (MB/s); None - take it from settings.
    limit_text: Option<String>,
    /// The same for the scheduled speed.
    sched_text: Option<String>,
    /// Friends status was already announced for the current login.
    persona_sent: bool,
    /// Collections were already requested for the current login.
    colls_requested: bool,
    /// Selected collection in the library (None - all games).
    collection: Option<String>,
    /// Which window frame is already applied (Some(true) - normal Windows frame).
    pub(crate) frame_applied: Option<bool>,
    /// Section tour with screenshots (VAPORUST_TOUR, for development).
    pub(crate) tour: Option<tour::Tour>,
    /// Section in the previous frame (to clear the image cache on navigation).
    pub(crate) mem_place: Option<(View, Option<u32>)>,
    /// Open game page in the library.
    pub(crate) lib_page: Option<u32>,
    /// For which game page achievements and news were already requested.
    pub(crate) page_loaded: Option<u32>,
    /// How many unread there were in the previous frame (for the window title).
    last_unread: u32,
    /// When the queue last started an install.
    queue_started: Option<std::time::Instant>,
    /// When we last checked what to hide from Steam.
    stash_check: Option<std::time::Instant>,
    freeze_webhelper: bool,

    // Network (Aurelia)
    net: Shared,
    /// Copy of the network state for frames and the change number it was taken at.
    net_snap: NetState,
    net_gen: u64,
    net_snap_at: std::time::Instant,
    /// Own profile requested (name for the top bar).
    own_profile_asked: bool,
    /// Memory was already given back to the system (window in the tray or a game is running) and when last.
    trimmed: bool,
    /// Window for adding or editing a non-Steam game.
    custom_edit: Option<custom::Edit>,
    /// Uninstall confirmation: (appid, name, size on disk).
    pub(crate) uninstall_ask: Option<(u32, String, u64)>,
    /// Own game note window: (appid, name, text).
    pub(crate) note_edit: Option<(u32, String, String)>,
    /// When we last checked for a request to launch a game (shortcut).
    run_check: Option<std::time::Instant>,
    /// "My licenses": search; wallet history already requested.
    lic_filter: String,
    wallet_asked: bool,
    /// The startup section is already chosen.
    start_applied: bool,
    /// Scale that Vaporust set itself (otherwise it was changed with Ctrl+plus/minus).
    zoom_applied: f32,
    /// For which scale (points per pixel) the minimum window size is already set.
    min_ppp_sent: f32,
    /// When the window was last enlarged to the minimum size.
    min_grow_at: Option<std::time::Instant>,
    /// First-run wizard step.
    wizard_step: usize,
    /// Which game Vaporust is launching (its state is shown on the game page).
    status_app: Option<u32>,
    /// Last shown state (a new one becomes a popup notification).
    status_seen: String,
    /// "Friends" page.
    friends_page: friendspage::FriendsPage,
    trim_at: std::time::Instant,
    tx: tokio::sync::mpsc::UnboundedSender<Cmd>,

    // Interface
    filter: String,
    /// Genre filter (store tag) and a flag that game details were already requested.
    genre: Option<String>,
    genre_asked: bool,
    only_installed: bool,
    login_user: String,
    login_pass: String,
    login_code: String,
    ach_open: Option<(u32, String)>,
    ach_state: AchState,
    /// Open "Game properties" window.
    props: Option<props::PropsWin>,
    closing: Arc<AtomicBool>,
    /// When "Close game" was clicked (appid, time): after 10 s it can be closed forcefully.
    stop_asked: Option<(u32, std::time::Instant)>,
    /// How many times in a row the download resumed after a lost connection.
    resume_tries: u8,
    /// Account info already requested (card in "Settings").
    account_asked: bool,
    /// Cache of "is Steam running?" (process polling at most once per 2 s).
    steam_check: Option<(std::time::Instant, bool)>,

    // Friends and chat
    show_friends: bool,
    friend_filter: String,
    /// Open chat (separate Windows window).
    chat: chatwin::SharedChat,
    /// appids whose names were already requested.
    requested_apps: std::collections::HashSet<u32>,

    /// Covers for the grid view (loaded in the background, visible only).
    covers: covers::Covers,
    /// Workshop search on the game page.
    ws_ui: workshopui::WsUi,
    /// Open Workshop window: (appid, game name).
    ws_open: Option<(u32, String)>,
    /// "Screenshots" section (open while Some).
    shots_win: Option<shots::ShotsWin>,
    /// "Storage" tab (while open).
    storage_win: Option<storage::StorageWin>,
    /// "Activate a key" window: entered text (None - window closed).
    key_input: Option<String>,
    key_refreshed: bool,
    /// "News" tab.
    news_ui: newsfeed::NewsUi,
    /// Steam notifications: when we asked and which were already seen (for popups about new ones).
    notifs_at: Option<std::time::Instant>,
    notifs_seen: std::collections::HashSet<String>,
    /// "Badges" tab.
    badges_ui: badges::BadgesUi,
    /// "Steam Points" tab.
    points_ui: points::PointsUi,
    /// "Family" tab.
    family_ui: family::FamilyUi,
    group_ui: grouppage::GroupUi,
    edit_ui: profileedit::EditUi,
    cart_ui: cartui::CartUi,
    music: music::Music,
    /// Install confirmation on double click: (appid, name).
    install_ask: Option<(u32, String)>,
    dialogs: dialogs::Dialogs,
    accounts_open: bool,
    accounts_noted: Option<String>,
    award_asked: bool,
    award_pick: Option<(u32, u32)>,
    /// "Friends activity" tab.
    activity_ui: activity::ActivityUi,
    /// "Statistics" tab and recording of playtime history.
    stats_ui: stats_ui::StatsUi,
    /// Screenshot comments window.
    shot_comments: Option<shotcomments::ShotComments>,
    /// Game "Community" tab.
    community_ui: community::CommunityUi,
    /// "Group chats" tab.
    groups_ui: groupchat::GroupsUi,
    /// Started with --tray: hide the window to the tray as soon as it appears.
    start_hidden: bool,
    /// The window is already placed at the saved position.
    window_placed: bool,
    /// Start with Windows (cached registry value for the checkbox).
    autostart_on: Option<bool>,
    /// Result of the game list export.
    export_note: Option<String>,
    /// Licenses (purchase dates) already requested.
    licenses_asked: bool,
    /// The "Where you are logged in" list is expanded.
    sessions_open: bool,
    /// Open settings section (settingsui::SECTIONS()).
    settings_section: u8,
    /// egui context (for background threads that need to wake the window).
    egui_ctx: egui::Context,
    /// Workshop, store, inventory opened in a separate window (Ctrl+click).
    ws_detached: bool,
    store_detached: bool,
    inv_detached: bool,
    /// What is shown in the center of the main window.
    view: View,
    /// Profile, "Add friend" and uninstall confirmation windows.
    social_ui: socialwin::SocialUi,
    /// Open "Steam Store" window.
    store_win: Option<storewin::StoreWin>,
    inv_win: Option<invwin::InvWin>,
    /// Other saved settings (sorting, status, tray, sound).
    settings: settings::Settings,
    /// When games were last launched (from Steam's localconfig.vdf).
    last_played: HashMap<u32, u64>,

    // Installation
    libraries: Vec<std::path::PathBuf>,
    lib_idx: usize,
    seen_disk_changed: u64,
    /// When the disk was last rescanned and whether the window was active (for auto_rescan).
    last_rescan: std::time::Instant,
    was_focused: bool,
    /// Last download schedule check.
    sched_check: Option<std::time::Instant>,
    /// Last reconnect attempt in offline mode.
    offline_retry: Option<std::time::Instant>,
    /// Notifications about friends, discounts and downloads.
    notices: notices::Notices,
    /// Speed limit sent to Aurelia (KB/s).
    sent_limit: Option<u64>,
    install_note: String,
}

impl LauncherApp {
    fn new(cc: &eframe::CreationContext) -> Self {
        popup::test_from_env();
        let ctx = &cc.egui_ctx;
        // Settings from the previous run (or defaults).
        let saved: settings::Settings = cc
            .storage
            .and_then(|s| eframe::get_value::<settings::Settings>(s, settings::KEY))
            .map(|mut s| {
                s.migrate();
                s
            })
            .unwrap_or_default();
        crate::lang::set_en(crate::lang::resolve(&saved.language));
        // Intel GPUs: with the frame drawn by the program, on some laptops Windows draws its own
        // title bar on top (a black bar, the mouse misses). There the normal Windows frame is the
        // default; checked once, then it is the user's choice in "Settings -> Window".
        let mut saved = saved;
        if !saved.frame_checked {
            // Without OpenGL it cannot be checked - we will try on the next run.
            if let Some(gl) = cc.gl.as_ref() {
                saved.frame_checked = true;
                use eframe::glow::HasContext;
                let (vendor, renderer) = unsafe { (gl.get_parameter_string(eframe::glow::VENDOR), gl.get_parameter_string(eframe::glow::RENDERER)) };
                if crate::winframe::needs_system_frame(&vendor, &renderer) {
                    saved.system_frame = true;
                    crate::errlog::write(tr!("окно", "window"), &trf!("видеокарта {vendor} / {renderer}: обычная рамка Windows", "graphics {vendor} / {renderer}: normal Windows frame"));
                }
            }
        }
        let net: Shared = Arc::default();
        let tx = net::spawn_worker(net.clone(), ctx.clone());
        // In no-login mode the Steam session is not started at all (no password or login key needed).
        if !saved.local_mode {
            let _ = tx.send(Cmd::Restore);
        }
        // Colors in the style of the Steam client.
        theme::apply(ctx);
        // Scroll bars are always visible (by default they are thin and appear only on hover).
        ctx.style_mut(|s| {
            s.spacing.scroll = egui::style::ScrollStyle::solid();
            // No spinners on loading images: they repaint the window every frame; the loader
            // repaints the window itself when the image arrives.
            s.visuals.image_loading_spinners = false;
        });
        // Image loaders for the chat (http + png/jpeg/gif/webp).
        egui_extras::install_image_loaders(ctx);
        // Our own loader on top: images are downscaled and decoded in the background (saves memory).
        imgload::install(ctx);
        // Windows fallback fonts for characters missing in the built-in ones (otherwise squares).
        fonts::install(ctx);

        // HWND of the main window - for taskbar flashing and for the tray.
        {
            use raw_window_handle::{HasWindowHandle, RawWindowHandle};
            if let Ok(h) = cc.window_handle() {
                if let RawWindowHandle::Win32(w) = h.as_raw() {
                    notify::init(w.hwnd.get());
                    icons::set_taskbar_icon(w.hwnd.get());
                }
            }
        }
        popup::set_sound(saved.notify_sound);
        popup::init(&cc.egui_ctx);
        tray::set_close_to_tray(saved.close_to_tray);
        tray::start();

        let mut app = Self {
            steam: None,
            games: Vec::new(),
            launch_status: Arc::new(Mutex::new(String::new())),
            launching: Arc::new(AtomicBool::new(false)),
            cancel: Arc::new(AtomicBool::new(false)),
            rescan_needed: Arc::new(AtomicBool::new(false)),
            updates_checked: false,
            pin_ok: Default::default(),
            pin_stale_sent: Default::default(),
            upd: Default::default(),
            upd_hidden: false,
            upd_manual: false,
            duplicates: None,
            proxy_note: false,
            limit_text: None,
            sched_text: None,
            persona_sent: false,
            colls_requested: false,
            collection: None,
            lib_page: None,
            mem_place: None,
            page_loaded: None,
            // The window is created without a frame: the command is sent only if settings ask for the normal frame.
            frame_applied: Some(false),
            tour: tour::Tour::from_env(),
            last_unread: 0,
            queue_started: None,
            stash_check: None,
            // On by default (at the user's request), then as saved.
            freeze_webhelper: saved.freeze_webhelper,
            net,
            net_snap: NetState::default(),
            net_gen: u64::MAX,
            net_snap_at: std::time::Instant::now(),
            own_profile_asked: false,
            trimmed: false,
            custom_edit: None,
            uninstall_ask: None,
            note_edit: None,
            run_check: None,
            lic_filter: String::new(),
            wallet_asked: false,
            start_applied: false,
            zoom_applied: 1.0,
            min_ppp_sent: 0.0,
            min_grow_at: None,
            wizard_step: 0,
            status_app: None,
            status_seen: String::new(),
            friends_page: Default::default(),
            trim_at: std::time::Instant::now(),
            tx,
            filter: String::new(),
            genre: None,
            genre_asked: false,
            only_installed: saved.only_installed,
            login_user: String::new(),
            login_pass: String::new(),
            login_code: String::new(),
            ach_open: None,
            ach_state: AchState::default(),
            props: None,
            closing: Arc::new(AtomicBool::new(false)),
            stop_asked: None,
            resume_tries: 0,
            account_asked: false,
            steam_check: None,
            show_friends: saved.show_friends,
            friend_filter: String::new(),
            chat: Default::default(),
            requested_apps: Default::default(),
            libraries: Vec::new(),
            lib_idx: 0,
            seen_disk_changed: 0,
            last_rescan: std::time::Instant::now(),
            was_focused: true,
            install_note: String::new(),
            sched_check: None,
            offline_retry: None,
            notices: Default::default(),
            sent_limit: None,
            covers: covers::Covers::new(ctx, local::find_steam().map(|s| s.dir)),
            social_ui: Default::default(),
            ws_ui: Default::default(),
            ws_open: None,
            shots_win: None,
            storage_win: None,
            key_input: None,
            key_refreshed: false,
            news_ui: Default::default(),
            notifs_at: None,
            notifs_seen: Default::default(),
            badges_ui: Default::default(),
            points_ui: Default::default(),
            family_ui: Default::default(),
            group_ui: Default::default(),
            edit_ui: Default::default(),
            cart_ui: Default::default(),
            music: Default::default(),
            install_ask: None,
            dialogs: Default::default(),
            accounts_open: false,
            accounts_noted: None,
            award_asked: false,
            award_pick: None,
            activity_ui: Default::default(),
            stats_ui: Default::default(),
            shot_comments: None,
            community_ui: Default::default(),
            groups_ui: Default::default(),
            start_hidden: autostart::started_in_tray(),
            window_placed: false,
            autostart_on: None,
            export_note: None,
            licenses_asked: false,
            sessions_open: false,
            settings_section: 0,
            egui_ctx: cc.egui_ctx.clone(),
            ws_detached: false,
            store_detached: false,
            inv_detached: false,
            view: View::Library,
            store_win: None,
            inv_win: None,
            settings: saved,
            last_played: HashMap::new(),
        };
        // Updates: remove the old version file and check GitHub (if enabled).
        updater::cleanup_old();
        if app.settings.check_updates {
            updater::check(&app.upd, ctx);
        }
        app.rescan();
        // Library folder chosen last time.
        if let Some(lib) = app.settings.library.clone() {
            if let Some(i) = app.libraries.iter().position(|l| l.to_string_lossy() == lib) {
                app.lib_idx = i;
            }
        }
        app
    }

    /// Rescan the disk without a button: when returning to the window and every 30 s while it is active
    /// (games may have been installed or removed via Steam). The status at the bottom changes only if
    /// the game count changed, so messages like "Готово." are not overwritten.
    fn auto_rescan(&mut self, ctx: &egui::Context) {
        let focused = ctx.input(|i| i.focused);
        let regained = focused && !self.was_focused;
        self.was_focused = focused;
        let due = focused && self.last_rescan.elapsed() >= crate::lite::secs(30, 120);
        if !(regained || due) || self.launching.load(Ordering::Relaxed) {
            return;
        }
        let before = self.games.len();
        let status = self.launch_status.lock().unwrap().clone();
        self.rescan();
        if self.games.len() == before {
            *self.launch_status.lock().unwrap() = status;
        }
    }

    fn rescan(&mut self) {
        self.last_rescan = std::time::Instant::now();
        self.steam = local::find_steam();
        if let Some(s) = &self.steam {
            effects::set_steam_dir(&s.dir);
        }
        self.games = self
            .steam
            .as_ref()
            .map(|s| local::scan_games(&s.dir))
            .unwrap_or_default();
        // Pinned games are not offered for update.
        for g in &mut self.games {
            if self.settings.pinned.contains_key(&g.appid) {
                g.needs_update = false;
            }
        }
        self.libraries = self
            .steam
            .as_ref()
            .map(|s| local::library_dirs(&s.dir))
            .unwrap_or_default();
        if self.lib_idx >= self.libraries.len() {
            self.lib_idx = 0;
        }
        self.last_played = self
            .steam
            .as_ref()
            .and_then(|s| launchopts::find_localconfig(&s.dir))
            .map(|p| launchopts::read_last_played(&p))
            .unwrap_or_default();
        if !self.launching.load(Ordering::Relaxed) {
            *self.launch_status.lock().unwrap() = match &self.steam {
                Some(_) => String::new(),
                None => tr!("Steam не найден в реестре", "Steam not found in the registry").to_string(),
            };
        }
    }

    /// An unfinished game started in Vaporust (Steam does not consider it its own): its library.
    fn movable_partial(&self, appid: u32) -> Option<std::path::PathBuf> {
        if !self.settings.holding.contains(&appid) {
            return None;
        }
        self.games
            .iter()
            .find(|g| g.appid == appid && !g.ready)
            .and_then(|g| g.dir.parent()?.parent()?.parent().map(|p| p.to_path_buf()))
    }

    fn install(&mut self, appid: u32, name: String) {
        self.install_note.clear();
        let steam_open = local::steam_running();
        // Started in the temporary library (while Steam was open) - continue there.
        if let Some(real) = self.settings.staged.get(&appid).cloned() {
            if !self.settings.holding.contains(&appid) {
                self.settings.holding.push(appid);
            }
            self.settings.current_download = Some(settings::QueueItem { appid, name: name.clone(), paused: false });
            let staging = local::staging_dir(std::path::Path::new(&real));
            let _ = std::fs::create_dir_all(staging.join("steamapps").join("common"));
            let _ = self.tx.send(Cmd::Install { appid, name, library: staging });
            return;
        }
        // Steam is open: an unfinished game in the real library must not be touched (Steam could
        // start downloading it itself), and a new one goes to the temporary library Steam does not see.
        let partial_real = self.games.iter().any(|g| g.appid == appid);
        // Own unfinished one (started in Vaporust) - move it to the temporary folder and finish it there.
        if steam_open && self.settings.download_with_steam && partial_real {
            if let Some(lib) = self.movable_partial(appid) {
                match local::to_staging(&lib, appid) {
                    Ok(()) => {
                        self.settings.staged.insert(appid, lib.to_string_lossy().into_owned());
                        self.rescan();
                        return self.install(appid, name);
                    }
                    Err(e) => {
                        self.install_note = trf!("Не удалось продолжить при открытом Steam: {e}. Закройте Steam.", "Could not continue with Steam open: {e}. Close Steam.");
                        return;
                    }
                }
            }
        }
        if steam_open && (!self.settings.download_with_steam || partial_real) {
            self.install_note = tr!("Сначала закройте Steam (и игру, если она запущена), потом нажмите «Установить» еще раз.", "Close Steam first (and the game if it is running), then press \"Install\" again.").into();
            return;
        }
        // An unfinished game is resumed in the same library where it lies.
        let library = self
            .games
            .iter()
            .find(|g| g.appid == appid)
            .and_then(|g| g.dir.parent()?.parent()?.parent().map(|p| p.to_path_buf()))
            .or_else(|| self.libraries.get(self.lib_idx).cloned());
        let Some(library) = library else {
            self.install_note = tr!("Не найдена папка библиотеки Steam.", "Steam library folder not found.").into();
            return;
        };
        // While the game is unfinished it is "ours": when paused, its appmanifest is hidden from Steam.
        if !self.settings.holding.contains(&appid) {
            self.settings.holding.push(appid);
        }
        // If Vaporust is closed mid-download, it resumes on the next run.
        self.settings.current_download = Some(settings::QueueItem { appid, name: name.clone(), paused: false });
        let library = if steam_open {
            self.settings.staged.insert(appid, library.to_string_lossy().into_owned());
            let staging = local::staging_dir(&library);
            let _ = std::fs::create_dir_all(staging.join("steamapps").join("common"));
            staging
        } else {
            library
        };
        let _ = self.tx.send(Cmd::Install {
            appid,
            name,
            library,
        });
    }

    /// Hide unfinished Vaporust downloads from Steam (paused, cancelled, interrupted),
    /// otherwise the Steam started for a game would begin downloading them itself. Checked every 3 s.
    fn guard_partials(&mut self, net: &NetState) {
        // Finished ones are no longer "ours", Steam takes care of them from now on.
        self.settings.holding.retain(|a| !self.games.iter().any(|g| g.appid == *a && g.ready));
        if self.settings.holding.is_empty()
            || self.stash_check.is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(3))
        {
            return;
        }
        self.stash_check = Some(std::time::Instant::now());
        if self.launching.load(Ordering::Relaxed) || self.closing.load(Ordering::Relaxed) || self.steam_running() {
            return; // with Steam open, the files must not be touched
        }
        let active = net.install.as_ref().filter(|i| i.active()).map(|i| i.appid);
        let mut changed = false;
        for &appid in &self.settings.holding {
            if Some(appid) == active {
                continue;
            }
            let lib = self
                .games
                .iter()
                .find(|g| g.appid == appid && !g.ready && !g.stashed)
                .and_then(|g| g.dir.parent()?.parent()?.parent().map(|p| p.to_path_buf()));
            if let Some(lib) = lib {
                changed |= local::stash_manifest(&lib, appid);
            }
        }
        if changed {
            self.rescan();
        }
    }

    /// Put a game into the install queue (or start right away if nothing is downloading).
    fn enqueue(&mut self, appid: u32, name: String, net: &NetState) {
        if let Some(q) = self.settings.downloads.iter_mut().find(|q| q.appid == appid) {
            q.paused = false; // already queued and paused - "Resume" unpauses it
            return;
        }
        // While Steam is open, Vaporust does not download (Steam could start downloading the same game):
        // the game is queued and starts by itself as soon as Steam is closed. Previously clicking
        // "Install" in this case did nothing visible.
        let partial_real = self.games.iter().any(|g| g.appid == appid) && !self.settings.staged.contains_key(&appid) && self.movable_partial(appid).is_none();
        if self.steam_running() && !self.launching.load(Ordering::Relaxed) && (!self.settings.download_with_steam || partial_real) {
            self.settings.downloads.push(settings::QueueItem { appid, name: name.clone(), paused: false });
            popup::push(
                popup::Kind::Game(appid),
                tr!("Загрузка ждет закрытия Steam", "The download waits for Steam to close"),
                trf!("«{name}» в очереди: начнется сама, когда вы закроете Steam (меню «Vaporust» - «Закрыть Steam»).", "\"{name}\" is queued: it starts by itself when you close Steam (\"Vaporust\" menu - \"Close Steam\")."),
            );
            return;
        }
        let busy = net.install.as_ref().is_some_and(|i| i.active());
        let waiting = self.settings.downloads.iter().any(|q| !q.paused);
        let allowed = schedule::may_download(&self.settings);
        if !allowed {
            self.install_note = trf!(
                "«{name}» в очереди: по расписанию загрузки идут с {} до {}.", "\"{name}\" is queued: by schedule, downloads run from {} to {}.",
                schedule::hh(self.settings.sched_from),
                schedule::hh(self.settings.sched_to)
            );
        }
        if !busy && !waiting && !self.settings.queue_paused && allowed {
            self.install(appid, name);
            self.queue_started = Some(std::time::Instant::now());
        } else {
            self.settings.downloads.push(settings::QueueItem { appid, name, paused: false });
        }
    }

    /// The download is over: no longer "current". If the network broke it (not "Cancel" or
    /// "Pause"), it goes first in the queue and resumes when the connection is back (up to 3 times in a row).
    fn finish_tick(&mut self, net: &NetState) {
        let Some(cur) = self.settings.current_download.clone() else { return };
        let Some(inst) = net.install.as_ref().filter(|i| i.appid == cur.appid) else { return };
        let Some(res) = &inst.result else { return };
        self.settings.current_download = None;
        let user_stop = inst.abort.load(Ordering::Relaxed);
        if let (Err(e), false) = (res, user_stop) {
            crate::errlog::write(tr!("загрузка", "download"), &format!("{}: {e}", cur.name));
        }
        match res {
            Err(e) if should_resume(e, user_stop, self.resume_tries) => {
                self.resume_tries += 1;
                if !self.settings.downloads.iter().any(|q| q.appid == cur.appid) {
                    self.settings.downloads.insert(0, cur.clone());
                }
                self.install_note = trf!(
                    "Загрузка «{}» прервалась из-за связи: продолжится сама, когда Steam снова будет доступен.", "The download of \"{}\" was interrupted by the connection: it continues by itself when Steam is available again.",
                    cur.name
                );
                // Not right away: give the network time to come back.
                self.queue_started = Some(std::time::Instant::now() + std::time::Duration::from_secs(27));
            }
            Ok(()) => {
                self.resume_tries = 0;
                if let Some(real) = self.settings.staged.remove(&cur.appid) {
                    match local::unstage(std::path::Path::new(&real), cur.appid) {
                        Ok(()) => {
                            self.rescan();
                        }
                        Err(e) => {
                            crate::errlog::write(tr!("загрузка", "download"), &trf!("перенос из временной папки: {e}", "moving from the temporary folder: {e}"));
                            popup::push(popup::Kind::Info, tr!("Загрузка", "Download"), trf!("«{}» скачана, но не перенесена в библиотеку: {e}", "\"{}\" is downloaded but not moved to the library: {e}", cur.name));
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// Start the next game from the queue when the downloader is free.
    fn run_queue(&mut self, net: &NetState) {
        let busy = net.install.as_ref().is_some_and(|i| i.active());
        // The install command does not reach the network thread instantly: a 3 s pause between starts.
        let cooling = self.queue_started.is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(3));
        // The first game in the queue that is not paused.
        let next = self.settings.downloads.iter().position(|q| !q.paused);
        if busy
            || cooling
            || self.settings.queue_paused
            || !schedule::may_download(&self.settings)
            || next.is_none()
            || net.account.is_none()
            || self.launching.load(Ordering::Relaxed)
        {
            return;
        }
        if self.steam_running() {
            let q = &self.settings.downloads[next.unwrap()];
            let real_partial = self.games.iter().any(|g| g.appid == q.appid) && !self.settings.staged.contains_key(&q.appid) && self.movable_partial(q.appid).is_none();
            if !self.settings.download_with_steam || real_partial {
                return;
            }
        }
        let q = self.settings.downloads.remove(next.unwrap());
        self.install(q.appid, q.name);
        self.queue_started = Some(std::time::Instant::now());
    }

    /// Result messages (uninstalled, closed, launch error) - as a popup notification:
    /// the status bar at the bottom is gone, and launch progress is shown on the game page.
    fn status_popup(&mut self) {
        let st = self.launch_status.lock().unwrap().clone();
        if st == self.status_seen {
            return;
        }
        let busy = self.launching.load(Ordering::Relaxed) || self.closing.load(Ordering::Relaxed);
        // "Удаляю...", "Прошу закрыться..." are progress, not a result: not shown.
        let in_progress = st.ends_with("...") || st.ends_with('…');
        if !busy && !in_progress && !st.is_empty() && st != tr!("Готово.", "Done.") {
            // The game in quotes - its image on the left, a click opens its page.
            let game = st
                .split_once('«')
                .and_then(|(_, rest)| rest.split_once('»'))
                .map(|(name, _)| name)
                .and_then(|name| {
                    self.games
                        .iter()
                        .find(|g| g.name == name)
                        .map(|g| g.appid)
                });
            let kind = game.map_or(popup::Kind::Info, popup::Kind::Game);
            popup::push(kind, "Vaporust", st.clone());
        }
        self.status_seen = st;
    }

    /// Close the running game: gently (like the window's close button) or forcefully.
    fn stop_running(&mut self, appid: u32, force: bool) {
        let Some((id, name, dir)) = local::RUNNING.lock().unwrap().clone() else { return };
        if id != appid {
            return;
        }
        let n = local::stop_game(&dir, force);
        if !force {
            self.stop_asked = Some((appid, std::time::Instant::now()));
        }
        *self.launch_status.lock().unwrap() = match (force, n > 0) {
            (true, _) => trf!("«{name}» закрыта принудительно.", "\"{name}\" was force-closed."),
            (false, true) => trf!("Прошу «{name}» закрыться...", "Asking \"{name}\" to close..."),
            (false, false) => tr!("Процессы игры не найдены.", "Game processes not found.").into(),
        };
    }

    fn steam_running(&mut self) -> bool {
        let now = std::time::Instant::now();
        match self.steam_check {
            // A process snapshot costs ~5-7 ms - at most once per 5 s (when idle this is the main load).
            Some((t, v)) if now.duration_since(t) < std::time::Duration::from_secs(5) => v,
            _ => {
                let v = local::steam_running();
                self.steam_check = Some((now, v));
                v
            }
        }
    }

    /// Offline mode: clear Steam's offline flag when Steam is closed; without a connection -
    /// try to log in again once a minute.
    fn offline_tick(&mut self, net: &NetState) {
        if self.settings.steam_offline_set
            && !self.launching.load(Ordering::Relaxed)
            && !self.closing.load(Ordering::Relaxed)
            && !self.steam_running()
        {
            if let Some(s) = self.steam.clone() {
                if local::set_steam_offline(&s.dir, false).is_ok() {
                    self.settings.steam_offline_set = false;
                }
            }
        }
        if net.offline && net.account.is_none() && !net.busy && !self.settings.local_mode {
            let due = self.offline_retry.is_none_or(|t| t.elapsed() > std::time::Duration::from_secs(60));
            if due {
                self.offline_retry = Some(std::time::Instant::now());
                let _ = self.tx.send(Cmd::Restore);
            }
        }
    }

    fn close_steam(&mut self, ctx: &egui::Context) {
        let Some(steam) = self.steam.clone() else {
            return;
        };
        self.closing.store(true, Ordering::Relaxed);
        let (status, closing, ctx) = (self.launch_status.clone(), self.closing.clone(), ctx.clone());
        thread::spawn(move || {
            let set = |s: &str| {
                *status.lock().unwrap() = s.to_string();
                ctx.request_repaint();
            };
            local::close_steam(&steam, &set);
            set(tr!("Steam закрыт.", "Steam closed."));
            closing.store(false, Ordering::Relaxed);
        });
        self.steam_check = None;
    }

    fn start(&mut self, appid: u32, ctx: &egui::Context) {
        self.status_app = Some(appid);
        if custom::is_custom(appid) {
            self.launch_custom(appid);
            return;
        }
        self.steam_task(appid, ctx, false);
    }

    /// Update a game via the hidden Steam (without launching the game).
    /// "Update" in the library: without Steam (Aurelia) when possible; otherwise via the hidden Steam.
    pub(crate) fn start_update(&mut self, appid: u32, ctx: &egui::Context) {
        let Some(game) = self.games.iter().find(|g| g.appid == appid).cloned() else { return };
        let acf = local::appmanifest_path(&game);
        let info = local::read_acf(&acf);
        // Waiting for a language change (chosen in "Properties") - Vaporust switches it itself (switch_language).
        let switching = !info.user_language.is_empty()
            && !info.mounted_language.is_empty()
            && info.user_language != info.mounted_language;
        // Beta change - without Steam: Aurelia downloads the chosen branch versions for installed depots.
        let beta = (info.user_beta != info.mounted_beta).then(|| info.user_beta.clone());
        let logged_in = self.net.lock().unwrap().account.is_some();
        // The game is in two libraries: Aurelia takes the alphabetically first copy - may update the wrong one.
        let duplicate = self
            .steam
            .as_ref()
            .is_some_and(|s| local::find_duplicates(&s.dir).iter().any(|(id, _, _)| *id == appid));
        if self.settings.update_via_steam || duplicate || !logged_in || self.settings.pinned.contains_key(&appid) {
            self.steam_task(appid, ctx, true);
            return;
        }
        if self.steam_running() {
            self.install_note = tr!("Сначала закройте Steam (кнопка «Закрыть Steam» внизу), потом нажмите «Обновить» еще раз.", "Close Steam first (the \"Close Steam\" button at the bottom), then press \"Update\" again.").into();
            return;
        }
        if self.net.lock().unwrap().install.as_ref().is_some_and(|i| i.active()) {
            self.install_note = tr!("Сейчас идет другая загрузка - обновите игру, когда она закончится.", "Another download is running - update the game when it finishes.").into();
            return;
        }
        self.install_note.clear();
        if switching {
            // Language first (the beta, if also chosen, on the next "Update" click).
            let _ = self.tx.send(Cmd::SetLanguage { appid, name: game.name.clone(), acf, lang: info.user_language.clone() });
            return;
        }
        let _ = self.tx.send(Cmd::UpdateDirect { appid, name: game.name.clone(), acf, beta });
    }

    /// Update via the hidden Steam (as before).
    pub(crate) fn start_update_steam(&mut self, appid: u32, ctx: &egui::Context) {
        self.steam_task(appid, ctx, true);
    }

    /// "Play" or "Update": both tasks start the hidden Steam, so they go one at a time.
    fn steam_task(&mut self, appid: u32, ctx: &egui::Context, update: bool) {
        if self.launching.load(Ordering::Relaxed) || self.closing.load(Ordering::Relaxed) {
            return;
        }
        // A game without Steam protection - launch directly (checkbox in "Properties").
        let family = is_family_only(&self.net.lock().unwrap(), appid);
        if !update && self.settings.no_steam.contains(&appid) && !family {
            let net = self.net.lock().unwrap().clone();
            self.direct_launch(appid, &net);
            return;
        }
        // A pinned version cannot be updated, and it must be prepared before launch.
        if self.settings.pinned.contains_key(&appid) {
            if update {
                *self.launch_status.lock().unwrap() = tr!("Версия игры закреплена - открепите ее в «Свойствах», чтобы обновить.", "The game version is pinned - unpin it in \"Properties\" to update.").into();
                return;
            }
            let net = self.net.lock().unwrap().clone();
            if !self.pin_gate(appid, &net) {
                return;
            }
        }
        let Some(steam) = self.steam.clone() else {
            return;
        };
        let Some(game) = self.games.iter().find(|g| g.appid == appid).cloned() else {
            return;
        };
        // No connection to Steam: let the hidden Steam start offline right away (without a question window).
        // The flag is cleared when Steam is closed again (offline_tick).
        if self.net.lock().unwrap().offline && !self.steam_running() {
            match local::set_steam_offline(&steam.dir, true) {
                Ok(_) => self.settings.steam_offline_set = true,
                Err(e) => *self.launch_status.lock().unwrap() = trf!("Автономный режим Steam не включен: {e}", "Steam offline mode was not turned on: {e}"),
            }
        }
        let opts = local::LaunchOptions {
            freeze_webhelper: self.freeze_webhelper,
            steam_args: self.settings.steam_args.clone(),
        };
        self.launching.store(true, Ordering::Relaxed);
        self.cancel.store(false, Ordering::Relaxed);
        let status = self.launch_status.clone();
        let launching = self.launching.clone();
        let cancel = self.cancel.clone();
        let rescan = self.rescan_needed.clone();
        let script = self.settings.scripts.get(&appid).cloned().unwrap_or_default();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let set = |s: &str| {
                *status.lock().unwrap() = s.to_string();
                ctx.request_repaint();
            };
            let res = if update {
                local::update_via_steam(&steam, &game, opts, &cancel, &set)
            } else {
                // Launch scripts: a command before launch (we wait for it), after exit (we do not wait).
                if !script.before.trim().is_empty() {
                    set(tr!("Выполняю команду перед запуском...", "Running the pre-launch command..."));
                }
                let warn = scripts::run_before(&script, game.appid, &game.name, &game.dir);
                let res = local::run_chain(&steam, &game, opts, &cancel, &set);
                let after = if res.is_ok() && !cancel.load(Ordering::Relaxed) {
                    scripts::run_after(&script, game.appid, &game.name, &game.dir)
                } else {
                    None
                };
                match (warn.or(after), res) {
                    (Some(w), Ok(msg)) => Ok(format!("{msg} {w}")),
                    (_, r) => r,
                }
            };
            match res {
                Ok(msg) => set(&msg),
                Err(e) => {
                    crate::errlog::write(if update { tr!("обновление", "update") } else { tr!("запуск", "launch") }, &format!("{}: {e}", game.name));
                    set(&e)
                }
            }
            launching.store(false, Ordering::Relaxed);
            // Steam may have updated games - rescan the disk and check for updates again.
            rescan.store(true, Ordering::Relaxed);
            ctx.request_repaint();
        });
    }

    /// All games (owned and installed) without filters and sorting.
    fn row_map(&self, net: &NetState) -> HashMap<u32, Row> {
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
    fn rows(&self, net: &NetState) -> Vec<Row> {
        let map = self.row_map(net);
        let colls = net.collections.as_deref().unwrap_or(&[]);
        let hidden: std::collections::HashSet<u32> = colls
            .iter()
            .find(|c| c.id == "hidden")
            .map(|c| c.apps.iter().copied().collect())
            .unwrap_or_default();
        let selected = self.collection.as_ref().and_then(|id| colls.iter().find(|c| &c.id == id));
        let filter = self.filter.to_lowercase();
        let has_update = |r: &Row| r.needs_update || net.updates.as_ref().is_some_and(|u| u.contains(&r.appid));
        let mut rows: Vec<Row> = map
            .into_values()
            .filter(|r| !self.only_installed || r.installed)
            // Collection: when selected - only its games; "all games" - without "Hidden", as in Steam.
            // "Family library" - only family games; in "All games" - by the checkbox in settings.
            .filter(|r| match (self.collection.as_deref(), selected) {
                (Some(FAMILY), _) => r.family,
                (Some(UNPLAYED), _) => !r.family && r.playtime_min == Some(0) && !hidden.contains(&r.appid),
                (Some(READY), _) => r.ready && !has_update(r) && !hidden.contains(&r.appid),
                (Some(UPDATES), _) => r.ready && has_update(r),
                (_, Some(c)) => c.apps.contains(&r.appid),
                _ => !hidden.contains(&r.appid) && (self.settings.show_family || !r.family),
            })
            .filter(|r| r.name.to_lowercase().contains(&filter))
            .collect();
        // Genre: the game's first 8 store tags (like the most prominent tags on the Steam page).
        if let Some(g) = &self.genre {
            let st = net.store.lock().unwrap();
            rows.retain(|r| {
                st.apps.get(&r.appid).is_some_and(|a| a.tags.iter().take(8).any(|t| t.name.as_deref() == Some(g.as_str())))
            });
        }
        let by_name = |a: &Row, b: &Row| a.name.to_lowercase().cmp(&b.name.to_lowercase());
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

    fn friends_panel(&mut self, ui: &mut egui::Ui, net: &NetState) {
        // Friend list snapshot: (id, name, status, game, unread).
        let mut unknown_apps: Vec<u32> = Vec::new();
        let mut list: Vec<(u64, String, u32, Option<String>, u32)> = {
            let roster = net.roster.read().unwrap();
            roster
                .values()
                // 3 = friend (EFriendRelationship); Steam groups (account type 7) are in the same list - we hide them.
                .filter(|f| f.relationship == 3 && (f.steam_id >> 52) & 0xF == 1)
                .map(|f| {
                    // Game name: from Steam, from our library, from already resolved ones, otherwise the id.
                    let game = f.game_name.clone().filter(|g| !g.is_empty()).or_else(|| {
                        let a = f.game_app_id.filter(|a| *a != 0)?;
                        let known = net
                            .app_names
                            .get(&a)
                            .cloned()
                            .or_else(|| net.owned.iter().find(|g| g.app_id == a).map(|g| g.name.clone()));
                        Some(known.unwrap_or_else(|| {
                            unknown_apps.push(a);
                            trf!("игра {a}", "game {a}")
                        }))
                    });
                    // Own nickname for the friend (set in Steam) comes first, the Steam name in parentheses, as in Steam.
                    // The name has not arrived yet (Steam sends it a bit later for offline friends) - do not show the id.
                    let persona = f.persona_name.clone().filter(|n| !n.is_empty()).unwrap_or_else(|| tr!("загружается...", "loading...").into());
                    let shown = match &f.nickname {
                        Some(n) => format!("{n} ({persona})"),
                        None => persona,
                    };
                    // Detailed in-game status (map, score), if the game reports it.
                    let game = match (game, &f.rich_status) {
                        (Some(g), Some(rs)) => Some(format!("{g} · {rs}")),
                        (g, _) => g,
                    };
                    (
                        f.steam_id,
                        shown,
                        f.persona_state.unwrap_or(0),
                        game,
                        net.chats.get(&f.steam_id).map_or(0, |c| c.unread),
                    )
                })
                .collect()
        };
        // Unknown names are requested once.
        unknown_apps.retain(|a| self.requested_apps.insert(*a));
        if !unknown_apps.is_empty() {
            let _ = self.tx.send(Cmd::ResolveApps(unknown_apps));
        }
        let online = list.iter().filter(|f| f.2 != 0).count();
        // Order: with new messages, in game, online, offline; within each - by name.
        let rank = |f: &(u64, String, u32, Option<String>, u32)| {
            if f.4 > 0 {
                0
            } else if f.3.is_some() {
                1
            } else if f.2 != 0 {
                2
            } else {
                3
            }
        };
        // Favorites on top (after friends with new messages).
        let favs = self.settings.fav_friends.clone();
        list.sort_by(|a, b| {
            let fa = a.4 == 0 && favs.contains(&a.0);
            let fb = b.4 == 0 && favs.contains(&b.0);
            (a.4 == 0).cmp(&(b.4 == 0)).then(fb.cmp(&fa)).then(rank(a).cmp(&rank(b))).then(a.1.to_lowercase().cmp(&b.1.to_lowercase()))
        });
        // Avatars (hash from the friend status): avatars.fastly.steamstatic.com/<hash>.jpg - 32x32.
        let avatars: HashMap<u64, String> = net
            .roster
            .read()
            .unwrap()
            .values()
            .filter_map(|f| f.avatar_hash.as_ref().map(|h| (f.steam_id, format!("https://avatars.fastly.steamstatic.com/{h}.jpg"))))
            .collect();
        let notes = self.settings.friend_notes.clone();

        // Header with the menu, friend requests.
        self.friends_header(ui, net);
        if list.is_empty() {
            ui.horizontal(|ui| {
                crate::spin::spinner(ui);
                ui.weak(tr!("Загружаю список...", "Loading the list..."));
            });
            return;
        }
        ui.add_space(4.0);
        ui.label(egui::RichText::new(trf!("ДРУЗЬЯ В СЕТИ  {online} / {}", "FRIENDS ONLINE  {online} / {}", list.len())).size(11.0).color(theme::WEAK));
        ui.add(
            egui::TextEdit::singleline(&mut self.friend_filter)
                .hint_text(tr!("поиск", "search"))
                .desired_width(f32::INFINITY),
        );
        ui.separator();
        let filter = self.friend_filter.to_lowercase();
        let mut open = None;
        let mut friend_act = None;
        // Groups, as in Steam: favorites, in game, online, offline (collapsible).
        let group_of = |f: &(u64, String, u32, Option<String>, u32)| -> u8 {
            if favs.contains(&f.0) {
                3
            } else if f.2 == 0 {
                2
            } else if f.3.is_some() {
                0
            } else {
                1
            }
        };
        let mut toggle_group: Option<u8> = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for (gid, gname) in [(3u8, tr!("ИЗБРАННЫЕ", "FAVORITES")), (0, tr!("В ИГРЕ", "IN GAME")), (1, tr!("В СЕТИ", "ONLINE")), (2, tr!("НЕ В СЕТИ", "OFFLINE"))] {
            let members: Vec<&(u64, String, u32, Option<String>, u32)> = list
                .iter()
                .filter(|f| group_of(f) == gid)
                .filter(|f| {
                    let note = notes.get(&f.0).map(|n| n.to_lowercase()).unwrap_or_default();
                    filter.is_empty() || f.1.to_lowercase().contains(&filter) || note.contains(&filter)
                })
                .collect();
            if members.is_empty() {
                continue;
            }
            // Group header: a click collapses it (all groups are expanded while searching).
            let closed = filter.is_empty() && self.settings.friend_groups_closed.contains(&gid);
            let head = ui
                .horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 14.0), egui::Sense::hover());
                    let c = r.center();
                    let pts = if closed {
                        vec![c + egui::vec2(-2.5, -4.0), c + egui::vec2(3.0, 0.0), c + egui::vec2(-2.5, 4.0)]
                    } else {
                        vec![c + egui::vec2(-4.0, -2.0), c + egui::vec2(4.0, -2.0), c + egui::vec2(0.0, 3.0)]
                    };
                    ui.painter().add(egui::Shape::convex_polygon(pts, theme::WEAK, egui::Stroke::NONE));
                    ui.label(egui::RichText::new(format!("{gname}  ({})", members.len())).size(11.0).strong().color(theme::WEAK));
                })
                .response;
            let head = ui.interact(head.rect, ui.id().with(("friend_group", gid)), egui::Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
            if head.clicked() {
                toggle_group = Some(gid);
            }
            ui.add_space(2.0);
            if closed {
                ui.add_space(4.0);
                continue;
            }
            for &(id, name, state, game, unread) in members.iter().map(|f| (&f.0, &f.1, &f.2, &f.3, &f.4)).collect::<Vec<_>>().iter() {
                let note = notes.get(id).cloned().unwrap_or_default();
                let (color, status) = match (state, game) {
                    (_, Some(g)) => (egui::Color32::from_rgb(140, 200, 80), g.clone()),
                    (0, _) => (egui::Color32::GRAY, tr!("не в сети", "offline").into()),
                    (1, _) => (egui::Color32::from_rgb(90, 170, 230), tr!("в сети", "online").into()),
                    (2, _) => (egui::Color32::from_rgb(220, 120, 110), tr!("не беспокоить", "do not disturb").into()),
                    (3 | 4, _) => (egui::Color32::from_rgb(110, 140, 170), tr!("отошел", "away").into()),
                    _ => (egui::Color32::from_rgb(90, 170, 230), tr!("в сети", "online").into()),
                };
                let mut title = egui::RichText::new(crate::richtext::fix(ui, name)).color(if *state == 0 { egui::Color32::GRAY } else { color });
                if *unread > 0 {
                    title = title.strong();
                }
                // The whole row (avatar, name, status) is one button across the panel width.
                let block = ui
                    .horizontal(|ui| {
                    // Avatar with a status-colored border, as in Steam.
                    let (ar, _) = ui.allocate_exact_size(egui::vec2(32.0, 32.0), egui::Sense::hover());
                    match avatars.get(id) {
                        Some(u) => {
                            egui::Image::new(u.as_str()).rounding(2.0).paint_at(ui, ar);
                        }
                        None => {
                            ui.painter().rect_filled(ar, 2.0, theme::CARD);
                        }
                    }
                    ui.painter().rect_stroke(ar.expand(1.0), 2.0, egui::Stroke::new(1.5, if *state == 0 { egui::Color32::from_gray(90) } else { color }));
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.add(egui::Label::new(title).truncate());
                            if favs.contains(id) {
                                ui.label(egui::RichText::new(tr!("избранный", "favorite")).small().color(egui::Color32::from_rgb(230, 190, 80)));
                            }
                            if *unread > 0 {
                                ui.colored_label(egui::Color32::from_rgb(240, 200, 80), format!("● {unread}"));
                            }
                        });
                        ui.add(egui::Label::new(egui::RichText::new(&status).small().color(color)).truncate());
                        if !note.is_empty() {
                            ui.label(egui::RichText::new(&note).small().italics().weak());
                        }
                    });
                    })
                    .response;
                let rect = egui::Rect::from_min_max(
                    egui::pos2(ui.max_rect().left(), block.rect.top() - 2.0),
                    egui::pos2(ui.max_rect().right(), block.rect.bottom() + 2.0),
                );
                let resp = ui
                    .interact(rect, ui.id().with(("friend", *id)), egui::Sense::click())
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    // Mini profile on hover, as in Steam: large avatar, name, status, note.
                    .on_hover_ui(|ui| {
                        ui.horizontal(|ui| {
                            let (ar, _) = ui.allocate_exact_size(egui::vec2(64.0, 64.0), egui::Sense::hover());
                            match avatars.get(id) {
                                Some(u) => {
                                    egui::Image::new(u.replace(".jpg", "_medium.jpg")).rounding(3.0).paint_at(ui, ar);
                                }
                                None => {
                                    ui.painter().rect_filled(ar, 3.0, theme::CARD);
                                }
                            }
                            ui.painter().rect_stroke(ar.expand(1.0), 3.0, egui::Stroke::new(2.0, if *state == 0 { egui::Color32::from_gray(90) } else { color }));
                            ui.vertical(|ui| {
                                ui.label(egui::RichText::new(crate::richtext::fix(ui, name)).size(15.0).strong());
                                ui.label(egui::RichText::new(&status).color(color));
                                if !note.is_empty() {
                                    ui.label(egui::RichText::new(&note).italics().weak());
                                }
                                ui.weak(tr!("Щелчок - чат, правый щелчок - меню", "Click - chat, right click - menu"));
                            });
                        });
                    });
                if resp.hovered() {
                    ui.painter().rect_filled(
                        rect,
                        3.0,
                        egui::Color32::from_rgba_unmultiplied(255, 255, 255, 14),
                    );
                }
                if resp.clicked() {
                    open = Some((*id, name.clone()));
                }
                // Right click: profile, message, remove from friends.
                socialwin::friend_menu(&resp, *id, name, favs.contains(id), &mut friend_act);
                ui.add_space(4.0);
            }
            ui.add_space(4.0);
            }
        });
        if let Some(g) = toggle_group {
            if !self.settings.friend_groups_closed.remove(&g) {
                self.settings.friend_groups_closed.insert(g);
            }
        }
        let ctx = ui.ctx().clone();
        if let Some((id, name)) = open {
            self.open_chat(&ctx, id, name, net);
        }
        if let Some(a) = friend_act {
            self.friend_action(a, net, &ctx);
        }
    }

    fn open_chat(&mut self, ctx: &egui::Context, id: u64, name: String, net: &NetState) {
        if !net.chats.get(&id).is_some_and(|c| c.history_loaded) {
            let _ = self.tx.send(Cmd::OpenChat(id));
        }
        // The same friend - just keep the window; another one - switch the window to them.
        // The window is already open - a new conversation opens as a tab (as in Steam).
        let mut chat = self.chat.lock().unwrap();
        match chat.as_mut() {
            Some(c) => c.switch_to(id, name),
            None => *chat = Some(chatwin::ChatWin::new(id, name)),
        }
        drop(chat);
        // If the chat window is already open, bring it to the front (a command to a nonexistent window
        // would create an empty "egui window").
        if notify::chat_open() {
            ctx.send_viewport_cmd_to(chatwin::viewport_id(), egui::ViewportCommand::Focus);
        }
    }

    /// "Already downloaded...": pick the game folder in steamapps\common and add it to Steam.
    fn import_game(&mut self, appid: u32, name: String) {
        if self.steam_running() {
            *self.launch_status.lock().unwrap() = tr!("Закройте Steam, чтобы добавить игру (кнопка «Закрыть Steam» внизу).", "Close Steam to add the game (the \"Close Steam\" button at the bottom).").into();
            return;
        }
        let Some(dir) = rfd::FileDialog::new()
            .set_title(trf!("Папка с файлами «{name}» (внутри steamapps\\common)", "Folder with the files of \"{name}\" (inside steamapps\\common)"))
            .pick_folder()
        else {
            return;
        };
        // We expect <library>\steamapps\common\<game folder>.
        let library = dir
            .parent()
            .filter(|p| p.file_name().is_some_and(|n| n.eq_ignore_ascii_case("common")))
            .and_then(|p| p.parent())
            .filter(|p| p.file_name().is_some_and(|n| n.eq_ignore_ascii_case("steamapps")))
            .and_then(|p| p.parent())
            .map(|p| p.to_path_buf());
        match library {
            Some(library) => {
                let _ = self.tx.send(Cmd::Import { appid, name, library });
            }
            None => {
                *self.launch_status.lock().unwrap() = tr!("Папка игры должна лежать в библиотеке Steam: …\\steamapps\\common\\<папка игры>. \
                     Перенесите ее туда и попробуйте снова.", "The game folder must be in a Steam library: …\\steamapps\\common\\<game folder>. Move it there and try again.")
                    .into();
            }
        }
    }

    /// A section instead of the library: store, inventory or Workshop.
    fn section_view(&mut self, ui: &mut egui::Ui, net: &NetState) {
        // The section is open in a separate window - only a hint here.
        let detached = match self.view {
            View::Store => self.store_detached.then_some(tr!("Магазин", "Store")),
            View::Inventory => self.inv_detached.then_some(tr!("Инвентарь", "Inventory")),
            View::Workshop => self.ws_detached.then_some(tr!("Мастерская", "Workshop")),
            _ => None,
        };
        if let Some(what) = detached {
            ui.add_space(20.0);
            ui.vertical_centered(|ui| {
                ui.label(trf!("{what} открыт в отдельном окне.", "{what} is open in a separate window."));
                if ui.button(tr!("Показать здесь", "Show here")).clicked() {
                    match self.view {
                        View::Store => self.store_detached = false,
                        View::Inventory => self.inv_detached = false,
                        _ => self.ws_detached = false,
                    }
                }
            });
            return;
        }
        match self.view {
            View::Store => {
                if self.store_win.is_none() {
                    self.open_store(None, false);
                }
                self.store_contents(ui, net);
            }
            View::Inventory => {
                if self.inv_win.is_none() {
                    self.open_inventory(false);
                }
                self.inventory_contents(ui, net);
            }
            View::Workshop => self.workshop_contents(ui, net),
            View::Settings => self.settings_contents(ui, net),
            View::Screenshots => self.shots_contents(ui, net),
            View::Storage => self.storage_contents(ui),
            View::News => self.news_contents(ui, net),
            View::Badges => self.badges_contents(ui, net),
            View::Points => self.points_contents(ui, net),
            View::Family => self.family_contents(ui, net),
            View::Activity => self.activity_contents(ui, net),
            View::Stats => self.stats_contents(ui, net),
            View::Community => self.community_contents(ui, net),
            View::Groups => self.groups_contents(ui, net),
            View::Group => self.group_contents(ui, net),
            View::ProfileEdit => self.profile_edit_contents(ui, net),
            View::Achievements => self.achievements_contents(ui, net),
            View::Props => self.props_contents(ui, net),
            View::Cart => self.cart_contents(ui, net),
            View::Music => self.music_contents(ui),
            View::Profile => self.profile_contents(ui, net),
            View::Friends => self.friends_page_contents(ui, net),
            View::Downloads => self.downloads_contents(ui, net),
            View::Library => {}
        }
    }

    /// Library left column, as in Steam: "Home", search, collection, filters and the game list.
    /// Library search and filters: in the left column or (toolbar) as a row above the table.
    fn library_filters(&mut self, ui: &mut egui::Ui, net: &NetState, toolbar: bool) {
        // No count: the search covers all library games (own, family, non-Steam), and the count
        // of "own" ones (228) next to the "All games (345)" title was confusing.
        let hint = tr!("Поиск по названию", "Search by name");
        ui.add(egui::TextEdit::singleline(&mut self.filter).hint_text(hint).desired_width(if toolbar { 220.0 } else { f32::INFINITY }));
                    // Steam collections (right click on a game - add to a collection).
                    if let Some(colls) = net.collections.as_ref() {
                        let current = self
                            .collection
                            .as_ref()
                            .and_then(|id| colls.iter().find(|c| &c.id == id))
                            .map(|c| c.name.clone())
                            .or_else(|| (self.collection.as_deref() == Some(FAMILY)).then(|| tr!("Семейная библиотека", "Family library").to_string()))
                            .or_else(|| (self.collection.as_deref() == Some(UNPLAYED)).then(|| tr!("Ни разу не запускались", "Never launched").to_string()))
                            .or_else(|| (self.collection.as_deref() == Some(READY)).then(|| tr!("Готово к игре", "Ready to play").to_string()))
                            .or_else(|| (self.collection.as_deref() == Some(UPDATES)).then(|| tr!("Есть обновление", "Update available").to_string()))
                            .unwrap_or_else(|| tr!("Все игры", "All games").into());
                        let family_n = self.row_map(&net).values().filter(|r| r.family).count();
                        // Own collection (not built-in and not filter-based): rename, delete.
                        let own = self
                            .collection
                            .as_ref()
                            .and_then(|id| colls.iter().find(|c| &c.id == id && c.id.starts_with("uc-") && !c.dynamic))
                            .cloned();
                        let own_coll = own.is_some();
                        ui.horizontal(|ui| {
                        egui::ComboBox::from_id_salt("collection")
                            .width(if toolbar { 190.0 } else { ui.available_width() - if own_coll { 34.0 } else { 4.0 } })
                            .truncate()
                            .selected_text(current)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.collection, None, tr!("Все игры", "All games"));
                                if family_n > 0 {
                                    ui.selectable_value(
                                        &mut self.collection,
                                        Some(FAMILY.to_string()),
                                        trf!("Семейная библиотека ({family_n})", "Family library ({family_n})"),
                                    )
                                    .on_hover_text(tr!("Игры, которыми с вами делятся члены семьи Steam", "Games shared with you by Steam Family members"));
                                }
                                let unplayed_n = net.owned.iter().filter(|g| g.playtime_forever_minutes == 0).count();
                                if unplayed_n > 0 {
                                    ui.selectable_value(
                                        &mut self.collection,
                                        Some(UNPLAYED.to_string()),
                                        trf!("Ни разу не запускались ({unplayed_n})", "Never launched ({unplayed_n})"),
                                    )
                                    .on_hover_text(tr!("Купленные игры, в которых 0 минут", "Owned games with 0 minutes played"));
                                }
                                ui.selectable_value(&mut self.collection, Some(READY.to_string()), tr!("Готово к игре", "Ready to play"))
                                    .on_hover_text(tr!("Установленные игры, которым не нужно обновление", "Installed games that need no update"));
                                ui.selectable_value(&mut self.collection, Some(UPDATES.to_string()), tr!("Есть обновление", "Update available"))
                                    .on_hover_text(tr!("Установленные игры, для которых вышло обновление", "Installed games with an update available"));
                                ui.separator();
                                for c in colls {
                                    ui.selectable_value(
                                        &mut self.collection,
                                        Some(c.id.clone()),
                                        format!("{} ({})", c.name, c.apps.len()),
                                    );
                                }
                            })
                            .response
                            .on_hover_text(tr!("Коллекции из Steam. Добавить игру - правый щелчок по ней. «Скрытые» в «Все игры» не показываются.", "Collections from Steam. To add a game, right-click it. \"Hidden\" are not shown in \"All games\"."));
                        if let Some(c) = own {
                            ui.menu_button("...", |ui| {
                                let key = egui::Id::new("coll_rename");
                                let mut name: String =
                                    ui.data_mut(|d| d.get_temp(key)).unwrap_or_else(|| c.name.clone());
                                ui.horizontal(|ui| {
                                    ui.add(egui::TextEdit::singleline(&mut name).desired_width(150.0));
                                    let ok = !name.trim().is_empty() && name.trim() != c.name;
                                    if ui.add_enabled(ok, egui::Button::new(tr!("Переименовать", "Rename"))).clicked() {
                                        let _ = self.tx.send(Cmd::Coll(colls::CollCmd::Rename {
                                            id: c.id.clone(),
                                            name: name.trim().to_string(),
                                        }));
                                        ui.data_mut(|d| d.remove::<String>(key));
                                        ui.close_menu();
                                    }
                                });
                                ui.data_mut(|d| d.insert_temp(key, name));
                                ui.separator();
                                let confirm = egui::Id::new("coll_delete_confirm");
                                let asked = ui.data(|d| d.get_temp::<String>(confirm)) == Some(c.id.clone());
                                if !asked {
                                    if ui.button(tr!("Удалить коллекцию...", "Delete collection...")).clicked() {
                                        ui.data_mut(|d| d.insert_temp(confirm, c.id.clone()));
                                    }
                                } else {
                                    ui.label(trf!("Удалить «{}»? Игры останутся в библиотеке.", "Delete \"{}\"? The games stay in the library.", c.name));
                                    ui.horizontal(|ui| {
                                        if ui.button(tr!("Удалить", "Delete")).clicked() {
                                            let _ = self.tx.send(Cmd::Coll(colls::CollCmd::Delete { id: c.id.clone() }));
                                            self.collection = None;
                                            ui.data_mut(|d| d.remove::<String>(confirm));
                                            ui.close_menu();
                                        }
                                        if ui.button(tr!("Нет", "No")).clicked() {
                                            ui.data_mut(|d| d.remove::<String>(confirm));
                                        }
                                    });
                                }
                            })
                            .response
                            .on_hover_text(tr!("Переименовать или удалить эту коллекцию (изменится и в Steam)", "Rename or delete this collection (it changes in Steam too)"));
                        }
                        });
                    }
        if !toolbar {
        egui::ComboBox::from_id_salt("sort")
            .width(ui.available_width() - 4.0)
            .truncate()
            .selected_text(trf!("Сортировка: {}", "Sort: {}", self.settings.sort.label()))
            .show_ui(ui, |ui| {
                for s in settings::Sort::ALL {
                    ui.selectable_value(&mut self.settings.sort, s, s.label());
                }
            });
        }
        self.genre_combo(ui, net, toolbar);
        ui.checkbox(&mut self.only_installed, tr!("Только установленные", "Installed only"));
        if let Some(e) = &net.collections_error {
            ui.colored_label(egui::Color32::from_rgb(230, 110, 110), e);
        }
    }

    /// Genre choice. Tags come from the store: the first time the list is opened, details of
    /// all library games are requested (50 per request).
    fn genre_combo(&mut self, ui: &mut egui::Ui, net: &NetState, toolbar: bool) {
        let ids: Vec<u32> = self.row_map(net).keys().copied().filter(|id| *id < crate::custom::BASE).collect();
        let (genres, loaded) = {
            let st = net.store.lock().unwrap();
            let mut count: std::collections::HashMap<String, usize> = Default::default();
            let mut loaded = 0;
            for id in &ids {
                if let Some(a) = st.apps.get(id) {
                    loaded += 1;
                    for t in a.tags.iter().take(8) {
                        if let Some(n) = &t.name {
                            *count.entry(n.clone()).or_default() += 1;
                        }
                    }
                }
            }
            let mut v: Vec<(String, usize)> = count.into_iter().filter(|(_, n)| *n >= 2).collect();
            v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            v.truncate(40);
            (v, loaded)
        };
        let text = match &self.genre {
            Some(g) => trf!("Жанр: {g}", "Genre: {g}"),
            None => tr!("Жанр: любой", "Genre: any").to_string(),
        };
        let resp = egui::ComboBox::from_id_salt("genre")
            .width(if toolbar { 170.0 } else { ui.available_width() - 4.0 })
            .truncate()
            .selected_text(text)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut self.genre, None, tr!("Любой", "Any"));
                if loaded < ids.len() {
                    ui.horizontal(|ui| {
                        crate::spin::spinner(ui);
                        ui.weak(trf!("загружаю метки: {loaded} из {}", "loading tags: {loaded} of {}", ids.len()));
                    });
                }
                for (g, n) in &genres {
                    ui.selectable_value(&mut self.genre, Some(g.clone()), format!("{g} ({n})"));
                }
            });
        if (resp.response.clicked() || self.genre.is_some()) && !self.genre_asked {
            self.genre_asked = true;
            let _ = self.tx.send(Cmd::Store(store::StoreCmd::Details(ids)));
        }
        resp.response.on_hover_text(tr!("Метки игр из магазина Steam", "Game tags from the Steam store"));
    }

    fn library_sidebar(&mut self, ui: &mut egui::Ui, net: &NetState, rows: &[Row], vc: &libview::Ctx) -> Option<libview::Action> {
        let mut out = None;
        let home = self.lib_page.is_none();
        let text = egui::RichText::new(tr!("Главная", "Home")).size(14.0).strong().color(if home { egui::Color32::WHITE } else { theme::TEXT });
        let fill = if home { theme::CARD } else { egui::Color32::TRANSPARENT };
        if ui.add_sized([ui.available_width(), 28.0], egui::Button::new(text).fill(fill)).clicked() {
            self.lib_page = None;
        }
        ui.add_space(4.0);
        self.library_filters(ui, net, false);
        ui.add_space(2.0);
        // In the table view games are already shown as the table on the right - a second list is not needed.
        let table = self.settings.view == settings::View::List || !self.settings.covers;
        if table {
            return out;
        }
        ui.add_space(2.0);
        if let Some(a) = libview::sidebar(ui, rows, vc, &self.covers, self.lib_page, self.settings.covers) {
            out = Some(a);
        }
        out
    }

    /// In the background (window in the tray or a game running, and Vaporust not active) - unload images
    /// and give memory back to the system: the game needs it more. Repeated once a minute while in the background.
    fn memory_tick(&mut self, ctx: &egui::Context) {
        let game = local::RUNNING.lock().unwrap().is_some();
        let background = notify::in_tray() || (game && !ctx.input(|i| i.focused));
        if !(self.settings.trim_memory && background) {
            self.trimmed = false;
            return;
        }
        if !self.trimmed {
            self.trimmed = true;
            ctx.forget_all_images();
            self.covers.clear();
            imgload::trim_working_set();
            self.trim_at = std::time::Instant::now();
        } else if self.trim_at.elapsed() >= std::time::Duration::from_secs(60) {
            imgload::trim_working_set();
            self.trim_at = std::time::Instant::now();
        }
    }

    fn account_panel(&mut self, ui: &mut egui::Ui, net: &NetState) {
        if net.account.is_some() {
            self.top_nav(ui, net);
        } else if self.settings.local_mode {
            // No-login mode: only installed games and launching via Steam.
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new(tr!("Режим без входа в аккаунт", "No-login mode")).strong());
                ui.weak(tr!("Установленные игры из файлов Steam, запуск через Steam. Пароль Steam в Vaporust не вводится.", "Installed games from Steam files, launched through Steam. The Steam password is not entered in Vaporust."));
                // There is no top menu without login: library and settings are here.
                if ui.selectable_label(self.view == View::Library, tr!("Библиотека", "Library")).clicked() {
                    self.view = View::Library;
                }
                if ui.selectable_label(self.view == View::Settings, tr!("Настройки", "Settings")).clicked() {
                    self.view = View::Settings;
                }
                if ui
                    .button(tr!("Войти в аккаунт", "Log in"))
                    .on_hover_text(tr!("Включит магазин, друзей и чат, загрузку и обновление игр без Steam", "Turns on the store, friends and chat, game downloads and updates without Steam"))
                    .clicked()
                {
                    self.settings.local_mode = false;
                    let _ = self.tx.send(Cmd::Restore);
                }
            });
        } else if net.offline {
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(egui::Color32::from_rgb(230, 170, 60), tr!("Нет связи со Steam.", "No connection to Steam."))
                    .on_hover_text(&net.offline_reason);
                ui.label(tr!("Автономный режим: игры из сохраненного списка, запуск через Steam без сети.", "Offline mode: games from the saved list, launched through Steam without network."));
                if ui.add_enabled(!net.busy, egui::Button::new(tr!("Переподключиться", "Reconnect"))).clicked() {
                    self.offline_retry = Some(std::time::Instant::now());
                    let _ = self.tx.send(Cmd::Restore);
                }
                if ui
                    .add_enabled(!net.busy, egui::Button::new(tr!("Войти заново", "Log in again")))
                    .on_hover_text(tr!("Если дело не в сети: войти в аккаунт логином и паролем или QR-кодом", "If it is not the network: log in with login and password or a QR code"))
                    .clicked()
                {
                    let mut s = self.net.lock().unwrap();
                    s.offline = false;
                    s.need_login = true;
                }
                if net.busy {
                    crate::spin::spinner(ui);
                }
            });
        } else if net.need_login {
            ui.label(tr!("Вход в Steam (нужен для списка всех игр, времени в игре и достижений)", "Steam login (needed for the list of all games, playtime and achievements)"));
            egui::Grid::new("login").num_columns(2).show(ui, |ui| {
                ui.label(tr!("Логин:", "Login:"));
                ui.text_edit_singleline(&mut self.login_user);
                ui.end_row();
                ui.label(tr!("Пароль:", "Password:"));
                ui.add(egui::TextEdit::singleline(&mut self.login_pass).password(true));
                ui.end_row();
                ui.label(tr!("Код Steam Guard:", "Steam Guard code:"));
                ui.add(
                    egui::TextEdit::singleline(&mut self.login_code)
                        .hint_text(tr!("если пришел на почту или в приложение", "if it came by email or in the app")),
                );
                ui.end_row();
            });
            ui.horizontal(|ui| {
                // While waiting for the QR code, the user can change their mind and log in with a password (the QR wait is interrupted).
                let waiting_qr = net.qr_url.is_some();
                let can = (!net.busy || waiting_qr) && !self.login_user.is_empty() && !self.login_pass.is_empty();
                if ui.add_enabled(can, egui::Button::new(tr!("Войти", "Log in"))).clicked() {
                    let code = self.login_code.trim().to_string();
                    let _ = self.tx.send(Cmd::Login {
                        user: self.login_user.trim().to_string(),
                        pass: std::mem::take(&mut self.login_pass),
                        code: (!code.is_empty()).then_some(code),
                    });
                    self.login_code.clear();
                }
                if ui
                    .add_enabled(!net.busy, egui::Button::new(tr!("Войти по QR-коду", "Log in with a QR code")))
                    .clicked()
                {
                    let _ = self.tx.send(Cmd::LoginQr);
                }
                if ui
                    .button(tr!("Без входа", "Without login"))
                    .on_hover_text(tr!("Только установленные игры и запуск через Steam: пароль Steam вводить не нужно. Войти можно позже.", "Installed games only, launched through Steam: no Steam password needed. You can log in later."))
                    .clicked()
                {
                    self.settings.local_mode = true;
                    let mut s = self.net.lock().unwrap();
                    s.need_login = false;
                    s.qr_url = None;
                    s.message.clear();
                }
            });
            if let Some(url) = &net.qr_url {
                if net.qr_scanned {
                    ui.label(tr!("Код отсканирован, подтвердите вход в приложении Steam.", "The code is scanned, confirm the login in the Steam app."));
                } else {
                    ui.label(tr!("Отсканируйте код в мобильном приложении Steam:", "Scan the code in the Steam mobile app:"));
                }
                draw_qr(ui, url);
                if ui.button(tr!("Отменить вход по QR", "Cancel QR login")).clicked() {
                    let _ = self.tx.send(Cmd::CancelQr);
                }
            }
        }
        if !net.message.is_empty() {
            ui.label(&net.message);
        }
    }

    /// Game achievements - a page of the main window ("Library" section).
    fn achievements_contents(&mut self, ui: &mut egui::Ui, net: &NetState) {
        let Some((appid, name)) = self.ach_open.clone() else {
            return;
        };
        let mut open = true;
        let mut ask_friend: Option<u64> = None;
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui.button(tr!("< К игре", "< Back to game")).clicked() {
                open = false;
            }
            ui.label(egui::RichText::new(trf!("Достижения: {name}", "Achievements: {name}")).size(19.0).strong());
        });
        ui.separator();
        let st = &mut self.ach_state;
        egui::Frame::none()
            .show(ui, |ui| match net.achievements.get(&appid) {
                None => {
                    crate::spin::spinner(ui);
                }
                Some(Err(e)) => {
                    ui.label(trf!("Ошибка: {e}", "Error: {e}"));
                }
                Some(Ok(list)) if list.is_empty() => {
                    ui.label(tr!("У этой игры нет достижений.", "This game has no achievements."));
                }
                Some(Ok(list)) => {
                    let done = list.iter().filter(|a| a.unlocked).count();
                    ui.horizontal(|ui| {
                        ui.label(trf!("Получено {done} из {}", "Unlocked {done} of {}", list.len()));
                        ui.add(
                            egui::ProgressBar::new(done as f32 / list.len().max(1) as f32)
                                .desired_width(160.0)
                                .text(format!("{}%", done * 100 / list.len().max(1))),
                        );
                    });
                    // Order, filter, comparison with a friend.
                    ui.horizontal_wrapped(|ui| {
                        egui::ComboBox::from_id_salt("ach_sort")
                            .selected_text(ACH_SORTS()[st.sort as usize])
                            .show_ui(ui, |ui| {
                                for (i, s) in ACH_SORTS().iter().enumerate() {
                                    ui.selectable_value(&mut st.sort, i as u8, *s);
                                }
                            });
                        egui::ComboBox::from_id_salt("ach_filter")
                            .selected_text(ACH_FILTERS()[st.filter as usize])
                            .show_ui(ui, |ui| {
                                for (i, s) in ACH_FILTERS().iter().enumerate() {
                                    ui.selectable_value(&mut st.filter, i as u8, *s);
                                }
                            });
                        let roster = net.roster.read().unwrap();
                        let mut friends: Vec<(u64, String)> = roster
                            .values()
                            .filter(|f| (f.relationship == 3 && (f.steam_id >> 52) & 0xF == 1))
                            .map(|f| (f.steam_id, f.persona_name.clone().unwrap_or_else(|| f.steam_id.to_string())))
                            .collect();
                        drop(roster);
                        friends.sort_by_key(|f| f.1.to_lowercase());
                        let current = st
                            .friend
                            .and_then(|id| friends.iter().find(|f| f.0 == id))
                            .map(|f| trf!("Сравнить с: {}", "Compare with: {}", f.1))
                            .unwrap_or_else(|| tr!("Сравнить с другом", "Compare with a friend").into());
                        egui::ComboBox::from_id_salt("ach_friend").selected_text(current).height(320.0).show_ui(ui, |ui| {
                            if ui.selectable_label(st.friend.is_none(), tr!("Без сравнения", "No comparison")).clicked() {
                                st.friend = None;
                            }
                            for (id, n) in &friends {
                                if ui.selectable_label(st.friend == Some(*id), n).clicked() {
                                    st.friend = Some(*id);
                                    ask_friend = Some(*id);
                                }
                            }
                        });
                    });
                    // Friend achievements: ✔/✖ by API name.
                    let friend_map: Option<HashMap<&str, bool>> = match st.friend.map(|f| net.friend_achievements.get(&(appid, f))) {
                        Some(Some(Ok(v))) => {
                            let got = v.iter().filter(|a| a.unlocked).count();
                            if got == 0 {
                                ui.weak(tr!("У друга нет полученных достижений (не играл или скрыл данные об играх).", "The friend has no unlocked achievements (did not play or hides game details)."));
                            } else {
                                ui.label(trf!("У друга {got} из {}", "The friend has {got} of {}", v.len()));
                            }
                            Some(v.iter().map(|a| (a.api_name.as_str(), a.unlocked)).collect())
                        }
                        Some(Some(Err(e))) => {
                            ui.weak(trf!("Достижения друга не получены: {e}", "Could not get the friend's achievements: {e}"));
                            None
                        }
                        Some(None) => {
                            crate::spin::spinner(ui);
                            None
                        }
                        None => None,
                    };
                    ui.separator();
                    let mut rows: Vec<&aurelia::steam_client::GameAchievement> = list
                        .iter()
                        .filter(|a| match st.filter {
                            1 => a.unlocked,
                            2 => !a.unlocked,
                            _ => true,
                        })
                        .collect();
                    match st.sort {
                        1 => rows.sort_by(|a, b| a.global_percent.total_cmp(&b.global_percent)),
                        2 => rows.sort_by(|a, b| b.global_percent.total_cmp(&a.global_percent)),
                        3 => rows.sort_by_key(|a| std::cmp::Reverse(a.unlock_time.unwrap_or(0))),
                        _ => {}
                    }
                    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                        for a in rows {
                            ui.horizontal(|ui| {
                                let icon = if a.unlocked { &a.icon_unlocked } else { &a.icon_locked };
                                if !icon.is_empty() {
                                    ui.add(egui::Image::new(icon.as_str()).fit_to_exact_size(egui::vec2(40.0, 40.0)));
                                } else {
                                    ui.label(if a.unlocked { "✔" } else { "✖" });
                                }
                                ui.vertical(|ui| {
                                    ui.horizontal(|ui| {
                                        if a.unlocked {
                                            ui.strong(&a.name);
                                        } else {
                                            ui.label(&a.name);
                                        }
                                        if a.global_percent > 0.0 && a.global_percent < 10.0 {
                                            ui.colored_label(egui::Color32::from_rgb(230, 170, 60), tr!("редкое", "rare"));
                                        }
                                    });
                                    if a.hidden && !a.unlocked && a.description.is_empty() {
                                        ui.weak(tr!("Скрытое достижение", "Hidden achievement"));
                                    } else if !a.description.is_empty() {
                                        ui.weak(&a.description);
                                    }
                                    let mut line = trf!("Есть у {:.1}% игроков", "{:.1}% of players have it", a.global_percent);
                                    if let Some(t) = a.unlock_time.filter(|_| a.unlocked) {
                                        line.push_str(&trf!(" · получено {}", " · unlocked {}", crate::socialwin::fmt_date(t as u64)));
                                    }
                                    if let Some(m) = &friend_map {
                                        match m.get(a.api_name.as_str()) {
                                            Some(true) => line.push_str(tr!(" · у друга есть", " · the friend has it")),
                                            Some(false) => line.push_str(tr!(" · у друга нет", " · the friend does not have it")),
                                            None => {}
                                        }
                                    }
                                    ui.weak(line);
                                });
                            });
                            ui.add_space(4.0);
                        }
                    });
                }
            });
        if let Some(f) = ask_friend {
            if !net.friend_achievements.contains_key(&(appid, f)) {
                let _ = self.tx.send(Cmd::FriendAchievements(appid, f));
            }
        }
        if !open {
            self.ach_open = None;
            self.ach_state = AchState::default();
            self.view = View::Library;
            self.lib_page = Some(appid);
        }
    }
}

fn draw_qr(ui: &mut egui::Ui, url: &str) {
    let Ok(code) = qrcode::QrCode::new(url.as_bytes()) else {
        ui.label(url);
        return;
    };
    let w = code.width();
    let colors = code.to_colors();
    let cell = 4.0;
    let quiet = 4;
    let size = (w + 2 * quiet) as f32 * cell;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, egui::Color32::WHITE);
    for y in 0..w {
        for x in 0..w {
            if colors[y * w + x] == qrcode::Color::Dark {
                let min = rect.min + egui::vec2((x + quiet) as f32 * cell, (y + quiet) as f32 * cell);
                painter.rect_filled(
                    egui::Rect::from_min_size(min, egui::vec2(cell, cell)),
                    0.0,
                    egui::Color32::BLACK,
                );
            }
        }
    }
}

fn fmt_hours(min: u32) -> String {
    if min < 60 {
        trf!("{min} мин", "{min} min")
    } else {
        trf!("{:.1} ч", "{:.1} h", min as f32 / 60.0)
    }
}

/// `:name:` of own emoticons -> ːnameː, the way they are typed in Steam.
fn emoticons_to_bbcode(text: &str, net: &NetState) -> String {
    let Some((emoticons, _)) = &net.emoticons else {
        return text.to_string();
    };
    let mut out = text.to_string();
    for name in emoticons {
        // Steam expects an emoticon as ːnameː (U+02D0); the server strips the [emoticon] tag.
        out = out.replace(&format!(":{name}:"), &format!("ː{name}ː"));
    }
    out
}

/// Whether the V key is physically pressed right now.
#[cfg(windows)]
fn key_v_down() -> bool {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
    unsafe { (GetAsyncKeyState(0x56) as u16 & 0x8000) != 0 } // 0x56 = VK_V
}

#[cfg(not(windows))]
fn key_v_down() -> bool {
    false
}

/// Clipboard image to a temporary PNG file. Works both for an image
/// (screenshot, "Copy image" in a browser) and for an image file copied in Explorer.
/// Ok(None) - no image in the clipboard.
fn clipboard_image_file() -> Result<Option<std::path::PathBuf>, String> {
    let mut cb = arboard::Clipboard::new().map_err(|e| trf!("Буфер обмена недоступен: {e}", "Clipboard unavailable: {e}"))?;
    if let Ok(img) = cb.get_image() {
        let rgba = image::RgbaImage::from_raw(img.width as u32, img.height as u32, img.bytes.into_owned())
            .ok_or(tr!("Не удалось прочитать картинку из буфера обмена.", "Could not read the image from the clipboard."))?;
        let path = std::env::temp_dir().join(format!(
            "vaporust_paste_{}.png",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0)
        ));
        rgba.save_with_format(&path, image::ImageFormat::Png)
            .map_err(|e| trf!("Не удалось сохранить картинку из буфера: {e}", "Could not save the image from the clipboard: {e}"))?;
        return Ok(Some(path));
    }
    if let Ok(files) = cb.get().file_list() {
        let is_img = |p: &std::path::Path| {
            p.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| ["png", "jpg", "jpeg", "gif", "webp", "bmp"].contains(&e.to_lowercase().as_str()))
        };
        return Ok(files.into_iter().find(|p| is_img(p)));
    }
    Ok(None)
}

/// One piece of a chat message: text, link, image, emoticon or sticker.
/// Images load from Steam servers on first display (egui_extras), a spinner turns while loading.
fn draw_seg(ui: &mut egui::Ui, seg: chatfmt::Seg, anims: &mut anim::Cache, who: &str) {
    use chatfmt::Seg;
    match seg {
        Seg::Spoiler(t) => {
            // As in Steam: hidden by a dark bar, a click reveals it (until the window repaints).
            let id = ui.id().with(("spoiler", &t));
            let shown = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
            if shown {
                egui::Frame::none().fill(egui::Color32::from_rgb(40, 46, 58)).inner_margin(egui::Margin::symmetric(4.0, 1.0)).rounding(2.0).show(ui, |ui| {
                    ui.label(t);
                });
            } else {
                let r = ui
                    .add(egui::Button::new(egui::RichText::new(tr!("спойлер - нажмите, чтобы показать", "spoiler - click to show")).size(12.0).color(egui::Color32::from_gray(150))).fill(egui::Color32::from_rgb(20, 22, 28)))
                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                if r.clicked() {
                    ui.data_mut(|d| d.insert_temp(id, true));
                }
            }
        }
        Seg::Code(t) => {
            egui::Frame::none().fill(egui::Color32::from_rgb(18, 21, 27)).inner_margin(egui::Margin::same(6.0)).rounding(3.0).show(ui, |ui| {
                ui.label(egui::RichText::new(t).monospace().size(12.5));
            });
        }
        Seg::Quote { author, text } => {
            // Quote: a stripe on the left, who said it and the text.
            let resp = egui::Frame::none().inner_margin(egui::Margin { left: 10.0, right: 4.0, top: 2.0, bottom: 2.0 }).show(ui, |ui| {
                ui.vertical(|ui| {
                    if let Some(a) = author {
                        ui.label(egui::RichText::new(trf!("{a} пишет:", "{a} wrote:")).size(12.0).color(egui::Color32::from_gray(150)));
                    }
                    ui.label(egui::RichText::new(text).italics());
                });
            });
            let r = resp.response.rect;
            ui.painter().rect_filled(egui::Rect::from_min_size(r.min, egui::vec2(3.0, r.height())), 1.0, egui::Color32::from_rgb(90, 110, 140));
        }
        Seg::Me(t) => {
            ui.label(egui::RichText::new(format!("* {who} {t}")).italics().color(egui::Color32::from_rgb(200, 170, 230)));
        }
        Seg::Text(t) => {
            ui.label(t);
        }
        Seg::Link { url, label } => {
            ui.hyperlink_to(label, &url).on_hover_text(&url);
        }
        Seg::Emoticon(name) => {
            ui.add(egui::Image::new(chatfmt::emoticon_url(&name)).fit_to_exact_size(egui::vec2(30.0, 30.0)))
                .on_hover_text(format!(":{name}:"));
        }
        Seg::RoomEffect(kind) => {
            // As in Steam: effect icon, "... launches balloons!" and a replay button.
            ui.add(egui::Image::new(effects::icon_url(&kind)).fit_to_exact_size(egui::vec2(28.0, 28.0)));
            ui.label(egui::RichText::new(effects::label(&kind, who)).italics());
            let known = effects::KNOWN.iter().any(|(n, _)| n.eq_ignore_ascii_case(&kind));
            if known && ui.small_button(tr!("Повторить эффект", "Replay effect")).clicked() {
                effects::request_replay(&kind);
            }
        }
        Seg::Sticker(name) => {
            // Exact size (with max_size egui shrinks it to the line height); APNG is animated by us.
            anims
                .show(ui, &chatfmt::sticker_url(&name), egui::vec2(128.0, 128.0))
                .on_hover_text(trf!("Стикер: {name}", "Sticker: {name}"));
        }
        Seg::Image { url, size } => {
            // Size known - reserve the space right away so the conversation does not jump while loading.
            let max = egui::vec2(360.0, 280.0);
            // Show Steam's reduced copy (up to 512 points), not the original: less memory and GPU work.
            let img = egui::Image::new(imgload::still_if_needed(&chatfmt::thumbnail_url(&url))).sense(egui::Sense::click());
            let img = match size {
                Some((w, h)) => {
                    let k = (max.x / w).min(max.y / h).min(1.0);
                    img.fit_to_exact_size(egui::vec2(w * k, h * k))
                }
                // Size unknown: a square by the smaller side, the image fits into it.
                None => img.fit_to_exact_size(egui::vec2(max.y, max.y)),
            };
            let resp = ui
                .add(img)
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(tr!("Открыть в браузере в полном размере", "Open full size in the browser"));
            if resp.clicked() {
                ui.ctx().open_url(egui::OpenUrl::new_tab(&url));
            }
        }
    }
}

/// Last launch date: "сегодня", "вчера" or "03.11.2026".
fn fmt_day(unix: u64) -> String {
    use chrono::{Local, TimeZone};
    let Some(t) = Local.timestamp_opt(unix as i64, 0).single() else {
        return String::new();
    };
    let days = (Local::now().date_naive() - t.date_naive()).num_days();
    match days {
        0 => tr!("сегодня", "today").into(),
        1 => tr!("вчера", "yesterday").into(),
        _ => t.format("%d.%m.%Y").to_string(),
    }
}

/// Message time: "14:05" today, "03.11 14:05" earlier.
fn fmt_time(unix: u64) -> String {
    use chrono::{Local, TimeZone};
    match Local.timestamp_opt(unix as i64, 0).single() {
        Some(t) if t.date_naive() == Local::now().date_naive() => t.format("%H:%M").to_string(),
        Some(t) => t.format("%d.%m %H:%M").to_string(),
        None => String::new(),
    }
}

fn fmt_eta(secs: u64) -> String {
    if secs >= 3600 {
        trf!("{} ч {} мин", "{} h {} min", secs / 3600, secs % 3600 / 60)
    } else if secs >= 60 {
        trf!("{} мин", "{} min", secs / 60)
    } else {
        trf!("{secs} с", "{secs} s")
    }
}

impl eframe::App for LauncherApp {
    /// Background under the panels: fully transparent (the notification window shows rounded corners;
    /// in normal windows the background is covered by panels anyway, and window transparency is off).
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    /// Scroll positions and expanded blocks are not remembered between runs: the library, as in Steam,
    /// opens at the top. Everything needed (view, scale, window) is kept in our own settings.
    fn persist_egui_memory(&self) -> bool {
        false
    }

    /// eframe calls this on exit and periodically (every 30 s).
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.settings.window = if self.settings.remember_window {
            winpos::read(self.settings.window).or(self.settings.window)
        } else {
            None
        };
        self.settings.freeze_webhelper = self.freeze_webhelper;
        self.settings.only_installed = self.only_installed;
        self.settings.show_friends = self.show_friends;
        self.settings.library = self
            .libraries
            .get(self.lib_idx)
            .map(|l| l.to_string_lossy().into_owned());
        eframe::set_value(storage, settings::KEY, &self.settings);
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let frame_start = std::time::Instant::now();
        // Memory: when switching sections, images of the previous one are not needed - clear the cache;
        // and keep an eye on the image cache limit.
        let place = (self.view, self.lib_page);
        if self.mem_place != Some(place) {
            if self.mem_place.is_some() {
                imgload::forget_large(ctx);
            }
            self.mem_place = Some(place);
        }
        crate::exp::set(self.settings.experimental);
        crate::lite::set(self.settings.light_mode);
        crate::lang::set_en(crate::lang::resolve(&self.settings.language));
        imgload::ANIMATIONS.store(self.settings.animations, Ordering::Relaxed);
        imgload::ECONOMY.store(self.settings.economy, Ordering::Relaxed);
        imgload::budget_tick(ctx);
        self.memory_tick(ctx);
        // Network state snapshot per frame so the lock is not held. Copied again only
        // when the state changed (and every 2 s just in case), otherwise the previous copy is used.
        let gen = self.net.gen();
        if gen != self.net_gen || self.net_snap_at.elapsed() >= std::time::Duration::from_secs(2) {
            self.net_snap = self.net.lock().unwrap().clone();
            self.net_gen = gen;
            self.net_snap_at = std::time::Instant::now();
        }
        let mut net = std::mem::take(&mut self.net_snap);
        let net_gen = std::mem::replace(&mut self.net_gen, u64::MAX);
        self.tour_tick(ctx, &net);

        // Unread messages: a count in the window title; when everything is read - remove the tray dot.
        let unread: u32 = net.chats.values().map(|c| c.unread).sum();
        if unread != self.last_unread {
            self.last_unread = unread;
            let base = concat!("Vaporust ", env!("CARGO_PKG_VERSION"));
            let title = if unread > 0 { format!("({unread}) {base}") } else { base.to_string() };
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
            if unread == 0 {
                tray::set_unread(false);
            }
        }
        // Window size and position from the previous run (before going to the tray on autostart).
        if !self.window_placed && notify::main_hwnd() != 0 {
            self.window_placed = true;
            if let Some(p) = self.settings.window.filter(|_| self.settings.remember_window) {
                if winpos::apply(p) {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true));
                }
            }
        }
        // Start with Windows (--tray): the window goes to the tray as soon as it has an HWND.
        if self.start_hidden && notify::main_hwnd() != 0 {
            self.start_hidden = false;
            tray::hide_main();
        }
        // The "Close button minimizes to tray" checkbox lives in the tray menu; we save it in settings.
        self.settings.close_to_tray = tray::close_to_tray();
        // The close button hides the window to the tray if configured (except "Exit" from the tray menu).
        if ctx.input(|i| i.viewport().close_requested())
            && tray::close_to_tray()
            && !tray::exit_requested()
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            tray::hide_main();
        }

        self.auto_rescan(ctx);
        // The install has finished (successfully or not) - rescan the disk.
        if net.disk_changed != self.seen_disk_changed {
            self.seen_disk_changed = net.disk_changed;
            self.rescan();
        }
        // Steam finished a game or an update - rescan the disk and recheck updates.
        if self.rescan_needed.swap(false, Ordering::Relaxed) {
            self.rescan();
            self.updates_checked = false;
        }
        // Steam collections - once after login.
        if net.account.is_none() {
            self.colls_requested = false;
        } else if !self.colls_requested {
            self.colls_requested = true;
            let _ = self.tx.send(Cmd::Coll(colls::CollCmd::Load));
        }
        // Saved friends status - after every login.
        if net.account.is_none() {
            self.persona_sent = false;
        } else if !self.persona_sent {
            self.persona_sent = true;
            let _ = self.tx.send(Cmd::SetPersona(self.settings.persona));
            self.send_aurelia_config();
            // Own room effects: right away from the saved list, a fresh one from the inventory in the background.
            if net.chat_effects.is_none() {
                self.net.lock().unwrap().chat_effects = Some(self.settings.chat_effects.clone());
            }
            let _ = self.tx.send(Cmd::LoadChatEffects);
        }
        if let Some(e) = net.chat_effects.as_ref().filter(|e| **e != self.settings.chat_effects) {
            self.settings.chat_effects = e.clone();
        }
        // Update check once after login (and after Steam work).
        if net.account.is_none() {
            self.updates_checked = false;
        } else if !self.updates_checked && !self.games.is_empty() {
            self.updates_checked = true;
            let list = self.games.iter().filter(|g| g.ready).map(|g| (g.appid, g.dir.clone())).collect();
            let _ = self.tx.send(Cmd::CheckUpdates(list));
        }
        // Pinned versions: launch after preparation, rollback, outdated masking.
        self.pin_tick(ctx, &mut net);
        // Launch without Steam: the launch table has arrived.
        self.direct_tick();
        let installing = net.install.as_ref().is_some_and(|i| i.active());
        // Repaint: during a download or game launch - once a second (progress bar, status),
        // when idle - every 5 s (the "Close Steam" button if Steam was started manually).
        // While a game runs and the Vaporust window is not active - less often (nobody looks at the status).
        let game_bg = local::RUNNING.lock().unwrap().is_some() && !ctx.input(|i| i.focused);
        let busy = installing || (!game_bg && (self.launching.load(Ordering::Relaxed) || self.closing.load(Ordering::Relaxed)));
        ctx.request_repaint_after(if busy { crate::lite::secs(1, 2) } else { crate::lite::secs(5, 15) });

        // Window frame: own or the normal Windows one (switches right away).
        winframe::set_system(self.settings.system_frame);
        if self.frame_applied != Some(self.settings.system_frame) {
            self.frame_applied = Some(self.settings.system_frame);
            ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(self.settings.system_frame));
        }
        winframe::heal_client_size(ctx);
        winframe::diag_once(ctx);
        // Windows insists on drawing its title bar over our frame - switch to the normal Windows frame.
        if let Some(m) = winframe::caption_forced() {
            self.settings.system_frame = true;
            errlog::write(tr!("окно", "window"), &trf!("Windows рисует свой заголовок ({m}): включена обычная рамка Windows", "Windows draws its own title bar ({m}): normal Windows frame turned on"));
            ctx.request_repaint();
        }
        // Frame check - during the first 10 s after start (then repaint as usual, without extra frames).
        if !self.settings.system_frame && ctx.input(|i| i.time) < 10.0 {
            ctx.request_repaint_after(std::time::Duration::from_millis(500));
        }
        if !notify::in_tray() {
            let title = if unread > 0 {
                format!("({unread}) Vaporust {}", env!("CARGO_PKG_VERSION"))
            } else {
                concat!("Vaporust ", env!("CARGO_PKG_VERSION")).to_string()
            };
            let _ = &title; // the window name is on the taskbar (ViewportCommand::Title above)
            // As in Steam: there is no separate title bar - window buttons are on the right of the section row,
            // the window is dragged by the empty space of that row.
            winframe::frame_only(ctx);
            let own_frame = !winframe::system();
            let top_frame = egui::Frame::none().fill(theme::TOP).inner_margin(if own_frame {
                egui::Margin { left: 10.0, right: 0.0, top: 0.0, bottom: 2.0 }
            } else {
                egui::Margin::symmetric(10.0, 6.0)
            });
            let drag_id = egui::Id::new("vaporust_top_drag");
            let top = egui::TopBottomPanel::top("account").frame(top_frame).show_separator_line(false).show(ctx, |ui| {
                winframe::drag_behind(ui, drag_id);
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // The bell right next to the window buttons, as one group.
                        ui.spacing_mut().item_spacing.x = 0.0;
                        winframe::window_buttons(ui, 34.0);
                        // The bell is in the same cell and on the same line as the window buttons.
                        self.steam_notifs_button(ui, &net);
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            self.account_panel(ui, &net);
                        });
                    });
                });
            });
            winframe::remember_drag(ctx, drag_id, top.response.rect);
            if net.account.is_some() {
                let sub_frame = egui::Frame::none().fill(theme::TOP).inner_margin(egui::Margin { left: 10.0, right: 10.0, top: 0.0, bottom: 5.0 });
                egui::TopBottomPanel::top("subnav").frame(sub_frame).show(ctx, |ui| self.sub_nav(ui, &net));
            }
            self.update_banner(ctx);

            let bottom_frame = egui::Frame::none().fill(theme::TOP).inner_margin(egui::Margin::symmetric(8.0, 3.0));
            egui::TopBottomPanel::bottom("status").frame(bottom_frame).show(ctx, |ui| {
                ui.add_space(2.0);
                let full = ui.max_rect();
                let row = ui.horizontal(|ui| {
                    // On the left, as in Steam - "Add a game".
                    ui.menu_button(egui::RichText::new(tr!("+ Добавить игру", "+ Add a game")).size(13.0), |ui| {
                        ui.set_min_width(220.0);
                        if ui.button(tr!("Добавить стороннюю игру...", "Add a non-Steam game...")).on_hover_text(tr!("Игра или программа не из Steam", "A game or program not from Steam")).clicked() {
                            self.custom_edit = Some(custom::Edit { to_steam: self.settings.custom_to_steam, ..Default::default() });
                            ui.close_menu();
                        }
                        if ui
                            .button(tr!("Взять сторонние игры из Steam", "Import non-Steam games from Steam"))
                            .on_hover_text(tr!("Игры, добавленные в настоящий Steam как сторонние, появятся и в Vaporust", "Games added to the real Steam as non-Steam games also appear in Vaporust"))
                            .clicked()
                        {
                            self.import_steam_shortcuts();
                            ui.close_menu();
                        }
                        if ui.button(tr!("Активировать в Steam...", "Activate on Steam...")).on_hover_text(tr!("Ключ продукта Steam: игра, дополнение, набор", "Steam product key: a game, DLC, bundle")).clicked() {
                            self.key_input = Some(String::new());
                            self.key_refreshed = false;
                            self.net.lock().unwrap().key_result = None;
                            ui.close_menu();
                        }
                    });
                    // On the right, as in Steam - "Friends and chat".
                    if net.account.is_some() {
                        let unread: u32 = net.chats.values().map(|c| c.unread).sum();
                        let label = if unread > 0 { trf!("Друзья и чат ({unread})", "Friends and chat ({unread})") } else { tr!("Друзья и чат", "Friends and chat").to_string() };
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.toggle_value(&mut self.show_friends, label);
                        });
                    }
                }).response.rect;
                // In the middle, as in Steam - "Downloads".
                let center = egui::Rect::from_center_size(egui::pos2(full.center().x, row.center().y), egui::vec2(280.0, row.height()));
                ui.allocate_new_ui(egui::UiBuilder::new().max_rect(center).layout(egui::Layout::centered_and_justified(egui::Direction::LeftToRight)), |ui| {
                    self.downloads_button(ui, &net);
                });
                // Freezing steamwebhelper and other checkboxes are in "Settings" ("Account" menu).
                ui.add_space(2.0);
            });

            if net.account.is_some() && self.show_friends {
                egui::SidePanel::right("friends")
                    .resizable(true)
                    .default_width(230.0)
                    .width_range(200.0..=380.0)
                    .show(ctx, |ui| self.friends_panel(ui, &net));
            }

            let ctrl = ctx.input(|i| i.modifiers.ctrl);
            // Library as in Steam: the game list with search and filters on the left, "Home" on the right
            // (grid or list) or the selected game's page.
            let launching = self.launching.load(Ordering::Relaxed) || self.closing.load(Ordering::Relaxed);
            let rows = if self.view == View::Library { self.rows(&net) } else { Vec::new() };
            let queued: std::collections::HashSet<u32> =
                self.settings.downloads.iter().filter(|q| !q.paused).map(|q| q.appid).collect();
            let paused: std::collections::HashSet<u32> =
                self.settings.downloads.iter().filter(|q| q.paused).map(|q| q.appid).collect();
            let status_text = self.launch_status.lock().unwrap().clone();
            let vc = libview::Ctx {
                launching,
                installing,
                logged_in: net.account.is_some(),
                net: &net,
                queued: &queued,
                paused: &paused,
                collections: net.collections.as_deref().unwrap_or(&[]),
                update_via_steam: self.settings.update_via_steam,
                running: local::RUNNING.lock().unwrap().as_ref().map(|r| r.0),
                status_app: self.status_app,
                status: &status_text,
            };
            let mut side_action = None;
            // Compact view (table): no left column, filters as a row above the table.
            let table_mode = self.settings.view == settings::View::List || !self.settings.covers;
            if self.view == View::Library && !table_mode {
                let side = egui::Frame::none().fill(theme::SIDEBAR).inner_margin(egui::Margin::symmetric(6.0, 6.0));
                egui::SidePanel::left("games")
                    .frame(side)
                    .resizable(true)
                    .default_width(250.0)
                    .width_range(180.0..=420.0)
                    .show(ctx, |ui| side_action = self.library_sidebar(ui, &net, &rows, &vc));
            }
            egui::CentralPanel::default().show(ctx, |ui| {
                // Store, inventory, Workshop - instead of the library, in the same window.
                // Each page (and each game) has its own scroll: otherwise a new page opened
                // at the position the previous one was scrolled to (egui tells scroll areas apart by id).
                let page_key = (self.view, self.lib_page, self.ach_open.as_ref().map(|a| a.0));
                if self.view != View::Library {
                    ui.push_id(page_key, |ui| self.section_view(ui, &net));
                    return;
                }
                let page_row = self.lib_page.and_then(|id| self.row_map(&net).remove(&id));
                // For the "played in two weeks" line on the game page - load friends activity once.
                if page_row.is_some() && !crate::lite::on() && !self.activity_ui.asked && net.account.is_some() && !net.roster.read().unwrap().is_empty() {
                    self.activity_ui.asked = true;
                    self.ask_activity(&net);
                }
                if page_row.is_some() && !self.licenses_asked && net.account.is_some() {
                    self.licenses_asked = true;
                    let _ = self.tx.send(Cmd::Licenses);
                }
                // The table view has no left column - go back to the table with a button.
                if page_row.is_some() && table_mode && ui.button(tr!("< Все игры", "< All games")).clicked() {
                    self.lib_page = None;
                }
                let action = if let Some(r) = &page_row {
                    // Game page: news and player count from store data, achievements from the network.
                    let store = net.store.lock().unwrap();
                    let ex = store.extras.get(&r.appid);
                    let note = self.settings.game_notes.get(&r.appid).cloned().unwrap_or_default();
                    let data = libview::PageData {
                        achievements: net.achievements.get(&r.appid),
                        news: ex.and_then(|e| e.news.as_ref()),
                        players: ex.and_then(|e| e.players),
                        info: store.apps.get(&r.appid),
                        genres: ex.map(|e| e.genres.as_slice()).unwrap_or(&[]),
                        trailer: ex.and_then(|e| e.trailer.as_deref()),
                        support: ex.and_then(|e| e.support.as_deref()),
                        note: &note,
                    };
                    libview::page(ui, r, &vc, &self.covers, &data, self.settings.covers)
                } else {
                    // "Home": title, view, "What to play?", then the grid or the list.
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        let title = match &self.collection {
                            None => tr!("Все игры", "All games").to_string(),
                            Some(id) if id == FAMILY => tr!("Семейная библиотека", "Family library").to_string(),
                            Some(id) if id == UNPLAYED => tr!("Ни разу не запускались", "Never launched").to_string(),
                            Some(id) if id == READY => tr!("Готово к игре", "Ready to play").to_string(),
                            Some(id) if id == UPDATES => tr!("Есть обновление", "Update available").to_string(),
                            Some(id) => net
                                .collections
                                .as_ref()
                                .and_then(|c| c.iter().find(|c| &c.id == id))
                                .map(|c| c.name.clone())
                                .unwrap_or_else(|| tr!("Коллекция", "Collection").into()),
                        };
                        let title = match &self.genre {
                            Some(g) => format!("{title} · {g}"),
                            None => title,
                        };
                        ui.label(egui::RichText::new(format!("{title} ({})", rows.len())).size(17.0).strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if self.settings.covers {
                                ui.selectable_value(&mut self.settings.view, settings::View::List, tr!("Таблица", "Table"))
                                    .on_hover_text(tr!("Компактный вид: игры таблицей, без обложек", "Compact view: games in a table, no covers"));
                                ui.selectable_value(&mut self.settings.view, settings::View::Grid, tr!("Плитка", "Grid"));
                            }
                            if self.settings.random_button
                                && !rows.is_empty()
                                && ui
                                    .button(tr!("Во что поиграть?", "What to play?"))
                                    .on_hover_text(tr!("Случайная игра из показанных (с учетом поиска, коллекции и фильтров)", "A random game from the shown ones (search, collection and filters apply)"))
                                    .clicked()
                            {
                                let seed = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .map(|d| d.subsec_nanos() as usize ^ d.as_secs() as usize)
                                    .unwrap_or(0);
                                self.lib_page = Some(rows[seed % rows.len()].appid);
                            }
                        });
                    });
                    ui.add_space(4.0);
                    // Table: search and filters as a row under the title (in the grid view they are in the left column, also under "Home").
                    if table_mode {
                        ui.horizontal_wrapped(|ui| self.library_filters(ui, &net, true)); // in a narrow window the filters wrap
                        ui.add_space(4.0);
                    }
                    // Without covers ("Performance" setting) - always the list.
                    if self.settings.view == settings::View::List || !self.settings.covers {
                        libview::list(ui, &rows, &vc, &self.covers, &mut self.settings.sort, self.settings.covers)
                    } else {
                        if self.collection.is_none() && self.filter.trim().is_empty() && self.genre.is_none() {
                            self.whats_new_shelf(ui, &net);
                        }
                        // The "Recent games" shelf - on "All games" without a search, as in Steam.
                        let recent: Vec<Row> = if self.collection.is_none() && self.filter.trim().is_empty() && self.genre.is_none() {
                            let mut r: Vec<Row> = rows.iter().filter(|r| r.last_played > 0).cloned().collect();
                            r.sort_by(|a, b| b.last_played.cmp(&a.last_played));
                            r.truncate(12);
                            r
                        } else {
                            Vec::new()
                        };
                        libview::grid(ui, &rows, &vc, &self.covers, &recent)
                    }
                };
                let action = side_action.take().or(action);
                // A game page was opened (by any path: click, notification, menu) - request
                // achievements, news and player count once.
                if let Some(p) = self.lib_page {
                    if self.page_loaded != Some(p) && net.account.is_some() && !custom::is_custom(p) {
                        self.page_loaded = Some(p);
                        if net.account.is_some() && !net.achievements.contains_key(&p) {
                            let _ = self.tx.send(Cmd::Achievements(p));
                        }
                        let _ = self.tx.send(Cmd::Store(store::StoreCmd::Page(p)));
                        // Developer, publisher, release date - from store details.
                        let _ = self.tx.send(Cmd::Store(store::StoreCmd::Details(vec![p])));
                        // Game DLC (what is owned) - for the page.
                        let _ = self.tx.send(Cmd::PageDlc(p));
                    }
                } else {
                    self.page_loaded = None;
                }
                match action {
                    Some(libview::Action::Play(appid)) => self.start(appid, ctx),
                    Some(libview::Action::Update(appid)) => self.start_update(appid, ctx),
                    Some(libview::Action::Install(appid, name)) => self.enqueue(appid, name, &net),
                    Some(libview::Action::AskInstall(appid, name)) => self.install_ask = Some((appid, name)),
                    Some(libview::Action::Props(appid, name)) => self.open_props(appid, name),
                    Some(libview::Action::Achievements(appid, name)) => {
                        if !net.achievements.contains_key(&appid) {
                            let _ = self.tx.send(Cmd::Achievements(appid));
                        }
                        self.close_extras(View::Achievements);
                        self.ach_open = Some((appid, name));
                        self.view = View::Achievements;
                    }
                    Some(libview::Action::Page(appid)) => {
                        // Achievements and news are requested above, when the page is shown.
                        self.lib_page = Some(appid);
                    }
                    Some(libview::Action::Store(appid)) => self.open_store(Some(appid), ctrl),
                    Some(libview::Action::Workshop(appid, name)) => self.open_workshop(appid, name, ctrl),
                    Some(libview::Action::Community(appid, name)) => self.open_community(appid, name),
                    Some(libview::Action::Shots(appid)) => self.open_shots(Some(appid)),
                    Some(libview::Action::Folder(appid)) => {
                        if let Some(g) = self.games.iter().find(|g| g.appid == appid) {
                            local::open_folder(&g.dir);
                        } else if let Some(dir) = self.settings.custom_games.iter().find(|g| g.id == appid).and_then(|g| std::path::Path::new(&g.exe).parent().map(|p| p.to_path_buf())) {
                            local::open_folder(&dir);
                        }
                    }
                    Some(libview::Action::Import(appid, name)) => self.import_game(appid, name),
                    Some(libview::Action::Stop(appid)) => self.stop_running(appid, false),
                    Some(libview::Action::Pause(appid)) => {
                        if let Some(inst) = net.install.as_ref().filter(|i| i.result.is_none() && i.appid == appid) {
                            inst.abort.store(true, Ordering::Relaxed);
                            self.settings.downloads.retain(|q| q.appid != appid);
                            self.settings.downloads.insert(0, settings::QueueItem { appid, name: inst.name.clone(), paused: true });
                        }
                    }
                    Some(libview::Action::Note(appid, name)) => {
                        let text = self.settings.game_notes.get(&appid).cloned().unwrap_or_default();
                        self.note_edit = Some((appid, name, text));
                    }
                    Some(libview::Action::Shortcut(appid, name)) => self.make_shortcut(appid, &name),
                    Some(libview::Action::Uninstall(appid, name)) => self.ask_uninstall(appid, name),
                    Some(libview::Action::Collection { id, appid, add }) => {
                        let _ = self.tx.send(Cmd::Coll(colls::CollCmd::Toggle { id, appid, add }));
                    }
                    Some(libview::Action::NewCollection { name, appid }) => {
                        let _ = self.tx.send(Cmd::Coll(colls::CollCmd::Create { name, appid: Some(appid) }));
                    }
                    None => {}
                }
            });
        } else {
            // In the tray the window is off-screen: the library UI is not drawn (saves CPU),
            // the logic below (queue, schedule, notifications, chat) works as usual.
            egui::CentralPanel::default().show(ctx, |_| {});
        }
        self.schedule_tick(&net);
        self.offline_tick(&net);
        self.notices_tick(&net);
        self.steam_notifs_tick(&net);
        self.stats_tick(&net);
        self.popups_ui(ctx);
        self.custom_tick(ctx);
        self.wizard(ctx);
        self.uninstall_dialog(ctx, &net);
        self.note_dialog(ctx);
        self.run_request_tick(ctx);
        self.status_popup();
        self.popup_clicks(ctx, &net);
        self.finish_tick(&net);
        // Download progress - on the tray icon (bar and tooltip).
        tray::set_download(
            net.install
                .as_ref()
                .filter(|i| i.result.is_none() && i.total > 0 && i.download)
                .map(|i| (i.name.clone(), i.done as f32 / i.total as f32)),
        );
        self.zoom_tick(ctx);
        aurelia::steam_client::CELL_OVERRIDE.store(self.settings.download_cell, std::sync::atomic::Ordering::Relaxed);
        // Startup section (the "Open on startup" setting).
        if !self.start_applied {
            self.start_applied = true;
            if self.settings.start_view == 1 {
                self.open_store(None, false);
            }
        }
        self.run_queue(&net);
        self.guard_partials(&net);

        self.store_window(ctx, &net);
        self.inventory_window(ctx, &net);
        self.social_windows(ctx, &net);
        self.accounts_window(ctx, &net);
        self.install_ask_dialog(ctx, &net);
        self.key_window(ctx, &net);
        self.shot_comments_window(ctx, &net);
        self.workshop_window(ctx, &net);
        chatwin::show(ctx, &self.chat, &self.net, &self.tx);
        stats::frame(frame_start);
        self.net_snap = net;
        self.net_gen = net_gen;
    }
}

fn main() -> eframe::Result<()> {
    // Game shortcut ("--run <appid>"): if Vaporust is already open, pass it the request and exit.
    if gamemisc::hand_over_run() {
        return Ok(());
    }
    gamemisc::mark_running();
    // Portable mode (portable.txt next to the exe): data goes into the VaporustData folder.
    let persistence_path = portable::init();
    accounts::init();
    errlog::install_panic_hook();
    let options = eframe::NativeOptions {
        persistence_path,
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([960.0, 630.0])
            // Do not let the window shrink to unreadable (or be restored like that from saved settings).
            .with_min_inner_size([760.0, 460.0])
            // Permanent identifier: eframe stores settings under it (%APPDATA%Vaporust),
            // so they are not lost when the version in the title changes.
            .with_app_id("Vaporust")
            // Own frame (winframe); the normal Windows one is enabled in settings from the first frame.
            .with_decorations(false)
            .with_icon(icons::icon(icons::Kind::Launcher)),
        // First - a normal window in the middle; saved size and position are applied by winpos (with
        // a check that the window fits on the monitor). eframe's built-in memory is off: the window
        // opened either tiny or full screen.
        persist_window: false,
        centered: true,
        ..Default::default()
    };
    eframe::run_native(
        concat!("Vaporust ", env!("CARGO_PKG_VERSION")),
        options,
        Box::new(|cc| Ok(Box::new(LauncherApp::new(cc)))),
    )
}

/// The game is available only via the family library (no own copy on the account).
pub(crate) fn is_family_only(net: &net::NetState, appid: u32) -> bool {
    !net.owned.iter().any(|g| g.app_id == appid) && net.family.iter().any(|f| f.app_id == appid)
}

/// Whether to resume a broken download by itself: not "Cancel"/"Pause", a network error, at most 3 times in a row.
fn should_resume(err: &str, user_stop: bool, tries: u8) -> bool {
    !user_stop && network_error(err) && tries < 3
}

/// The download error looks like a lost connection (not lack of space, access and the like).
fn network_error(e: &str) -> bool {
    let t = e.to_lowercase();
    ["timed out", "timeout", "connection", "network", "cdn", "dns", "reset", "unreachable", "сет", "связ", "offline"]
        .iter()
        .any(|w| t.contains(w))
}

/// Achievements window: order, filter, friend for comparison.
#[derive(Default)]
struct AchState {
    sort: u8,
    filter: u8,
    friend: Option<u64>,
}

#[allow(non_snake_case)]
fn ACH_SORTS() -> [&'static str; 4] {
    [tr!("Как в игре", "As in the game"), tr!("Сначала редкие", "Rarest first"), tr!("Сначала частые", "Most common first"), tr!("Недавно полученные", "Recently unlocked")]
}
#[allow(non_snake_case)]
fn ACH_FILTERS() -> [&'static str; 3] {
    [tr!("Все", "All"), tr!("Полученные", "Unlocked"), tr!("Не полученные", "Locked")]
}

impl LauncherApp {
    /// Extra sections (screenshots, storage, badges...) open one at a time: opening
    /// a new one closes the previous, so tabs do not pile up in the top bar. `keep` - the one being opened.
    pub(crate) fn close_extras(&mut self, keep: View) {
        if keep != View::Screenshots {
            self.shots_win = None;
        }
        if keep != View::Storage {
            self.storage_win = None;
        }
        if keep != View::Badges {
            self.badges_ui.open = false;
        }
        if keep != View::Points {
            self.points_ui.open = false;
        }
        if keep != View::Family {
            self.family_ui.open = false;
        }
        if keep != View::Activity {
            self.activity_ui.open = false;
        }
        if keep != View::Stats {
            self.stats_ui.open = false;
        }
        if keep != View::Group {
            self.group_ui = Default::default();
        }
        if keep != View::Community {
            self.community_ui.appid = None;
        }
        if keep != View::Groups {
            self.groups_ui.open = false;
        }
    }
}
