#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[macro_use]
mod lang;
mod util;
mod activity;
mod anim;
mod autostart;
mod backup;
mod badges;
mod chatfmt;
mod chatwin;
mod colls;
mod collview;
mod smartcoll;
mod achview;
mod friendslist;
mod launch;
mod libside;
mod queue;
mod rows;
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
mod loginui;
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
    /// Game achievements - a page of the main window (properties are a separate window).
    Achievements,
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
#[derive(Clone, Default)]
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
    /// Grows each time the network state snapshot is copied: the library lists are rebuilt only then
    /// (or when their filters change), not on every frame.
    snap_count: u64,
    /// Library rows: (key, all games) and (key, filtered and sorted list).
    row_cache: std::cell::RefCell<(u64, std::sync::Arc<HashMap<u32, Row>>, u64, std::sync::Arc<Vec<Row>>)>,
    net_snap_at: std::time::Instant,
    /// Own profile requested (name for the top bar).
    own_profile_asked: bool,
    /// Back / forward history of pages (view, game page, collections page), as in Steam.
    nav_back: Vec<(View, Option<u32>, bool)>,
    nav_fwd: Vec<(View, Option<u32>, bool)>,
    nav_last: (View, Option<u32>, bool),
    /// Library: the collections page instead of the home page.
    collections_page: bool,
    /// The editor of a new smart collection.
    smart_edit: smartcoll::SmartEdit,
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
    /// Interface language of the previous frame (true - English): on a change the cached Steam texts are dropped.
    lang_seen: Option<bool>,
    /// SteamID seen on the previous frame: when it changes, everything loaded for the old account in
    /// the interface is dropped and asked for again.
    account_seen: Option<u64>,
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
    /// Games moved from VaporustStaging while Steam was open: the open Steam sees them only after a restart.
    unstaged_open: std::collections::HashSet<u32>,
    /// When the soundtrack player last checked whether the track ended.
    music_checked: Option<std::time::Instant>,
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
            snap_count: 0,
            row_cache: std::cell::RefCell::new((u64::MAX, Default::default(), u64::MAX, Default::default())),
            net_snap_at: std::time::Instant::now(),
            own_profile_asked: false,
            nav_back: Vec::new(),
            nav_fwd: Vec::new(),
            nav_last: (View::Library, None, false),
            collections_page: false,
            smart_edit: Default::default(),
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
            lang_seen: None,
            account_seen: None,
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
            unstaged_open: Default::default(),
            music_checked: None,
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
        // A previous run was closed or crashed during a game: unfreeze the steamwebhelper it froze.
        local::thaw_leftovers();
        // Updates: check GitHub (if enabled).
        if app.settings.check_updates {
            updater::check(&app.upd, ctx);
        }
        app.rescan();
        // Library folder chosen last time.
        if let Some(lib) = app.settings.library.clone() {
            // Compared without case and slash differences (older versions saved the registry spelling).
            let same = |a: &str, b: &str| a.replace('/', "\\").trim_end_matches('\\').eq_ignore_ascii_case(b.replace('/', "\\").trim_end_matches('\\'));
            if let Some(i) = app.libraries.iter().position(|l| same(&l.to_string_lossy(), &lib)) {
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
            View::News => {
                self.store_band(ui, net);
                self.news_contents(ui, net)
            }
            View::Badges => self.badges_contents(ui, net),
            View::Points => {
                self.store_band(ui, net);
                self.points_contents(ui, net)
            }
            View::Family => self.family_contents(ui, net),
            View::Activity => self.activity_contents(ui, net),
            View::Stats => self.stats_contents(ui, net),
            View::Community => self.community_contents(ui, net),
            View::Groups => self.groups_contents(ui, net),
            View::Group => self.group_contents(ui, net),
            View::ProfileEdit => self.profile_edit_contents(ui, net),
            View::Achievements => self.achievements_contents(ui, net),
            View::Cart => {
                self.store_band(ui, net);
                self.cart_contents(ui, net)
            }
            View::Music => self.music_contents(ui),
            View::Profile => self.profile_contents(ui, net),
            View::Friends => self.friends_page_contents(ui, net),
            View::Downloads => self.downloads_contents(ui, net),
            View::Library => {}
        }
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

    pub(crate) fn account_panel(&mut self, ui: &mut egui::Ui, net: &NetState) {
        if net.account.is_some() {
            // Signed in: the sections are drawn by nav::sections.
        } else if self.settings.local_mode {
            // No-login mode: only installed games and launching via Steam. The same top bar as with
            // a login, with the sections that work without it and a sign-in button.
            ui.horizontal(|ui| {
                let lib = nav::top_item(ui, self.view == View::Library, tr!("БИБЛИОТЕКА", "LIBRARY"));
                self.drop_down(ui, &lib, nav::Section::Library, net);
                if lib.clicked() {
                    self.view = View::Library;
                    self.lib_page = None;
                    self.collections_page = false;
                }
                if nav::top_item(ui, self.view == View::Settings, tr!("НАСТРОЙКИ", "SETTINGS")).clicked() {
                    self.view = View::Settings;
                }
                ui.label(egui::RichText::new(tr!("без входа", "no login")).size(12.0).color(theme::WEAK)).on_hover_text(tr!(
                    "Установленные игры из файлов Steam, запуск через Steam. Пароль Steam в Vaporust не вводится.",
                    "Installed games from Steam files, launched through Steam. The Steam password is not entered in Vaporust."
                ));
                // "Sign in": a menu of the saved accounts (their login is remembered, no password needed)
                // and "Another account...".
                let b = egui::Button::new(egui::RichText::new(tr!("Войти в аккаунт", "Sign in")).color(egui::Color32::WHITE)).fill(theme::ACCENT).rounding(3.0);
                let resp = ui
                    .add(b)
                    .on_hover_text(tr!("Включит магазин, друзей и чат, загрузку и обновление игр без Steam", "Turns on the store, friends and chat, game downloads and updates without Steam"));
                let menu_id = ui.make_persistent_id("signin_menu");
                if resp.clicked() {
                    ui.memory_mut(|m| m.toggle_popup(menu_id));
                }
                let mut pick: Option<Option<String>> = None; // Some(Some(dir)) - an account, Some(None) - another one
                egui::popup::popup_below_widget(ui, menu_id, &resp, egui::PopupCloseBehavior::CloseOnClick, |ui| {
                    ui.set_min_width(240.0);
                    let saved = accounts::load();
                    for e in saved.list.iter().filter(|e| !e.login.is_empty()) {
                        let title = if e.name.is_empty() { e.login.clone() } else { format!("{} ({})", e.name, e.login) };
                        if ui.button(crate::richtext::fix(ui, &title)).on_hover_text(tr!("Вход сохранен, пароль не нужен", "The login is saved, no password needed")).clicked() {
                            pick = Some(Some(e.dir.clone()));
                        }
                    }
                    if saved.list.iter().any(|e| !e.login.is_empty()) {
                        ui.separator();
                    }
                    if ui.button(tr!("Другой аккаунт...", "Another account...")).clicked() {
                        pick = Some(None);
                    }
                });
                match pick {
                    Some(Some(dir)) => {
                        self.settings.local_mode = false;
                        if dir == accounts::load().active {
                            let _ = self.tx.send(Cmd::Restore);
                        } else {
                            self.switch_account(ui.ctx(), dir);
                        }
                    }
                    Some(None) => {
                        self.settings.local_mode = false;
                        let mut s = self.net.lock().unwrap();
                        s.need_login = true;
                        s.message.clear();
                    }
                    None => {}
                }
            });
        } else if net.offline {
            // No connection: a quiet amber status in the top bar with the actions next to it.
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                ui.label(egui::RichText::new("VAPORUST").size(14.0).strong().color(theme::TEXT));
                // The sections that work without a connection: from any other one (downloads, store)
                // there must be a way back.
                if nav::top_item(ui, self.view == View::Library, tr!("БИБЛИОТЕКА", "LIBRARY")).clicked() {
                    self.view = View::Library;
                    self.lib_page = None;
                }
                if nav::top_item(ui, self.view == View::Settings, tr!("НАСТРОЙКИ", "SETTINGS")).clicked() {
                    self.view = View::Settings;
                }
                ui.add_space(8.0);
                let (dot, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                ui.painter().circle_filled(dot.center(), 4.0, egui::Color32::from_rgb(230, 170, 60));
                ui.label(egui::RichText::new(tr!("Нет связи со Steam", "No connection to Steam")).color(egui::Color32::from_rgb(230, 170, 60)))
                    .on_hover_text(trf!(
                        "Автономный режим: игры из сохраненного списка, запуск через Steam без сети.\n{}",
                        "Offline mode: games from the saved list, launched through Steam without network.\n{}",
                        net.offline_reason
                    ));
                if net.busy {
                    crate::spin::spinner(ui);
                } else if ui.add(egui::Button::new(tr!("Переподключиться", "Reconnect")).rounding(3.0)).clicked() {
                    self.offline_retry = Some(std::time::Instant::now());
                    let _ = self.tx.send(Cmd::Restore);
                }
                if ui
                    .add_enabled(!net.busy, egui::Button::new(tr!("Войти заново", "Sign in again")).frame(false))
                    .on_hover_text(tr!("Если дело не в сети: войти в аккаунт логином и паролем или QR-кодом", "If it is not the network: sign in with login and password or a QR code"))
                    .clicked()
                {
                    let mut s = self.net.lock().unwrap();
                    s.offline = false;
                    s.need_login = true;
                }
            });
        } else if self.login_screen(net) {
            // The login form itself is a page in the middle of the window (loginui.rs); the top bar
            // keeps only the name, the window buttons and dragging.
            ui.add_space(4.0);
            ui.label(egui::RichText::new("VAPORUST").size(14.0).strong().color(theme::TEXT));
            return;
        }
        if !net.message.is_empty() {
            ui.label(&net.message);
        }
    }

}

/// The login QR code in a white square `size` points wide (cells are whole points, so it stays sharp).
pub(crate) fn draw_qr(ui: &mut egui::Ui, url: &str, size: f32) {
    let Ok(code) = qrcode::QrCode::new(url.as_bytes()) else {
        ui.label(url);
        return;
    };
    let w = code.width();
    let colors = code.to_colors();
    let quiet = 3;
    let cell = (size / (w + 2 * quiet) as f32).floor().max(1.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 6.0, egui::Color32::WHITE);
    // Center the code inside the square.
    let used = (w + 2 * quiet) as f32 * cell;
    let rect = egui::Rect::from_min_size(rect.min + egui::vec2(((size - used) / 2.0).floor(), ((size - used) / 2.0).floor()), egui::vec2(used, used));
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
pub(crate) fn draw_seg(ui: &mut egui::Ui, seg: chatfmt::Seg, anims: &mut anim::Cache, who: &str) {
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
            // A label that pretends to be another site's address is replaced with the real address.
            let shown = if chatfmt::misleading_label(&url, &label) { url.clone() } else { label };
            let r = ui
                .add(egui::Label::new(egui::RichText::new(shown).color(ui.visuals().hyperlink_color).underline()).sense(egui::Sense::click()))
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(&url);
            if r.clicked() {
                chatwin::open_link(ui.ctx(), &url);
            }
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
        Seg::Image { url, .. } if !chatfmt::trusted_image(&url) && !chatwin::image_revealed(&url) => {
            // Not a Steam site: do not load it by ourselves (the sender would see our IP address).
            let host = chatfmt::host_of(&url);
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new(trf!("Картинка с сайта {host}", "Image from {host}")).color(egui::Color32::from_gray(160)));
                if ui.small_button(tr!("Показать", "Show")).on_hover_text(tr!("Vaporust загрузит картинку с этого сайта: сайт увидит ваш IP-адрес", "Vaporust will load the image from this site: the site will see your IP address")).clicked() {
                    chatwin::reveal_image(&url);
                }
                if ui.small_button(tr!("Открыть ссылку", "Open link")).clicked() {
                    chatwin::open_link(ui.ctx(), &url);
                }
            });
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
                chatwin::open_link(ui.ctx(), &url);
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

    /// Vaporust closes: the launch thread is cut off, so whatever it froze is unfrozen here.
    /// Screenshot tour: the real mouse over the window must not leave hover effects and tooltips.
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        if self.tour.is_some() {
            raw.events.retain(|e| !matches!(e, egui::Event::PointerMoved(_) | egui::Event::PointerButton { .. } | egui::Event::MouseWheel { .. }));
            raw.events.push(egui::Event::PointerGone);
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        local::thaw_leftovers();
        // Closed normally: a fresh update works.
        updater::confirm_update();
    }

    /// eframe calls this on exit and periodically (every 30 s).
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        // The screenshot tour switches views and options: it must not overwrite the real settings.
        if std::env::var_os("VAPORUST_TOUR").is_some() || std::env::var_os("VAPORUST_NOSAVE").is_some() {
            return;
        }
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
        self.language_tick();
        imgload::ANIMATIONS.store(self.settings.animations, Ordering::Relaxed);
        imgload::ECONOMY.store(self.settings.economy, Ordering::Relaxed);
        imgload::budget_tick(ctx);
        self.memory_tick(ctx);
        self.music_tick(ctx);
        // Network state snapshot per frame so the lock is not held. Copied again only
        // when the state changed (and every 2 s just in case), otherwise the previous copy is used.
        let gen = self.net.gen();
        if gen != self.net_gen || self.net_snap_at.elapsed() >= std::time::Duration::from_secs(2) {
            self.net_snap = self.net.lock().unwrap().clone();
            self.net_gen = gen;
            self.snap_count += 1;
            self.net_snap_at = std::time::Instant::now();
        }
        let mut net = std::mem::take(&mut self.net_snap);
        let net_gen = std::mem::replace(&mut self.net_gen, u64::MAX);
        self.account_tick(&net);
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
                // Started into the tray (--tray): not maximized, or eframe would unfold it on the screen.
                if winpos::apply(p) && !self.start_hidden {
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
                self.title_rows(ui, &net);
            });
            winframe::remember_drag(ctx, drag_id, top.response.rect);
            self.update_banner(ctx);

            let bottom_frame = egui::Frame::none().fill(theme::TOP).inner_margin(egui::Margin::symmetric(8.0, 4.0));
            // No bottom bar on the login page and in the first-run wizard: only the card is there.
            let bottom_bar = !(self.login_screen(&net) || self.wizard_active(&net));
            egui::TopBottomPanel::bottom("status").frame(bottom_frame).show_animated(ctx, bottom_bar, |ui| {
                // A thin light line on top, as the inset border of the Steam bottom bar.
                let top_y = ui.max_rect().top() - 4.0;
                ui.painter().hline(ui.max_rect().x_range().expand(8.0), top_y, egui::Stroke::new(1.0, egui::Color32::from_rgba_unmultiplied(61, 68, 80, 166)));
                let full = ui.max_rect();
                let row = ui.horizontal(|ui| {
                    // On the left, as in Steam - "Add a game" (a menu that opens upwards).
                    let add_id = ui.make_persistent_id("add_game_menu");
                    let open = ui.memory(|m| m.is_popup_open(add_id));
                    let add = theme::bar_button(ui, tr!("Добавить игру", "Add a Game"), Some(theme::icon_plus), None, open);
                    if add.clicked() {
                        ui.memory_mut(|m| m.toggle_popup(add_id));
                    }
                    egui::popup::popup_above_or_below_widget(ui, add_id, &add, egui::AboveOrBelow::Above, egui::PopupCloseBehavior::CloseOnClick, |ui| {
                        ui.set_min_width(220.0);
                        if ui.button(tr!("Добавить стороннюю игру...", "Add a non-Steam game...")).on_hover_text(tr!("Игра или программа не из Steam", "A game or program not from Steam")).clicked() {
                            self.custom_edit = Some(custom::Edit { to_steam: self.settings.custom_to_steam, ..Default::default() });
                        }
                        if ui
                            .button(tr!("Взять сторонние игры из Steam", "Import non-Steam games from Steam"))
                            .on_hover_text(tr!("Игры, добавленные в настоящий Steam как сторонние, появятся и в Vaporust", "Games added to the real Steam as non-Steam games also appear in Vaporust"))
                            .clicked()
                        {
                            self.import_steam_shortcuts();
                        }
                        // A key is activated on Steam's servers for the signed-in account.
                        if net.account.is_some()
                            && ui.button(tr!("Активировать в Steam...", "Activate on Steam...")).on_hover_text(tr!("Ключ продукта Steam: игра, дополнение, набор", "Steam product key: a game, DLC, bundle")).clicked()
                        {
                            self.key_input = Some(String::new());
                            self.key_refreshed = false;
                            self.net.lock().unwrap().key_result = None;
                        }
                    });
                    // On the right, as in Steam - "Friends and chat".
                    if net.account.is_some() && !self.settings.games_only {
                        let unread: u32 = net.chats.values().map(|c| c.unread).sum();
                        let label = if unread > 0 { trf!("Друзья и чат ({unread})", "Friends & Chat ({unread})") } else { tr!("Друзья и чат", "Friends & Chat").to_string() };
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if theme::bar_button(ui, &label, None, Some(theme::icon_friends), self.show_friends).clicked() {
                                self.show_friends = !self.show_friends;
                            }
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

            if net.account.is_some() && self.show_friends && !self.settings.games_only && !self.settings.friends_window {
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
            let rows = if self.view == View::Library { self.rows(&net) } else { Default::default() };
            let queued: std::collections::HashSet<u32> =
                self.settings.downloads.iter().filter(|q| !q.paused).map(|q| q.appid).collect();
            let paused: std::collections::HashSet<u32> =
                self.settings.downloads.iter().filter(|q| q.paused).map(|q| q.appid).collect();
            let status_text = self.launch_status.lock().unwrap().clone();
            let vc = libview::Ctx {
                list_title: self.list_title(&net),
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
            let login_screen = self.login_screen(&net);
            let wizard = !login_screen && self.wizard_active(&net);
            if self.view == View::Library && !table_mode && !login_screen && !wizard {
                let side = egui::Frame::none().fill(theme::SIDEBAR).inner_margin(egui::Margin::ZERO);
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
                if login_screen {
                    self.login_page(ui, &net);
                    return;
                }
                if wizard {
                    self.wizard_page(ui);
                    return;
                }
                if self.view != View::Library {
                    ui.push_id(page_key, |ui| self.section_view(ui, &net));
                    return;
                }
                let page_row = self.lib_page.and_then(|id| self.row_map(&net).get(&id).cloned());
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
                } else if self.collections_page {
                    self.collections_view(ui, &net);
                    None
                } else {
                    // "Home": title, view, "What to play?", then the grid or the list.
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        let title = self.list_title(&net);
                        // In the grid the title and count are its own header; the table keeps the title here.
                        if table_mode {
                            ui.label(egui::RichText::new(format!("{title} ({})", rows.len())).font(theme::semibold(16.0)).color(theme::TEXT));
                        }
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
                    // Nothing to show: say why and what to do, instead of an empty background.
                    if rows.is_empty() {
                        self.empty_library(ui, &net);
                        None
                    } else if self.settings.view == settings::View::List || !self.settings.covers {
                        // Without covers ("Performance" setting) - always the list.
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
                        // "Play Next": owned games never launched, installed ones first (as the Steam shelf of
                        // games you own but have not played).
                        let next: Vec<Row> = if self.collection.is_none() && self.filter.trim().is_empty() && self.genre.is_none() {
                            let mut r: Vec<Row> = rows.iter().filter(|r| r.playtime_min == Some(0) && !r.family && !custom::is_custom(r.appid)).cloned().collect();
                            r.sort_by_key(|r| (!r.installed, r.name.to_lowercase()));
                            r.truncate(12);
                            r
                        } else {
                            Vec::new()
                        };
                        libview::grid(ui, &rows, &vc, &self.covers, &recent, &next)
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
                        if let Some(inst) = net.install.as_ref().filter(|i| i.result.is_none() && i.appid == appid && i.download) {
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
            // The store needs a login: in the no-login mode the library opens.
            if self.settings.start_view == 1 && !self.settings.local_mode && !self.settings.games_only {
                self.open_store(None, false);
            }
        }
        self.run_queue(&net);
        self.guard_partials(&net);

        self.store_window(ctx, &net);
        self.inventory_window(ctx, &net);
        self.social_windows(ctx, &net);
        self.friends_window(ctx, &net);
        self.props_window(ctx, &net);
        if self.smart_edit.open || self.smart_genre().is_some() {
            // Game tags come from the store: asked once for all library games (as for the genre filter),
            // also when a smart collection with a genre rule is chosen (after a restart nothing asked them).
            if !self.genre_asked && net.account.is_some() {
                self.genre_asked = true;
                let ids: Vec<u32> = self.row_map(&net).keys().copied().filter(|id| *id < crate::custom::BASE).collect();
                let _ = self.tx.send(Cmd::Store(store::StoreCmd::Details(ids)));
            }
            let genres = self.known_genres(&net);
            self.smart_dialog(ctx, &genres);
        }
        self.accounts_window(ctx, &net);
        self.install_ask_dialog(ctx, &net);
        self.key_window(ctx, &net);
        self.shot_comments_window(ctx, &net);
        self.workshop_window(ctx, &net);
        chatwin::show(ctx, &self.chat, &self.net, &self.tx);
        // "Link to another site" from a group chat (the friend chat window draws its own).
        if !notify::chat_open() {
            chatwin::link_warning(ctx);
        }
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
    // After a restart (account switch, update): the old window first saves its settings.
    portable::wait_for_previous();
    gamemisc::mark_running();
    // Portable mode (portable.txt next to the exe): data goes into the VaporustData folder.
    let persistence_path = portable::init();
    accounts::init();
    errlog::install_panic_hook();
    // A fresh update that could not start twice is rolled back here (before any window).
    updater::startup_guard();
    // Running for 15 s with the window open: a fresh update works. If the window fails to start,
    // the process ends earlier and the update stays "on trial".
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_secs(15));
        updater::confirm_update();
    });
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
            // The group stays known (back / forward returns to it), only a pending question is dropped.
            self.group_ui.confirm = None;
        }
        if keep != View::Groups {
            self.groups_ui.open = false;
        }
    }
}
