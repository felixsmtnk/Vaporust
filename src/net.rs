//! Working with the Steam network via the Aurelia library.
//! A separate thread with tokio: the UI sends commands, the thread updates the shared state.

use aurelia::core::models::{
    DepotPlatform, DownloadProgressState, DownloadState, OwnedGame, QrEvent, SessionState,
};
use aurelia::core::secret::Secret;
use aurelia::steam_client::{GameAchievement, InstallOptions, Roster, SteamClient};
use eframe::egui;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, RwLock,
    },
    thread,
    time::{Duration, Instant},
};
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};

pub enum Cmd {
    Restore,
    Login {
        user: String,
        pass: String,
        code: Option<String>,
    },
    LoginQr,
    /// Stop waiting for the QR code ("Cancel" button).
    CancelQr,
    RefreshLibrary,
    Achievements(u32),
    /// A friend's achievements in a game (appid, SteamID64) - for comparison.
    FriendAchievements(u32, u64),
    /// Install a game into the given Steam library.
    Install {
        appid: u32,
        name: String,
        library: PathBuf,
    },
    /// Load the conversation history with a friend.
    OpenChat(u64),
    /// Send a message to a friend.
    SendChat { to: u64, text: String },
    /// Look up game names by appid (for games friends are playing).
    ResolveApps(Vec<u32>),
    /// Game DLC for its library page: which are owned, and their names.
    PageDlc(u32),
    /// Load own emoticons and stickers (for the picker panel).
    LoadEmoticons,
    /// Update a game without Steam (Aurelia). acf - its appmanifest.
    /// Change a game's language without Steam (lang - as in UserConfig: "russian"...).
    SetLanguage { appid: u32, name: String, acf: PathBuf, lang: String },
    /// beta - Some(branch) if this is a beta change: after the download mark it installed (MountedConfig).
    UpdateDirect { appid: u32, name: String, acf: PathBuf, beta: Option<String> },
    /// Own room effects (from the Steam inventory) - reread when the "Effects" tab is opened.
    LoadChatEffects,
    /// Own screenshots in the Steam cloud: game (0 - all), page (from 1).
    /// Screenshots in the Steam cloud: whose (0 - own), game (0 - all), page.
    CloudShots { user: u64, appid: u32, page: u32 },
    /// Account info (country by IP, SteamID).
    AccountInfo,
    /// Steam notifications (the bell) and "mark all read".
    SteamNotifs,
    SteamNotifsRead,
    /// Item images from notifications: load inventories (inventory, context).
    NotifIcons(Vec<(u32, u32)>),
    /// Level, badges, cards; craft a badge from a set.
    Badges,
    CraftBadge { appid: u32, foil: bool },
    /// Steam Points: balance, catalog page, purchase.
    PointsBalance,
    PointsQuery { class: u32, search: String, cursor: Option<String> },
    PointsRedeem(crate::points::RewardItem),
    PointsOwned,
    /// Steam Family: members and games.
    Family,
    /// Friends' recent games.
    Activity(Vec<(u64, String)>),
    /// Own games over two weeks (for "Statistics").
    MyRecent,
    /// Whole profile (level, frame, games, groups, comments) and a comment on a profile.
    ProfileExtra(u64),
    /// Community group page by URL.
    GroupInfo(String),
    /// Join or leave a group.
    GroupMembership { url: String, info: crate::grouppage::GroupInfo, join: bool },
    PostComment { id: u64, text: String },
    /// Delete a comment from a profile (id - whose profile).
    DeleteComment { id: u64, gid: String },
    /// Load profile comments starting from start.
    MoreComments { id: u64, start: u32 },
    /// Screenshot: comments, own comment, vote.
    ShotComments { owner: u64, id: u64 },
    ShotComment { owner: u64, id: u64, text: String },
    ShotVote { id: u64, up: bool },
    /// Game community: discussion topics (page), topic, reply, guides.
    CommTopics { appid: u32, page: u32 },
    CommTopic(String),
    CommReply { topic: Box<crate::community::TopicView>, text: String },
    CommGuides { appid: u32, search: String },
    /// Open a guide in the program.
    CommGuide(u64),
    /// Group chats: list, channel messages, sending.
    /// Friends activity feed: from what time (0 - from scratch).
    Feed { start: u64 },
    /// Account licenses (purchase dates) - for the refund hint.
    Licenses,
    /// Wallet and purchase history (view only).
    WalletHistory,
    /// Own profile: load data for editing, save, privacy, avatar; friend nickname.
    ProfileEditLoad,
    ProfileEditSave(crate::profileedit::ProfileEdit),
    PrivacySave(crate::profileedit::Privacy),
    AvatarUpload(std::path::PathBuf),
    SetNickname(u64, String),
    /// Reaction to a chat message (add/remove).
    ChatReaction { friend: u64, time: u32, ordinal: u32, kind: i32, name: String, add: bool },
    /// Create a group chat with friends; invite a friend into the group.
    CreateChatGroup { name: String, invitees: Vec<u64> },
    InviteToChatGroup { group: u64, friend: u64 },
    /// Own game review: load, save.
    ReviewLoad(u32),
    ReviewSave { appid: u32, review: crate::actions::MyReview },
    /// New topic in the game's discussions.
    CreateTopic { appid: u32, title: String, text: String },
    /// Points awards: list, give an award (target type, target, award).
    AwardConfig,
    Award { target_type: u32, target: String, reaction: u32 },
    /// New wishlist order: (appid, position).
    WishlistReorder(Vec<(u32, u32)>),
    /// Cart: load, add a game (package), remove a line.
    CartLoad,
    CartAdd(u32),
    CartRemove(u64),
    /// Purchase options of a game (packages) - for "Add to cart".
    AppPackages(u32),
    /// Curators who reviewed the game.
    Curators(u32),
    /// Steam Family: invite, cancel an invite.
    FamilyInvite { family: String, steamid: u64, role: u32 },
    FamilyCancel { family: String, steamid: u64 },
    /// Workshop: game collections, collection contents, mod vote and favorite.
    WsCollections { appid: u32, text: String },
    WsCollectionItems(u64),
    WsVote { id: u64, up: bool },
    WsFavorite { id: u64, appid: u32, add: bool },
    /// List of download regions (cities with Steam servers).
    DownloadRegions,
    /// "Recent players" (who you played with).
    Coplay,
    /// Where the account is logged in.
    Sessions,
    /// Load an earlier part of the conversation with a friend.
    OlderChat(u64),
    ChatGroups,
    GroupMessages { group: u64, chat: u64 },
    GroupSend { group: u64, chat: u64, text: String },
    /// News feed of these games (appid, name).
    NewsFeed(Vec<(u32, String)>),
    /// Activate a Steam product key.
    ActivateKey(String),
    /// Send an image from a file.
    SendImage { to: u64, path: PathBuf },
    /// Profiles, friend requests, game invite, Workshop.
    Social(crate::social::SocialCmd),
    /// Steam collections: load, add/remove a game, create.
    Coll(crate::colls::CollCmd),
    /// Steam store command (search, game page, wishlist...).
    Store(crate::store::StoreCmd),
    Inv(crate::invent::InvCmd),
    /// Market and trades (selling, removing a listing, cancelling an order, trades).
    Market(crate::trade::MarketCmd),
    /// Friends status (EPersonaState: 1 online, 2 busy, 3 away, 7 invisible).
    SetPersona(u32),
    /// Check which installed games (appid, folder) need an update.
    CheckUpdates(Vec<(u32, PathBuf)>),
    /// Load data for the "Properties" window.
    LoadProps(u32),
    /// Enable/disable a DLC (base - the game, to reload the list).
    SetDlc { base: u32, dlc: u32, enabled: bool },
    /// Verify the integrity of game files.
    Verify { appid: u32, name: String },
    /// Move a game to another Steam library.
    Move { appid: u32, name: String, dest: PathBuf },
    /// Uninstall a game from disk.
    Uninstall { appid: u32, name: String },
    /// Remove a game from a folder shared with other games (others): only its own files.
    UninstallShared { appid: u32, name: String, dir: PathBuf, others: Vec<u32> },
    /// Pinned version: write the branch's latest depot versions into appmanifest so that
    /// the real Steam considers the game up to date. launch - launch the game afterwards.
    PinRefresh { appid: u32, path: PathBuf, branch: String, launch: bool },
    /// Rollback: download the given (old) depot versions (depot -> manifest ID).
    Downgrade { appid: u32, name: String, library: PathBuf, overrides: HashMap<u32, u64> },
    /// Manual sync of cloud saves. remote - userdata/<account>/<appid>/remote,
    /// dir - the game folder; direction: 0 download, 1 upload, 2 both ways; take: on
    /// conflict take the cloud (Some(true)) or the local (Some(false)) copy, None - only show.
    CloudSync { appid: u32, remote: PathBuf, dir: PathBuf, direction: u8, take: Option<bool> },
    /// Game launch table for launching without Steam.
    DirectSpec(u32),
    /// Aurelia settings: download speed limit (kbit/s, 0 - none) and proxy.
    AureliaConfig { limit_kbit: u64, proxy: Option<String> },
    /// Add an already downloaded game (files in library/steamapps/common/<folder>) and verify the files.
    Import { appid: u32, name: String, library: PathBuf },
    Logout,
}

/// Result of Cmd::PinRefresh for the main window.
#[derive(Clone)]
pub struct PinDone {
    pub appid: u32,
    pub launch: bool,
    pub result: Result<(), String>,
}

/// One message in a conversation.
#[derive(Clone, Default)]
pub struct ChatLine {
    pub from_self: bool,
    pub text: String,
    /// Unix time, seconds.
    pub time: u64,
    /// Message number within the second (for reactions and for telling messages apart).
    pub ordinal: u32,
    /// time and ordinal come from the Steam server, so (time, ordinal) identifies the message.
    /// false - a local fallback time; such a line is matched by text and time only.
    pub exact: bool,
    /// Reactions: (type 1 emoticon / 2 sticker, name, who reacted - account numbers).
    pub reactions: Vec<(i32, String, Vec<u32>)>,
}

/// Conversation with one friend.
#[derive(Clone, Default)]
pub struct Conversation {
    pub lines: Vec<ChatLine>,
    /// Unread incoming messages.
    pub unread: u32,
    pub history_loaded: bool,
    /// Load/send error, if any.
    pub error: Option<String>,
    /// When the friend last typed (for "typing...").
    pub typing_at: Option<Instant>,
    /// What is happening now (for example "Отправляю картинку...").
    pub status: Option<String>,
    /// Success message (for example "Приглашение отправлено").
    pub status_note: Option<String>,
    /// The oldest loaded message (time, ordinal) and whether there are older ones.
    pub oldest: Option<(u32, u32)>,
    pub more_history: bool,
    pub loading_older: bool,
}

/// Result of a manual cloud sync (for the "Properties" window).
#[derive(Clone, Default)]
pub struct CloudOutcome {
    pub downloaded: usize,
    pub uploaded: usize,
    /// Differing: (file, local size, cloud size).
    pub conflicts: Vec<(String, u64, u64)>,
    pub skipped: usize,
    pub failed: Vec<String>,
}

/// Depot description from Steam data: name, OS (oslist), language, DLC appid.
pub type DepotDesc = (Option<String>, Option<String>, Option<String>, Option<u32>);

/// "Properties" window data of one game; None - still loading.
#[derive(Clone, Default)]
pub struct PropsData {
    pub languages: Option<Result<Vec<String>, String>>,
    /// Public branches: (name, developer description).
    pub branches: Option<Result<Vec<(String, Option<String>)>, String>>,
    pub dlc: Option<Result<Vec<DlcRow>, String>>,
    pub cloud: Option<Result<Vec<CloudFile>, String>>,
    /// Depot descriptions: depot -> (name, OS, language, DLC).
    pub depots: Option<Result<HashMap<u32, DepotDesc>, String>>,
    /// Result of the last manual cloud sync; cloud_busy - a sync is running.
    pub cloud_sync: Option<Result<CloudOutcome, String>>,
    pub cloud_busy: bool,
    /// Error of the last action (for example a DLC toggle).
    pub error: Option<String>,
}

#[derive(Clone)]
pub struct DlcRow {
    pub app_id: u32,
    pub owned: bool,
    pub installed: bool,
    pub disabled: bool,
}

#[derive(Clone)]
pub struct CloudFile {
    pub name: String,
    pub size: u64,
    pub time: u64,
}

/// Install progress of one game.
#[derive(Clone)]
pub struct Install {
    pub appid: u32,
    pub name: String,
    pub stage: String,
    pub done: u64,
    pub total: u64,
    /// Speed, bytes/s (smoothed).
    pub speed: f64,
    /// None while running; Some(Ok) - done; Some(Err) - error or cancel.
    pub result: Option<Result<(), String>>,
    pub abort: Arc<AtomicBool>,
    /// What to show on success ("установлена.", "файлы в порядке."...).
    pub done_text: String,
    /// A game download (install or resume), not another file operation: the schedule,
    /// the queue and notifications rely on it. Not derived from the text - it is translated.
    pub download: bool,
}

impl Install {
    pub fn active(&self) -> bool {
        self.result.is_none()
    }
}

#[derive(Default, Clone)]
pub struct NetState {
    /// Logged-in account name; None if not logged in.
    pub account: Option<String>,
    /// Show the login form.
    pub need_login: bool,
    /// No connection to Steam (login saved): offline mode, library from the cache.
    pub offline: bool,
    /// Text of the error that turned on offline mode (for the hint).
    pub offline_reason: String,
    /// How the saved login is stored: Some(true) - encrypted with a Windows key, Some(false) -
    /// the key store is unavailable and the file stayed open, None - not checked yet.
    pub session_secure: Option<bool>,
    pub busy: bool,
    pub message: String,
    pub qr_url: Option<String>,
    pub qr_scanned: bool,
    /// Heavy fields are behind Arc: the UI copies NetState on every change, and copying such
    /// fields is just a reference count. They change via Arc::make_mut (copying only that field).
    pub owned: Arc<Vec<OwnedGame>>,
    /// Games of the Steam family group (others', not owned by this account).
    pub family: Arc<Vec<aurelia::steam_client::SharedApp>>,
    pub achievements: Arc<HashMap<u32, Result<Vec<GameAchievement>, String>>>,
    /// Friends' achievements: (appid, SteamID64).
    pub friend_achievements: Arc<HashMap<(u32, u64), Result<Vec<GameAchievement>, String>>>,
    /// Current or last install.
    pub install: Option<Install>,
    /// Grows when something changed on disk and the library needs a rescan.
    pub disk_changed: u64,
    /// Friends (updated by an Aurelia background task).
    pub roster: Arc<RwLock<Roster>>,
    /// Conversations by friend SteamID64.
    pub chats: Arc<HashMap<u64, Conversation>>,
    /// Game names resolved by appid.
    pub app_names: Arc<HashMap<u32, String>>,
    /// Own emoticons and stickers; None - not loaded yet.
    pub emoticons: Option<(Vec<String>, Vec<String>)>,
    /// Own room effects (names for /roomeffect), from the Steam inventory.
    pub chat_effects: Option<Vec<String>>,
    /// Screenshots in the Steam cloud: (game filter, total, loaded pages); None - not loaded.
    pub cloud_shots: Option<Result<(u32, u32, Vec<aurelia::steam_client::CloudScreenshot>), String>>,
    pub cloud_shots_busy: bool,
    /// Whose screenshots are in cloud_shots (0 - own).
    pub cloud_shots_user: u64,
    /// Account info from Aurelia ("Settings" -> "Account" tab).
    pub account_data: Option<aurelia::steam_client::AccountData>,
    /// Key activation: in progress / result.
    pub key_busy: bool,
    /// Steam notifications.
    pub steam_notifs: Option<Result<crate::steamnotif::Notifs, String>>,
    /// Item images for notifications: item id -> image URL.
    pub notif_icons: HashMap<String, String>,
    pub badges: Option<Result<crate::badges::Badges, String>>,
    pub badges_msg: Option<String>,
    pub points_balance: Option<u64>,
    pub points_page: Option<Result<crate::points::Page, String>>,
    pub points_busy: bool,
    pub points_msg: Option<String>,
    pub points_owned: Option<crate::points::Owned>,
    /// Chat effects bought in this session (the Steam inventory returns them with a delay).
    pub bought_effects: Vec<String>,
    pub family_group: Option<Result<crate::family::Family, String>>,
    pub activity: Option<Result<Vec<crate::activity::FriendRecent>, String>>,
    pub activity_busy: bool,
    pub my_recent: Option<Result<Vec<crate::activity::Recent>, String>>,
    pub profile_extra: HashMap<u64, Result<crate::profile_extra::ProfileExtra, String>>,
    pub groups_info: HashMap<String, Result<crate::grouppage::GroupInfo, String>>,
    pub group_msg: Option<String>,
    pub comment_msg: Option<String>,
    pub shot_comments: HashMap<u64, Result<(u32, Vec<crate::profile_extra::Comment>), String>>,
    pub shot_msg: Option<String>,
    pub comm_topics: Option<Result<(u32, u32, Vec<crate::community::Topic>), String>>,
    pub comm_topic: Option<Result<crate::community::TopicView, String>>,
    pub comm_guides: Option<Result<(u32, Vec<crate::community::Guide>), String>>,
    pub comm_msg: Option<String>,
    pub comm_guide: Option<Result<crate::community::GuideView, String>>,
    pub chat_groups: Option<Result<Vec<aurelia::steam_client::ChatGroup>, String>>,
    pub group_messages: HashMap<(u64, u64), Result<Vec<aurelia::steam_client::GroupMessage>, String>>,
    pub group_error: Option<String>,
    pub feed: Option<Result<crate::feed::FeedPage, String>>,
    pub feed_busy: bool,
    pub licenses: Option<Vec<crate::refund::License>>,
    pub wallet_history: Option<Result<Vec<crate::wallet::WalletRow>, String>>,
    pub coplay: Option<Result<Vec<crate::coplay::CoplayGame>, String>>,
    pub profile_edit: Option<Result<crate::profileedit::ProfileEdit, String>>,
    pub profile_edit_msg: Option<String>,
    /// Own reviews per game (None inside - not written yet).
    pub my_reviews: HashMap<u32, Result<Option<crate::actions::MyReview>, String>>,
    /// Result of the last community/store action (review, topic, award, cart, order...).
    pub action_msg: Option<String>,
    pub award_config: Option<Result<Vec<(u32, u32)>, String>>,
    pub cart: Option<Result<crate::actions::Cart, String>>,
    pub download_regions: Option<Result<Vec<(u32, String)>, String>>,
    pub app_packages: HashMap<u32, Result<Vec<(u32, String, String)>, String>>,
    pub ws_collections: HashMap<(u32, String), Result<Vec<crate::actions::WsCollection>, String>>,
    pub ws_collection_items: HashMap<u64, Result<Vec<u64>, String>>,
    pub sessions: Option<Result<Vec<crate::sessions::Session>, String>>,
    /// News feed of own games.
    pub news_feed: Option<Result<Vec<crate::newsfeed::FeedItem>, String>>,
    pub news_busy: bool,
    pub key_result: Option<crate::keys::KeyResult>,
    /// "Properties" window data by appid.
    pub props: HashMap<u32, PropsData>,
    /// Version pinning: result of the last Cmd::PinRefresh (taken by the main window).
    pub pin_done: Option<PinDone>,
    /// A rollback finished successfully - the main window pins this game.
    pub pin_after: Option<u32>,
    /// Launch table for launching without Steam: (appid, Ok(Some) - found, Ok(None) - no Windows entry).
    pub direct_spec: Option<(u32, Result<Option<crate::direct::LaunchSpec>, String>)>,
    /// Games that need an update (None - not checked yet).
    pub updates: Option<std::collections::HashSet<u32>>,
    /// SteamID64 of the logged-in account.
    pub my_id: Option<u64>,
    /// User profiles by SteamID64.
    pub profiles: HashMap<u64, Result<aurelia::steam_client::UserProfile, String>>,
    /// /id/<name> links -> SteamID64 ("Add friend" window).
    pub vanity: HashMap<String, Result<u64, String>>,
    /// Result of the last friend action (request, removal).
    pub friend_msg: Option<String>,
    /// Who was blocked in this session (Steam reports a block with a delay).
    pub blocked: std::collections::HashSet<u64>,
    /// Workshop (separate: lists can be large).
    pub workshop: crate::social::SharedWorkshop,
    /// Steam collections (None - not loaded yet).
    pub collections: Option<Vec<crate::colls::Coll>>,
    pub collections_error: Option<String>,
    /// Store data (separate: there is a lot of it, and NetState is copied every frame).
    pub store: crate::store::SharedStore,
    /// Inventory, wallet, Community Market (separate, like the store).
    pub inv: crate::invent::SharedInv,
}

pub type Shared = Arc<SharedNet>;

/// Network state under a lock with a change counter: every write (DerefMut) increments
/// the counter, and the UI copies the state only when it changed, not every frame
/// (it holds conversations, news, achievements - copying them every frame is expensive).
#[derive(Default)]
pub struct SharedNet {
    inner: Mutex<NetState>,
    gen: std::sync::atomic::AtomicU64,
}

pub struct NetGuard<'a> {
    g: std::sync::MutexGuard<'a, NetState>,
    gen: &'a std::sync::atomic::AtomicU64,
}

impl std::ops::Deref for NetGuard<'_> {
    type Target = NetState;
    fn deref(&self) -> &NetState {
        &self.g
    }
}

impl std::ops::DerefMut for NetGuard<'_> {
    fn deref_mut(&mut self) -> &mut NetState {
        self.gen.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        &mut self.g
    }
}

impl SharedNet {
    /// Like a Mutex: `.lock().unwrap()` (it never fails - a poisoned lock is simply taken).
    pub fn lock(&self) -> Result<NetGuard<'_>, std::convert::Infallible> {
        Ok(NetGuard { g: self.inner.lock().unwrap_or_else(|e| e.into_inner()), gen: &self.gen })
    }

    /// Change number: changes on every write.
    pub fn gen(&self) -> u64 {
        self.gen.load(std::sync::atomic::Ordering::Relaxed)
    }
}

fn upd(st: &Shared, ctx: &egui::Context, f: impl FnOnce(&mut NetState)) {
    // A response of a task from a previous login (logged out or logged into another account) - not written. The check
    // is under the same lock as the write: otherwise the login could change between the check and the write.
    let Some(mut g) = lock_current(st) else { return };
    f(&mut g);
    drop(g);
    ctx.request_repaint();
    // The chat window is a separate viewport: we wake it too (new messages, statuses), but only
    // if it is open - otherwise eframe briefly creates an empty "egui window".
    if crate::notify::chat_open() {
        ctx.request_repaint_of(crate::chatwin::viewport_id());
    }
}

pub fn spawn_worker(st: Shared, ctx: egui::Context) -> UnboundedSender<Cmd> {
    let (tx, mut rx) = unbounded_channel::<Cmd>();
    thread::spawn(move || {
        let rt = match tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(e) => {
                upd(&st, &ctx, |s| s.message = trf!("Не удалось запустить сеть: {e}", "Could not start the network: {e}"));
                return;
            }
        };
        rt.block_on(async move {
            let mut client = match SteamClient::new() {
                Ok(c) => c,
                Err(e) => {
                    upd(&st, &ctx, |s| s.message = trf!("Ошибка Aurelia: {e:#}", "Aurelia error: {e:#}"));
                    return;
                }
            };
            // Commands run in order. The exception is QR login: it waits up to 5 minutes, and all that time
            // other commands are accepted. A password login, a new QR or "Cancel" interrupt
            // the wait right away, other commands run afterwards.
            let mut later: std::collections::VecDeque<Cmd> = Default::default();
            loop {
                let cmd = match later.pop_front() {
                    Some(c) => c,
                    None => match rx.recv().await {
                        Some(c) => c,
                        None => break,
                    },
                };
                if !matches!(cmd, Cmd::LoginQr) {
                    handle(&mut client, cmd, &st, &ctx).await;
                    continue;
                }
                let mut interrupted = false;
                {
                    let fut = handle(&mut client, cmd, &st, &ctx);
                    tokio::pin!(fut);
                    loop {
                        tokio::select! {
                            _ = &mut fut => break,
                            c = rx.recv() => match c {
                                Some(c @ (Cmd::Login { .. } | Cmd::LoginQr | Cmd::CancelQr)) => {
                                    later.push_front(c);
                                    interrupted = true;
                                    break;
                                }
                                Some(c) => later.push_back(c),
                                None => return,
                            },
                        }
                    }
                }
                if interrupted {
                    upd(&st, &ctx, |s| {
                        s.qr_url = None;
                        s.qr_scanned = false;
                        s.busy = false;
                        s.message.clear();
                    });
                }
            }
        });
    });
    tx
}

async fn handle(client: &mut SteamClient, cmd: Cmd, st: &Shared, ctx: &egui::Context) {
    // Safety net: experimental actions are not run without the switch.
    if crate::exp::guarded(&cmd) && !crate::exp::on() {
        upd(st, ctx, |s| s.message = tr!("Это экспериментальная функция: включите ее в «Настройки -> Аккаунт».", "This is an experimental feature: turn it on in \"Settings -> Account\".").into());
        return;
    }
    match cmd {
        Cmd::Restore => {
            upd(st, ctx, |s| {
                s.busy = true;
                s.message = tr!("Вход в Steam...", "Logging in to Steam...").into();
            });
            match client.restore_session().await {
                Ok(session) => logged_in(client, session, st, ctx).await,
                // No connection to Steam, but the login is saved: offline mode - library from the Aurelia cache
                // (game list and playtime as of the last login); the UI tries to reconnect by itself.
                Err(e) if client.is_offline() || network_failure(&e).await => {
                    // This account's own cache (by SteamID from the saved login), not Aurelia's shared cache.
                    let me = aurelia::core::config::load_session().await.ok().and_then(|s| s.steam_id);
                    let cached = load_library_cache(me);
                    let why = format!("{e:#}");
                crate::errlog::write(tr!("сеть", "network"), &trf!("нет связи со Steam: {why}", "no connection to Steam: {why}"));
                    upd(st, ctx, |s| {
                        s.busy = false;
                        s.offline = true;
                        s.offline_reason = why;
                        s.need_login = false;
                        s.message.clear();
                        if s.owned.is_empty() {
                            s.owned = Arc::new(cached);
                        }
                    });
                }
                Err(_) => upd(st, ctx, |s| {
                    s.busy = false;
                    s.offline = false;
                    s.need_login = true;
                    s.message.clear();
                }),
            }
        }
        Cmd::Login { user, pass, code } => {
            upd(st, ctx, |s| {
                s.busy = true;
                s.message = tr!("Вход... Если включен мобильный аутентификатор, подтвердите вход в приложении Steam.", "Logging in... If the mobile authenticator is on, confirm the login in the Steam app.").into();
            });
            match client.login(user, Secret::new(pass), code, None).await {
                Ok(session) => logged_in(client, session, st, ctx).await,
                Err(e) => {
                    let needs_code = !client.pending_confirmations().is_empty();
                    upd(st, ctx, |s| {
                        s.busy = false;
                        s.message = if needs_code {
                            tr!("Steam просит код подтверждения. Введите код из письма или приложения \
                             и снова нажмите «Войти» (пароль тоже нужно ввести заново).", "Steam asks for a confirmation code. Enter the code from the email or the app and press \"Log in\" again (the password has to be entered again too).")
                                .into()
                        } else {
                            trf!("Не удалось войти: {e:#}", "Could not log in: {e:#}")
                        };
                    });
                }
            }
        }
        Cmd::LoginQr => {
            upd(st, ctx, |s| {
                s.busy = true;
                s.message = tr!("Получаю QR-код...", "Getting a QR code...").into();
            });
            let (st2, ctx2) = (st.clone(), ctx.clone());
            let result = client
                .login_qr(Duration::from_secs(300), move |ev| match ev {
                    QrEvent::Challenge(url) => upd(&st2, &ctx2, |s| {
                        s.qr_url = Some(url.to_string());
                        s.qr_scanned = false;
                        s.message.clear();
                    }),
                    QrEvent::Scanned => upd(&st2, &ctx2, |s| s.qr_scanned = true),
                })
                .await;
            upd(st, ctx, |s| {
                s.qr_url = None;
                s.qr_scanned = false;
            });
            match result {
                Ok(session) => logged_in(client, session, st, ctx).await,
                Err(e) => upd(st, ctx, |s| {
                    s.busy = false;
                    s.message = trf!("Вход по QR не удался: {e:#}", "QR login failed: {e:#}");
                }),
            }
        }
        Cmd::CancelQr => {} // the QR wait was already interrupted in the command loop
        Cmd::RefreshLibrary => {
            upd(st, ctx, |s| s.busy = true);
            let (mut client, st, ctx) = (client.clone(), st.clone(), ctx.clone());
            spawn_session(async move { load_owned(&mut client, &st, &ctx).await });
        }
        Cmd::FriendAchievements(appid, who) => {
            let (client, st, ctx) = (client.clone(), st.clone(), ctx.clone());
            spawn_session(async move {
                let res = client.fetch_achievements_for(appid, crate::lang::steam(), who).await.map_err(|e| format!("{e:#}"));
                upd(&st, &ctx, |s| {
                    Arc::make_mut(&mut s.friend_achievements).insert((appid, who), res);
                });
            });
        }
        Cmd::Achievements(appid) => {
            // In the background: a slow response does not hold up other commands.
            let (client, st, ctx) = (client.clone(), st.clone(), ctx.clone());
            spawn_session(async move {
                let res = client
                    .fetch_achievements(appid, crate::lang::steam())
                    .await
                .or_else(|e| {
                    let msg = format!("{e:#}");
                    // The game has no achievements: Steam answers with a NoMatch error, not an empty list.
                    if msg.contains("NoMatch") { Ok(Vec::new()) } else { Err(msg) }
                });
                upd(&st, &ctx, |s| {
                    Arc::make_mut(&mut s.achievements).insert(appid, res);
                });
            });
        }
        Cmd::Install {
            appid,
            name,
            library,
        } => {
            if st.lock().unwrap().install.as_ref().is_some_and(|i| i.active()) {
                return;
            }
            let abort = Arc::new(AtomicBool::new(false));
            upd(st, ctx, |s| {
                s.install = Some(Install {
                    appid,
                    name,
                    stage: tr!("Подготовка...", "Preparing...").into(),
                    done: 0,
                    total: 0,
                    speed: 0.0,
                    result: None,
                    abort: abort.clone(),
                    done_text: tr!("установлена.", "installed.").into(),
                    download: true,
                });
            });
            // The install runs in a separate task so other commands do not wait.
            let (client, st, ctx) = (client.clone(), st.clone(), ctx.clone());
            tokio::spawn(async move {
                let res = run_install(client, appid, library, abort, &st, &ctx).await;
                upd(&st, &ctx, |s| {
                    if let Some(i) = s.install.as_mut() {
                        i.result = Some(res);
                        i.speed = 0.0;
                    }
                    s.disk_changed += 1;
                });
            });
        }
        Cmd::OpenChat(friend) => {
            // In the background: while one chat's history loads, other commands do not wait.
            let (client, st, ctx) = (client.clone(), st.clone(), ctx.clone());
            spawn_session(async move {
            let res = client.chat_history(friend, 50, None).await;
            upd(&st, &ctx, |s| {
                let conv = Arc::make_mut(&mut s.chats).entry(friend).or_default();
                conv.unread = 0;
                match res {
                    Ok((msgs, more)) => {
                        conv.oldest = msgs.iter().map(|m| (m.timestamp, m.ordinal)).min();
                        conv.more_history = more;
                        // Steam returns newest to oldest. Messages received or sent while
                        // the history was loading may be newer than its snapshot - we keep them (merge_history).
                        let history: Vec<ChatLine> = msgs
                            .into_iter()
                            .rev()
                            .map(|m| ChatLine {
                                from_self: m.from_self,
                                text: m.message,
                                time: m.timestamp as u64,
                                ordinal: m.ordinal,
                                exact: true,
                                reactions: m.reactions,
                            })
                            .collect();
                        conv.lines = merge_history(history, std::mem::take(&mut conv.lines));
                        conv.history_loaded = true;
                        conv.error = None;
                    }
                    Err(e) => conv.error = Some(trf!("Не удалось загрузить историю: {e:#}", "Could not load the history: {e:#}")),
                }
            });
            });
        }
        Cmd::PageDlc(appid) => {
            let (client, st, ctx) = (client.clone(), st.clone(), ctx.clone());
            spawn_session(async move {
                let list = load_dlc(&client, appid).await;
                let ids: Vec<u32> = list.as_ref().map(|l| l.iter().map(|d| d.app_id).collect()).unwrap_or_default();
                upd(&st, &ctx, |s| s.props.entry(appid).or_default().dlc = Some(list));
                // DLC names (at most 40, so Steam is not flooded with requests).
                let known: Vec<u32> = st.lock().unwrap().app_names.keys().copied().collect();
                for id in ids.into_iter().filter(|i| !known.contains(i)).take(40) {
                    let (name, _) = client.resolve_install_game_info(id).await;
                    if !name.starts_with("App ") {
                        upd(&st, &ctx, |s| {
                            Arc::make_mut(&mut s.app_names).insert(id, name);
                        });
                    }
                }
            });
        }
        Cmd::ResolveApps(ids) => {
            let (client, st, ctx) = (client.clone(), st.clone(), ctx.clone());
            spawn_session(async move {
                for id in ids {
                    let (name, _) = client.resolve_install_game_info(id).await;
                    // Aurelia returns "App N" if the name was not found.
                    if !name.starts_with("App ") {
                        upd(&st, &ctx, |s| {
                            Arc::make_mut(&mut s.app_names).insert(id, name);
                        });
                    }
                }
            });
        }
        Cmd::SendChat { to, text } => {
            let orig = text.clone();
            let res = client.send_chat_message_ordered(to, text.clone()).await;
            upd(st, ctx, |s| {
                let conv = Arc::make_mut(&mut s.chats).entry(to).or_default();
                match res {
                    Ok(sent) => {
                        let time = if sent.server_timestamp > 0 {
                            sent.server_timestamp
                        } else {
                            now_unix()
                        };
                        // The server rewrites commands (/sticker) and emoticons; show them right away,
                        // the exact look comes from the history below.
                        let special = text.starts_with('/') || text.contains('ː');
                        conv.lines.push(ChatLine {
                            from_self: true,
                            text: sent.modified_message.unwrap_or(text),
                            time,
                            ordinal: sent.ordinal,
                            exact: sent.server_timestamp > 0,
                            ..Default::default()
                        });
                        conv.error = None;
                        if special {
                            conv.status = Some(tr!("Отправлено, обновляю...", "Sent, refreshing...").into());
                        }
                    }
                    Err(e) => conv.error = Some(trf!("Не отправлено: {e:#}", "Not sent: {e:#}")),
                }
            });
            let refresh = st.lock().unwrap().chats.get(&to).is_some_and(|c| c.status.is_some());
            if refresh {
                reload_history(client, to, st, ctx).await;
                upd(st, ctx, |s| {
                    let conv = Arc::make_mut(&mut s.chats).entry(to).or_default();
                    conv.status = None;
                    // The account does not have the effect (or sticker) - the server turns the command into an empty message.
                    let empty = conv.lines.iter().rev().find(|l| l.from_self).is_some_and(|l| l.text.trim().is_empty());
                    if empty && orig.starts_with("/roomeffect ") {
                        conv.error = Some(tr!("Эффект не отправлен: его нет в вашем аккаунте Steam (у друга могло появиться пустое сообщение).", "The effect was not sent: your Steam account does not have it (your friend may see an empty message).").into());
                    } else if empty && orig.starts_with("/sticker ") {
                        conv.error = Some(tr!("Стикер не отправлен: его нет в вашем аккаунте Steam.", "The sticker was not sent: your Steam account does not have it.").into());
                    }
                });
            }
        }
        Cmd::Coll(cmd) => crate::colls::handle(client, cmd, st, ctx),
        Cmd::Social(cmd) => crate::social::handle(client, cmd, st, ctx),
        Cmd::Store(cmd) => {
            let store = st.lock().unwrap().store.clone();
            crate::store::handle(client, cmd, &store, ctx);
        }
        Cmd::Inv(cmd) => {
            let inv = st.lock().unwrap().inv.clone();
            crate::invent::handle(client, cmd, &inv, ctx);
        }
        Cmd::Market(cmd) => {
            let inv = st.lock().unwrap().inv.clone();
            crate::trade::handle(client, cmd, &inv, ctx);
        }
        Cmd::SetPersona(state) => {
            let c = client.clone();
            spawn_session(async move {
                // The friends watcher announces the status from the Aurelia config by itself on login
                // (invisible); we wait for it and announce ours on top.
                tokio::time::sleep(Duration::from_secs(3)).await;
                let _ = c.announce_persona(state).await;
            });
        }
        Cmd::CheckUpdates(games) => {
            let (c, st, ctx) = (client.clone(), st.clone(), ctx.clone());
            spawn_session(async move {
                if let Ok(ids) = c.games_needing_update(&games).await {
                    upd(&st, &ctx, |s| s.updates = Some(ids.into_iter().collect()));
                }
            });
        }
        Cmd::LoadProps(appid) => load_props(client, appid, st, ctx),
        Cmd::SetDlc { base, dlc, enabled } => {
            let (c, st, ctx) = (client.clone(), st.clone(), ctx.clone());
            spawn_session(async move {
                let res = c.set_dlc_enabled(dlc, enabled).await.map_err(|e| format!("{e:#}"));
                let list = load_dlc(&c, base).await;
                upd(&st, &ctx, |s| {
                    let p = s.props.entry(base).or_default();
                    p.dlc = Some(list);
                    p.error = res.err().map(|e| trf!("DLC не переключено: {e}", "DLC not switched: {e}"));
                });
            });
        }
        Cmd::SetLanguage { appid, name, acf, lang } => {
            let c = client.clone();
            let (st2, ctx2) = (st.clone(), ctx.clone());
            start_op(appid, trf!("Смена языка: {name}", "Language change: {name}"), tr!("язык изменен.", "language changed."), st, ctx, move |abort| async move {
                switch_language(&c, appid, &acf, &lang, &abort, &st2, &ctx2).await?;
                // Files are in place - remove the yellow "Update" from the card.
                upd(&st2, &ctx2, |s| {
                    if let Some(u) = s.updates.as_mut() {
                        u.retain(|a| *a != appid);
                    }
                });
                Ok(())
            });
        }
        Cmd::UpdateDirect { appid, name, acf, beta } => {
            // Update without Steam: Aurelia downloads new versions of already installed depots.
            // Its appmanifest is written from scratch (loses language, SharedDepots, InstallScripts...), so
            // afterwards we restore the original file and write only the new versions and build into it.
            let c = client.clone();
            let (st2, ctx2) = (st.clone(), ctx.clone());
            start_op(appid, trf!("Обновление: {name}", "Update: {name}"), tr!("обновлена.", "updated."), st, ctx, move |abort| async move {
                let original = std::fs::read_to_string(&acf).map_err(|e| trf!("Не удалось прочитать файл игры: {e}", "Could not read the game file: {e}"))?;
                let state = Arc::new(RwLock::new(DownloadState::default()));
                if let Ok(mut s) = state.write() {
                    s.abort_signal = abort.clone();
                }
                let rx = c
                    .update_game(appid, state)
                    .await
                    .map_err(|e| trf!("Не удалось начать обновление: {e:#}", "Could not start the update: {e:#}"))?;
                let res = drain_progress(rx, &abort, tr!("Обновление отменено.", "Update cancelled."), &st2, &ctx2).await;
                merge_after_update(&acf, &original, res.is_ok())?;
                res?;
                if let Some(b) = &beta {
                    crate::local::set_mounted_config(&acf, "BetaKey", b)?;
                }
                upd(&st2, &ctx2, |s| {
                    if let Some(u) = s.updates.as_mut() {
                        u.retain(|a| *a != appid);
                    }
                });
                Ok(())
            });
        }
        Cmd::Verify { appid, name } => {
            let c = client.clone();
            let (st2, ctx2) = (st.clone(), ctx.clone());
            start_op(appid, trf!("Проверка файлов: {name}", "Verifying files: {name}"), tr!("файлы в порядке.", "the files are fine."), st, ctx, move |abort| async move {
                let state = Arc::new(RwLock::new(DownloadState::default()));
                if let Ok(mut s) = state.write() {
                    s.abort_signal = abort.clone();
                }
                let rx = c
                    .verify_game(appid, state)
                    .await
                    .map_err(|e| trf!("Не удалось начать проверку: {e:#}", "Could not start the verification: {e:#}"))?;
                drain_progress(rx, &abort, tr!("Проверка отменена.", "Verification cancelled."), &st2, &ctx2).await
            });
        }
        Cmd::Move { appid, name, dest } => {
            let c = client.clone();
            let (st2, ctx2) = (st.clone(), ctx.clone());
            start_op(appid, trf!("Перенос: {name}", "Moving: {name}"), tr!("перенесена.", "moved."), st, ctx, move |abort| async move {
                let rx = c
                    .move_install(appid, dest)
                    .await
                    .map_err(|e| trf!("Не удалось начать перенос: {e:#}", "Could not start moving: {e:#}"))?;
                drain_progress(rx, &abort, tr!("Перенос отменен (исходные файлы на месте).", "Moving cancelled (the original files are in place)."), &st2, &ctx2).await
            });
        }
        Cmd::PinRefresh { appid, path, branch, launch } => {
            let (c, st, ctx) = (client.clone(), st.clone(), ctx.clone());
            tokio::spawn(async move {
                let result = async {
                    let (latest, buildid) = c
                        .latest_depot_versions(appid, &branch)
                        .await
                        .map_err(|e| trf!("Не удалось узнать последнюю версию игры: {e:#}", "Could not get the latest game version: {e:#}"))?;
                    // Only depots the game actually has installed.
                    let installed = crate::local::installed_depots(&path);
                    let target: std::collections::BTreeMap<u32, u64> = installed
                        .keys()
                        .filter_map(|d| latest.get(d).map(|m| (*d, *m)))
                        .collect();
                    if target.is_empty() {
                        return Err(tr!("Steam не сообщил версии хранилищ этой игры.", "Steam did not report the depot versions of this game.").to_string());
                    }
                    crate::local::write_depot_versions(&path, &target, buildid.as_deref(), 4)
                }
                .await;
                upd(&st, &ctx, |s| s.pin_done = Some(PinDone { appid, launch, result }));
            });
        }
        Cmd::Import { appid, name, library } => {
            let c = client.clone();
            let (st2, ctx2) = (st.clone(), ctx.clone());
            start_op(appid, trf!("Добавление: {name}", "Adding: {name}"), tr!("добавлена и проверена.", "added and verified."), st, ctx, move |abort| async move {
                c.import_install(appid, library, DepotPlatform::Windows)
                    .await
                    .map_err(|e| trf!("Не удалось добавить игру: {e:#}", "Could not add the game: {e:#}"))?;
                // Files may be of an old version - verification downloads the differences.
                let state = Arc::new(RwLock::new(DownloadState::default()));
                if let Ok(mut s) = state.write() {
                    s.abort_signal = abort.clone();
                }
                let rx = c
                    .verify_game(appid, state)
                    .await
                    .map_err(|e| trf!("Игра добавлена, но проверка не началась: {e:#}", "The game is added, but the verification did not start: {e:#}"))?;
                drain_progress(rx, &abort, tr!("Проверка отменена (игра добавлена).", "Verification cancelled (the game is added)."), &st2, &ctx2).await
            });
        }
        Cmd::AureliaConfig { limit_kbit, proxy } => {
            // Right away for running downloads; config.json - so Aurelia does not bring back the old value.
            aurelia::steam_client::set_download_limit_now(limit_kbit);
            tokio::spawn(async move {
                if let Ok(mut cfg) = aurelia::core::config::LauncherConfig::load().await {
                    if cfg.download_limit_kbit != limit_kbit || cfg.proxy.url != proxy {
                        cfg.download_limit_kbit = limit_kbit;
                        cfg.proxy.url = proxy;
                        let _ = cfg.save().await;
                    }
                }
                aurelia::steam_client::set_download_limit_now(limit_kbit);
            });
        }
        Cmd::DirectSpec(appid) => {
            let (c, st, ctx) = (client.clone(), st.clone(), ctx.clone());
            tokio::spawn(async move {
                let r = c.fetch_launch_options(appid).await.map(|o| crate::direct::pick(&o)).map_err(es);
                upd(&st, &ctx, |s| s.direct_spec = Some((appid, r)));
            });
        }
        Cmd::CloudSync { appid, remote, dir, direction, take } => {
            use aurelia::library::cloud_sync::{CloudPathResolver, ConflictPolicy, SyncDirection};
            let (c, st, ctx) = (client.clone(), st.clone(), ctx.clone());
            upd(&st, &ctx, |s| {
                let p = s.props.entry(appid).or_default();
                p.cloud_busy = true;
                p.cloud_sync = None;
            });
            tokio::spawn(async move {
                let res = async {
                    let cloud = c.cloud_client().map_err(es)?;
                    // Aurelia 0.2.5: cloud rules (UFS) with root overrides for Windows and the SteamID.
                    let ufs = c.fetch_ufs_config(appid).await.unwrap_or_default();
                    let resolver = CloudPathResolver::new(remote, Some(dir))
                        .with_root_overrides(&ufs.root_overrides, "windows")
                        .with_steam_id(cloud.steam_id());
                    let specs = ufs.savefiles;
                    let direction = match direction {
                        0 => SyncDirection::Down,
                        1 => SyncDirection::Up,
                        _ => SyncDirection::Both,
                    };
                    let policy = match take {
                        Some(true) => ConflictPolicy::TakeCloud,
                        Some(false) => ConflictPolicy::TakeLocal,
                        None => ConflictPolicy::Detect,
                    };
                    let o = cloud.sync(appid, &resolver, &specs, direction, policy).await.map_err(es)?;
                    Ok::<_, String>(CloudOutcome {
                        downloaded: o.downloaded.len(),
                        uploaded: o.uploaded.len(),
                        conflicts: o.conflicts.iter().map(|x| (x.filename.clone(), x.local_size, x.cloud_size)).collect(),
                        skipped: o.skipped.len(),
                        failed: o.failed.iter().map(|f| format!("{}: {}", f.filename, f.error)).collect(),
                    })
                }
                .await;
                // Refresh the cloud file list after uploading.
                let files = c.cloud_client().ok();
                let list = match files {
                    Some(cl) => Some(cl.get_file_list(appid).await),
                    None => None,
                };
                upd(&st, &ctx, |s| {
                    let p = s.props.entry(appid).or_default();
                    p.cloud_busy = false;
                    p.cloud_sync = Some(res);
                    if let Some(Ok(l)) = list {
                        p.cloud = Some(Ok(l
                            .into_iter()
                            .map(|f| CloudFile { name: f.filename, size: f.size, time: f.timestamp as u64 })
                            .collect()));
                    }
                });
            });
        }
        Cmd::Downgrade { appid, name, library, overrides } => {
            let c = client.clone();
            let (st2, ctx2) = (st.clone(), ctx.clone());
            start_op(appid, trf!("Откат версии: {name}", "Version rollback: {name}"), tr!("откачена и закреплена.", "rolled back and pinned."), st, ctx, move |abort| async move {
                let state = Arc::new(RwLock::new(DownloadState::default()));
                if let Ok(mut s) = state.write() {
                    s.abort_signal = abort.clone();
                }
                let opts = InstallOptions {
                    library_override: Some(library.to_string_lossy().into_owned()),
                    manifest_overrides: Some(overrides),
                    ..Default::default()
                };
                let rx = c
                    .install_game(appid, DepotPlatform::Windows, opts, state)
                    .await
                    .map_err(|e| trf!("Не удалось начать откат: {e:#}", "Could not start the rollback: {e:#}"))?;
                drain_progress(rx, &abort, tr!("Откат отменен.", "Rollback cancelled."), &st2, &ctx2).await?;
                upd(&st2, &ctx2, |s| s.pin_after = Some(appid));
                Ok(())
            });
        }
        Cmd::Uninstall { appid, name } => {
            let c = client.clone();
            start_op(appid, trf!("Удаление: {name}", "Uninstalling: {name}"), tr!("удалена.", "uninstalled."), st, ctx, move |_| async move {
                c.uninstall_game(appid, false).await.map_err(|e| trf!("Не удалось удалить: {e:#}", "Could not uninstall: {e:#}"))
            });
        }
        Cmd::UninstallShared { appid, name, dir, others } => {
            let c = client.clone();
            start_op(
                appid,
                trf!("Удаление: {name}", "Uninstalling: {name}"),
                tr!("удалена (файлы других игр из общей папки не тронуты).", "uninstalled (files of other games in the shared folder are untouched)."),
                st,
                ctx,
                move |_| async move {
                    uninstall_shared(&c, appid, &dir, &others)
                        .await
                        .map(|_| ())
                        .map_err(|e| trf!("Не удалось удалить (ничего не удалено): {e}", "Could not uninstall (nothing deleted): {e}"))
                },
            );
        }
        Cmd::LoadEmoticons => {
            let (client, st, ctx) = (client.clone(), st.clone(), ctx.clone());
            spawn_session(async move {
                let lists = client.fetch_emoticons_and_stickers().await.unwrap_or_default();
                upd(&st, &ctx, |s| s.emoticons = Some(lists));
            });
        }
        Cmd::ActivateKey(key) => {
            let (client, st, ctx) = (client.clone(), st.clone(), ctx.clone());
            upd(&st, &ctx, |s| {
                s.key_busy = true;
                s.key_result = None;
            });
            spawn_session(async move {
                let r = crate::keys::activate(&client, &key).await;
                upd(&st, &ctx, |s| {
                    s.key_busy = false;
                    s.key_result = Some(r);
                });
            });
        }
        Cmd::NewsFeed(games) => {
            let (st, ctx) = (st.clone(), ctx.clone());
            upd(&st, &ctx, |s| s.news_busy = true);
            spawn_session(async move {
                let r = crate::newsfeed::load(games).await;
                upd(&st, &ctx, |s| {
                    s.news_busy = false;
                    s.news_feed = Some(r);
                });
            });
        }
        Cmd::NotifIcons(invs) => {
            let (client, st, ctx) = (client.clone(), st.clone(), ctx.clone());
            spawn_session(async move {
                let Ok(web) = crate::webapi::web_session(&client).await else { return };
                let mut icons = HashMap::new();
                for (app, cx) in invs {
                    if let Ok(items) = aurelia::steam_client::inventory_via_lang(&web, app, cx, "english").await {
                        for i in items {
                            if let Some(u) = i.icon_url.filter(|u| !u.is_empty()) {
                                let url = if u.starts_with("http") { u } else { format!("{}{u}", aurelia::web::web_session::ECON_IMAGE_BASE) };
                                icons.insert(i.asset_id, format!("{url}/96fx96f"));
                            }
                        }
                    }
                }
                upd(&st, &ctx, |s| s.notif_icons.extend(icons));
            });
        }
        Cmd::SteamNotifs | Cmd::SteamNotifsRead => {
            let mark = matches!(cmd, Cmd::SteamNotifsRead);
            let (client, st, ctx) = (client.clone(), st.clone(), ctx.clone());
            spawn_session(async move {
                if mark {
                    if let Err(e) = crate::steamnotif::mark_all_read(&client).await {
                        upd(&st, &ctx, |s| s.steam_notifs = Some(Err(trf!("Не отмечены: {e}", "Not marked: {e}"))));
                    }
                }
                let r = crate::steamnotif::load(&client).await;
                // A network error is not shown over an already loaded list.
                upd(&st, &ctx, |s| {
                    if r.is_ok() || !matches!(s.steam_notifs, Some(Ok(_))) {
                        s.steam_notifs = Some(r);
                    }
                });
            });
        }
        Cmd::Badges => {
            let (client, st, ctx) = (client.clone(), st.clone(), ctx.clone());
            spawn_session(async move {
                let Some(me) = client.my_steam_id() else { return };
                let r = crate::badges::load(&client, me).await;
                upd(&st, &ctx, |s| s.badges = Some(r));
            });
        }
        Cmd::CraftBadge { appid, foil } => {
            let (client, st, ctx) = (client.clone(), st.clone(), ctx.clone());
            spawn_session(async move {
                let Some(me) = client.my_steam_id() else { return };
                let r = crate::badges::craft(&client, me, appid, foil).await;
                let msg = match r {
                    Ok(()) => tr!("Значок создан.", "Badge crafted.").to_string(),
                    Err(e) => e,
                };
                let fresh = crate::badges::load(&client, me).await;
                upd(&st, &ctx, |s| {
                    s.badges_msg = Some(msg);
                    s.badges = Some(fresh);
                });
            });
        }
        Cmd::PointsBalance => {
            let (client, st, ctx) = (client.clone(), st.clone(), ctx.clone());
            spawn_session(async move {
                let Some(me) = client.my_steam_id() else { return };
                if let Ok(b) = crate::points::balance(&client, me).await {
                    upd(&st, &ctx, |s| s.points_balance = Some(b));
                }
            });
        }
        Cmd::PointsQuery { class, search, cursor } => {
            let (client, st, ctx) = (client.clone(), st.clone(), ctx.clone());
            upd(&st, &ctx, |s| s.points_busy = true);
            spawn_session(async move {
                let more = cursor.is_some();
                let r = crate::points::query(&client, class, &search, cursor.as_deref()).await;
                upd(&st, &ctx, |s| {
                    s.points_busy = false;
                    match (r, s.points_page.take()) {
                        // "Show more" - append to the same page.
                        (Ok(mut p), Some(Ok(mut old))) if more && old.class == p.class && old.search == p.search => {
                            old.items.append(&mut p.items);
                            old.cursor = p.cursor;
                            s.points_page = Some(Ok(old));
                        }
                        (r, _) => s.points_page = Some(r),
                    }
                });
            });
        }
        Cmd::PointsOwned => {
            let (client, st, ctx) = (client.clone(), st.clone(), ctx.clone());
            spawn_session(async move {
                if let Ok(mut o) = crate::points::owned(&client).await {
                    upd(&st, &ctx, |s| {
                        // Steam may not return items bought in this session yet - do not lose them.
                        if let Some(prev) = &s.points_owned {
                            o.profile.extend(prev.profile.iter().copied());
                            o.names.extend(prev.names.iter().cloned());
                        }
                        s.points_owned = Some(o);
                    });
                }
            });
        }
        Cmd::PointsRedeem(item) => {
            let (client, st, ctx) = (client.clone(), st.clone(), ctx.clone());
            spawn_session(async move {
                let r = crate::points::redeem(&client, item.defid).await;
                let bal = match client.my_steam_id() {
                    Some(me) => crate::points::balance(&client, me).await.ok(),
                    None => None,
                };
                let ok = r.is_ok();
                upd(&st, &ctx, |s| {
                    s.points_msg = Some(match r {
                        Ok(()) => trf!("Куплено: {}.", "Purchased: {}.", item.title),
                        Err(e) => e,
                    });
                    if bal.is_some() {
                        s.points_balance = bal;
                    }
                    if ok {
                        s.points_owned.get_or_insert_with(Default::default).add(&item);
                        // Straight into the chat panel: the Steam inventory updates with a delay.
                        match item.class {
                            12 => {
                                let e = crate::effects::from_item_name(&item.name);
                                s.bought_effects.push(e.clone());
                                let list = s.chat_effects.get_or_insert_with(Vec::new);
                                if !list.contains(&e) {
                                    list.push(e);
                                    list.sort();
                                }
                            }
                            11 | 4 => {
                                let (emo, stick) = s.emoticons.get_or_insert_with(Default::default);
                                let (list, name) = if item.class == 11 {
                                    (stick, item.name.clone())
                                } else {
                                    (emo, item.name.trim_matches(':').to_string())
                                };
                                if !list.contains(&name) {
                                    list.insert(0, name);
                                }
                            }
                            _ => {}
                        }
                    }
                });
            });
        }
        Cmd::Family => {
            let (client, st, ctx) = (client.clone(), st.clone(), ctx.clone());
            spawn_session(async move {
                let Some(me) = client.my_steam_id() else { return };
                let r = crate::family::load(&client, me).await;
                upd(&st, &ctx, |s| s.family_group = Some(r));
            });
        }
        Cmd::Activity(friends) => {
            let (client, st, ctx) = (client.clone(), st.clone(), ctx.clone());
            upd(&st, &ctx, |s| s.activity_busy = true);
            spawn_session(async move {
                let r = crate::activity::load(&client, friends).await;
                upd(&st, &ctx, |s| {
                    s.activity_busy = false;
                    s.activity = Some(r);
                });
            });
        }
        Cmd::MyRecent => {
            let (client, st, ctx) = (client.clone(), st.clone(), ctx.clone());
            spawn_session(async move {
                let Some(me) = client.my_steam_id() else { return };
                let r = crate::webapi::get(&client, "IPlayerService/GetRecentlyPlayedGames/v1", &[("steamid", me.to_string())])
                    .await
                    .map(|v| crate::activity::parse_recent(&v));
                upd(&st, &ctx, |s| s.my_recent = Some(r));
            });
        }
        Cmd::GroupInfo(url) => {
            let (st, ctx) = (st.clone(), ctx.clone());
            spawn_session(async move {
                let r = crate::grouppage::load(&url).await;
                upd(&st, &ctx, |s| {
                    s.group_msg = None;
                    s.groups_info.insert(url, r);
                });
            });
        }
        Cmd::GroupMembership { url, info, join } => {
            let (client, st, ctx) = (client.clone(), st.clone(), ctx.clone());
            spawn_session(async move {
                let r = crate::grouppage::membership(&client, &info, join).await;
                // Reread own profile (group list) and the group itself.
                let me = client.my_steam_id();
                let fresh = match me {
                    Some(me) => Some((me, crate::profile_extra::load(&client, me).await)),
                    None => None,
                };
                let g = crate::grouppage::load(&url).await;
                upd(&st, &ctx, |s| {
                    s.group_msg = Some(match r {
                        Ok(()) if join => tr!("Запрос отправлен. Если группа закрытая, нужно дождаться одобрения.", "Request sent. If the group is private, wait for approval.").into(),
                        Ok(()) => tr!("Вы вышли из группы.", "You left the group.").into(),
                        Err(e) => trf!("Не удалось: {e}", "Failed: {e}"),
                    });
                    if let Some((me, x)) = fresh {
                        s.profile_extra.insert(me, x);
                    }
                    s.groups_info.insert(url, g);
                });
            });
        }
        Cmd::ProfileExtra(id) => {
            let (client, st, ctx) = (client.clone(), st.clone(), ctx.clone());
            spawn_session(async move {
                let r = crate::profile_extra::load(&client, id).await;
                upd(&st, &ctx, |s| {
                    s.profile_extra.insert(id, r);
                });
            });
        }
        Cmd::DeleteComment { id, gid } => {
            let (client, st, ctx) = (client.clone(), st.clone(), ctx.clone());
            upd(&st, &ctx, |s| s.comment_msg = Some(tr!("Удаляю...", "Deleting...").into()));
            spawn_session(async move {
                let r = crate::profile_extra::delete_comment(&client, id, &gid).await;
                let fresh = crate::profile_extra::load(&client, id).await;
                upd(&st, &ctx, |s| {
                    s.comment_msg = Some(match r {
                        Ok(()) => tr!("Комментарий удален.", "Comment deleted.").into(),
                        Err(e) => e,
                    });
                    s.profile_extra.insert(id, fresh);
                });
            });
        }
        Cmd::MoreComments { id, start } => {
            let (client, st, ctx) = (client.clone(), st.clone(), ctx.clone());
            spawn_session(async move {
                if let Ok((total, more)) = crate::profile_extra::load_comments(&client, id, start).await {
                    upd(&st, &ctx, |s| {
                        // Comment page (10 each), as in Steam.
                        if let Some(Ok(x)) = s.profile_extra.get_mut(&id) {
                            x.comments_total = total;
                            x.comments = more;
                        }
                    });
                }
            });
        }
        Cmd::PostComment { id, text } => {
            let (client, st, ctx) = (client.clone(), st.clone(), ctx.clone());
            upd(&st, &ctx, |s| s.comment_msg = Some(tr!("Отправляю...", "Sending...").into()));
            spawn_session(async move {
                let r = crate::profile_extra::post_comment(&client, id, &text).await;
                let fresh = crate::profile_extra::load(&client, id).await;
                upd(&st, &ctx, |s| {
                    s.comment_msg = Some(match r {
                        Ok(()) => tr!("Комментарий опубликован.", "Comment posted.").into(),
                        Err(e) => e,
                    });
                    s.profile_extra.insert(id, fresh);
                });
            });
        }
        Cmd::ShotComments { owner, id } => {
            let (client, st, ctx) = (client.clone(), st.clone(), ctx.clone());
            spawn_session(async move {
                let r = crate::shotcomments::load(&client, owner, id).await;
                upd(&st, &ctx, |s| {
                    s.shot_comments.insert(id, r);
                });
            });
        }
        Cmd::ShotComment { owner, id, text } => {
            let (client, st, ctx) = (client.clone(), st.clone(), ctx.clone());
            upd(&st, &ctx, |s| s.shot_msg = Some(tr!("Отправляю...", "Sending...").into()));
            spawn_session(async move {
                let r = crate::shotcomments::post(&client, owner, id, &text).await;
                let fresh = crate::shotcomments::load(&client, owner, id).await;
                upd(&st, &ctx, |s| {
                    s.shot_msg = Some(match r {
                        Ok(()) => tr!("Комментарий опубликован.", "Comment posted.").into(),
                        Err(e) => e,
                    });
                    s.shot_comments.insert(id, fresh);
                });
            });
        }
        Cmd::ShotVote { id, up } => {
            let (client, st, ctx) = (client.clone(), st.clone(), ctx.clone());
            spawn_session(async move {
                let r = crate::shotcomments::vote(&client, id, up).await;
                upd(&st, &ctx, |s| {
                    s.shot_msg = Some(match r {
                        Ok(()) => tr!("Оценка поставлена.", "Vote sent.").into(),
                        Err(e) => e,
                    });
                });
            });
        }
        Cmd::CommTopics { appid, page } => {
            let (client, st, ctx) = (client.clone(), st.clone(), ctx.clone());
            upd(&st, &ctx, |s| s.comm_topics = None);
            spawn_session(async move {
                let r = crate::community::load_topics(&client, appid, page).await.map(|v| (appid, page, v));
                upd(&st, &ctx, |s| s.comm_topics = Some(r));
            });
        }
        Cmd::CommTopic(url) => {
            let (client, st, ctx) = (client.clone(), st.clone(), ctx.clone());
            upd(&st, &ctx, |s| s.comm_msg = Some(tr!("Открываю тему...", "Opening the topic...").into()));
            spawn_session(async move {
                let r = crate::community::load_topic(&client, &url).await;
                upd(&st, &ctx, |s| {
                    s.comm_msg = None;
                    s.comm_topic = Some(r);
                });
            });
        }
        Cmd::CommReply { topic, text } => {
            let (client, st, ctx) = (client.clone(), st.clone(), ctx.clone());
            upd(&st, &ctx, |s| s.comm_msg = Some(tr!("Отправляю ответ...", "Sending the reply...").into()));
            spawn_session(async move {
                let r = crate::community::reply(&client, &topic, &text).await;
                let fresh = crate::community::load_topic(&client, &topic.url).await;
                upd(&st, &ctx, |s| {
                    s.comm_msg = Some(match r {
                        Ok(()) => tr!("Ответ опубликован.", "Reply posted.").into(),
                        Err(e) => e,
                    });
                    s.comm_topic = Some(fresh);
                });
            });
        }
        Cmd::CommGuide(id) => {
            let (client, st, ctx) = (client.clone(), st.clone(), ctx.clone());
            upd(&st, &ctx, |s| s.comm_msg = Some(tr!("Открываю руководство...", "Opening the guide...").into()));
            spawn_session(async move {
                let r = crate::community::load_guide(&client, id).await;
                upd(&st, &ctx, |s| {
                    s.comm_msg = None;
                    s.comm_guide = Some(r);
                });
            });
        }
        Cmd::CommGuides { appid, search } => {
            let (client, st, ctx) = (client.clone(), st.clone(), ctx.clone());
            upd(&st, &ctx, |s| s.comm_guides = None);
            spawn_session(async move {
                let r = crate::community::load_guides(&client, appid, &search).await.map(|v| (appid, v));
                upd(&st, &ctx, |s| s.comm_guides = Some(r));
            });
        }
        Cmd::Feed { start } => {
            let (client, st, ctx) = (client.clone(), st.clone(), ctx.clone());
            upd(&st, &ctx, |s| s.feed_busy = true);
            spawn_session(async move {
                let Some(me) = client.my_steam_id() else { return };
                let r = crate::feed::load_days(&client, me, start, 20).await;
                upd(&st, &ctx, |s| {
                    s.feed_busy = false;
                    match (r, s.feed.take()) {
                        // "Show earlier" - append.
                        (Ok(mut p), Some(Ok(mut old))) if start > 0 => {
                            old.entries.append(&mut p.entries);
                            old.next = p.next;
                            s.feed = Some(Ok(old));
                        }
                        (r, _) => s.feed = Some(r),
                    }
                });
            });
        }
        Cmd::OlderChat(friend) => {
            let before = st.lock().unwrap().chats.get(&friend).and_then(|c| c.oldest);
            let Some(before) = before else { return };
            let (client, st, ctx) = (client.clone(), st.clone(), ctx.clone());
            upd(&st, &ctx, |s| Arc::make_mut(&mut s.chats).entry(friend).or_default().loading_older = true);
            spawn_session(async move {
                let r = client.chat_history(friend, 50, Some(before)).await;
                upd(&st, &ctx, |s| {
                    let conv = Arc::make_mut(&mut s.chats).entry(friend).or_default();
                    conv.loading_older = false;
                    match r {
                        Ok((msgs, more)) => {
                            if let Some(o) = msgs.iter().map(|m| (m.timestamp, m.ordinal)).min() {
                                conv.oldest = Some(o);
                            }
                            conv.more_history = more && !msgs.is_empty();
                            // They come newest to oldest - prepend to the conversation, in order.
                            let older: Vec<ChatLine> = msgs
                                .into_iter()
                                .rev()
                                .map(|m| ChatLine { from_self: m.from_self, text: m.message, time: m.timestamp as u64, ordinal: m.ordinal, exact: true, reactions: m.reactions })
                                .collect();
                            conv.lines.splice(0..0, older);
                        }
                        Err(e) => conv.error = Some(trf!("Не удалось загрузить: {e:#}", "Could not load: {e:#}")),
                    }
                });
            });
        }
        Cmd::Sessions => {
            let (client, st, ctx) = (client.clone(), st.clone(), ctx.clone());
            spawn_session(async move {
                let r = crate::sessions::load(&client).await;
                upd(&st, &ctx, |s| s.sessions = Some(r));
            });
        }
        Cmd::Licenses => {
            let (client, st, ctx) = (client.clone(), st.clone(), ctx.clone());
            spawn_session(async move {
                if let Ok(v) = crate::refund::load(&client).await {
                    upd(&st, &ctx, |s| s.licenses = Some(v));
                }
            });
        }
        cmd @ (Cmd::ProfileEditLoad
        | Cmd::ProfileEditSave(_)
        | Cmd::PrivacySave(_)
        | Cmd::AvatarUpload(_)
        | Cmd::SetNickname(..)
        | Cmd::ChatReaction { .. }
        | Cmd::CreateChatGroup { .. }
        | Cmd::InviteToChatGroup { .. }
        | Cmd::ReviewLoad(_)
        | Cmd::ReviewSave { .. }
        | Cmd::CreateTopic { .. }
        | Cmd::AwardConfig
        | Cmd::Award { .. }
        | Cmd::WishlistReorder(_)
        | Cmd::CartLoad
        | Cmd::CartAdd(_)
        | Cmd::CartRemove(_)
        | Cmd::AppPackages(_)
        | Cmd::Curators(_)
        | Cmd::FamilyInvite { .. }
        | Cmd::FamilyCancel { .. }
        | Cmd::WsCollections { .. }
        | Cmd::WsCollectionItems(_)
        | Cmd::WsVote { .. }
        | Cmd::WsFavorite { .. }) => {
            let (client, st, ctx) = (client.clone(), st.clone(), ctx.clone());
            spawn_session(async move { second_plan(&client, cmd, &st, &ctx).await });
        }
        Cmd::DownloadRegions => {
            let (st, ctx) = (st.clone(), ctx.clone());
            tokio::spawn(async move {
                let r = crate::actions::download_regions().await;
                upd(&st, &ctx, |s| s.download_regions = Some(r));
            });
        }
        Cmd::Coplay => {
            let (client, st, ctx) = (client.clone(), st.clone(), ctx.clone());
            spawn_session(async move {
                let r = match client.my_steam_id() {
                    Some(me) => crate::coplay::load(&client, me).await,
                    None => Err(tr!("нет входа", "not logged in").into()),
                };
                upd(&st, &ctx, |s| s.coplay = Some(r));
            });
        }
        Cmd::WalletHistory => {
            let (client, st, ctx) = (client.clone(), st.clone(), ctx.clone());
            spawn_session(async move {
                let r = crate::wallet::load(&client).await;
                upd(&st, &ctx, |s| s.wallet_history = Some(r));
            });
        }
        Cmd::ChatGroups => {
            let (client, st, ctx) = (client.clone(), st.clone(), ctx.clone());
            spawn_session(async move {
                let r = client.my_chat_groups().await.map_err(|e| format!("{e:#}"));
                // Like an open chat window in Steam: otherwise channel history may not be returned.
                if let Ok(g) = &r {
                    let _ = client.activate_chat_groups(&g.iter().map(|x| x.group_id).collect::<Vec<_>>()).await;
                }
                upd(&st, &ctx, |s| s.chat_groups = Some(r));
            });
        }
        Cmd::GroupMessages { group, chat } => {
            let (client, st, ctx) = (client.clone(), st.clone(), ctx.clone());
            spawn_session(async move {
                let r = client.group_messages(group, chat, 60).await.map_err(|e| format!("{e:#}"));
                upd(&st, &ctx, |s| {
                    // A failed refresh does not wipe already shown messages.
                    if r.is_ok() || !matches!(s.group_messages.get(&(group, chat)), Some(Ok(_))) {
                        s.group_messages.insert((group, chat), r);
                    }
                });
            });
        }
        Cmd::GroupSend { group, chat, text } => {
            let (client, st, ctx) = (client.clone(), st.clone(), ctx.clone());
            spawn_session(async move {
                let r = client.send_group_message(group, chat, &text).await;
                let fresh = client.group_messages(group, chat, 60).await.map_err(|e| format!("{e:#}"));
                upd(&st, &ctx, |s| {
                    s.group_error = r.err().map(|e| trf!("Не отправлено: {e:#}", "Not sent: {e:#}"));
                    if fresh.is_ok() {
                        s.group_messages.insert((group, chat), fresh);
                    }
                });
            });
        }
        Cmd::AccountInfo => {
            let (client, st, ctx) = (client.clone(), st.clone(), ctx.clone());
            spawn_session(async move {
                let d = client.get_account_data().await;
                upd(&st, &ctx, |s| s.account_data = Some(d));
            });
        }
        Cmd::CloudShots { user, appid, page } => {
            let (client, st, ctx) = (client.clone(), st.clone(), ctx.clone());
            upd(&st, &ctx, |s| s.cloud_shots_busy = true);
            spawn_session(async move {
                let r = if user == 0 {
                    client.my_cloud_screenshots(appid, page, 50).await
                } else {
                    client.user_screenshots(user, appid, page, 50).await
                }
                .map_err(|e| {
                    let t = es(e);
                    // A private Steam profile answers InvalidParam.
                    if user != 0 && t.contains("InvalidParam") { tr!("профиль или скриншоты скрыты настройками приватности", "the profile or screenshots are hidden by privacy settings").to_string() } else { t }
                });
                upd(&st, &ctx, |s| {
                    s.cloud_shots_busy = false;
                    let same_user = s.cloud_shots_user == user;
                    s.cloud_shots_user = user;
                    s.cloud_shots = Some(match r {
                        // Next page of the same filter - append.
                        Ok((total, v)) => match s.cloud_shots.take() {
                            Some(Ok((a, _, mut old))) if a == appid && page > 1 && same_user => {
                                old.extend(v);
                                Ok((appid, total, old))
                            }
                            _ => Ok((appid, total, v)),
                        },
                        Err(e) => Err(e),
                    });
                });
            });
        }
        Cmd::LoadChatEffects => {
            // Room effects: Steam does not tell a separate session which are bought - they are in the
            // inventory (753/6), like stickers: type "Winter Sale Event 2019 Chat Effect", name "Confetti".
            let (client, st, ctx) = (client.clone(), st.clone(), ctx.clone());
            spawn_session(async move {
                let Ok(web) = crate::webapi::web_session(&client).await else { return };
                let Ok(items) = aurelia::steam_client::inventory_via_lang(&web, 753, 6, "english").await else {
                    return;
                };
                let mut v: Vec<String> = items
                    .into_iter()
                    .filter(|i| i.item_type.to_lowercase().contains("chat effect"))
                    .map(|i| crate::effects::from_item_name(&i.name))
                    .collect();
                v.sort();
                v.dedup();
                upd(&st, &ctx, |s| {
                    for e in &s.bought_effects {
                        if !v.contains(e) {
                            v.push(e.clone());
                        }
                    }
                    v.sort();
                    s.chat_effects = Some(v);
                });
            });
        }
        Cmd::SendImage { to, path } => {
            let (client, st, ctx) = (client.clone(), st.clone(), ctx.clone());
            spawn_session(async move {
                let set_status = |v: Option<String>, err: Option<String>| {
                    upd(&st, &ctx, |s| {
                        let c = Arc::make_mut(&mut s.chats).entry(to).or_default();
                        c.status = v;
                        if err.is_some() {
                            c.error = err;
                        }
                    })
                };
                set_status(Some(tr!("Отправляю картинку...", "Sending the image...").into()), None);
                let prepared = tokio::task::spawn_blocking(move || prepare_image(&path))
                    .await
                    .unwrap_or_else(|e| Err(e.to_string()));
                let (bytes, ext, w, h) = match prepared {
                    Ok(p) => p,
                    Err(e) => return set_status(None, Some(e)),
                };
                let url = match client.upload_chat_image(to, bytes, ext, w, h).await {
                    Ok(u) => u,
                    Err(e) => return set_status(None, Some(trf!("Картинка не отправлена: {e:#}", "Image not sent: {e:#}"))),
                };
                // It seems that on upload confirmation Steam posts the image to the chat itself.
                // Check: if it is not in the history, send the message ourselves.
                tokio::time::sleep(Duration::from_millis(1500)).await;
                let posted = client
                    .recent_chat_messages(to, 5)
                    .await
                    .map(|m| m.iter().any(|m| m.message.contains(&url)))
                    .unwrap_or(false);
                if !posted {
                    let bb = format!("[img src={url} width={w} height={h}][url={url}]{url}[/url][/img]");
                    if let Err(e) = client.send_chat_message(to, bb).await {
                        return set_status(None, Some(trf!("Картинка загружена, но не отправлена: {e:#}", "Image uploaded but not sent: {e:#}")));
                    }
                }
                reload_history(&client, to, &st, &ctx).await;
                set_status(None, None);
            });
        }
        Cmd::Logout => {
            PROBE_GEN.fetch_add(1, Ordering::Relaxed); // the previous login's connection check is no longer needed
            SESSION_GEN.fetch_add(1, Ordering::Relaxed); // tasks of the previous login no longer write anything
            crate::launchopts::set_account(0);
            crate::webapi::clear_token();
            let _ = client.logout().await;
            upd(st, ctx, |s| {
                *s = NetState {
                    need_login: true,
                    message: tr!("Вы вышли из аккаунта.", "You logged out.").into(),
                    ..Default::default()
                };
            });
        }
    }
}

async fn logged_in(client: &mut SteamClient, session: SessionState, st: &Shared, ctx: &egui::Context) {
    // The login (AccountName), not the display name: accounts.rs finds the account in Steam's
    // loginusers.vdf by it. The display name is only a fallback when the login is unknown.
    let name = session
        .account_name
        .clone()
        .or(session.persona_name.clone())
        .unwrap_or_else(|| "?".into());
    let roster = Arc::new(RwLock::new(Roster::new()));
    let my_id = client.my_steam_id();
    // Launch options and shortcuts go to this account's Steam folder (several accounts on one PC).
    crate::launchopts::set_account(my_id.unwrap_or(0));
    crate::webapi::clear_token();
    // New login: tasks of the previous login no longer write anything (spawn_session, chat listener).
    SESSION_GEN.fetch_add(1, Ordering::Relaxed);
    upd(st, ctx, |s| {
        s.account = Some(name);
        s.need_login = false;
        s.offline = false;
        s.roster = roster.clone();
        s.my_id = my_id;
        Arc::make_mut(&mut s.chats).clear();
    });
    let secure = match secure_session().await {
        Ok(v) => v,
        Err(e) => {
            crate::errlog::write(tr!("вход", "login"), &trf!("не удалось зашифровать сохраненный вход: {e:#}", "could not encrypt the saved login: {e:#}"));
            false
        }
    };
    upd(st, ctx, |s| s.session_secure = Some(secure));
    if !secure {
        // A rare case (the Windows key store is unavailable) - but it must be known right away.
        crate::popup::push(
            crate::popup::Kind::Info,
            tr!("Вход не зашифрован", "Login not encrypted"),
            tr!("Хранилище ключей Windows недоступно: сохраненный вход лежит открытым. Подробнее - Настройки -> Аккаунт.", "The Windows key storage is unavailable: the saved login is stored unencrypted. Details - Settings -> Account."),
        );
    }
    start_social(client, roster, st, ctx);
    start_link_probe(st, ctx);
    load_owned(client, st, ctx).await;
}

/// Keep the saved login (Aurelia's session.json) encrypted: it holds the key with which
/// Steam lets you in without a password. Aurelia writes it in plain text by default (encryption
/// is enabled only by its console command secure-session), so after every login
/// we turn on encrypt_session and save the file again. The key lives in the Windows Credential
/// Manager (for this Windows user only). Ok(false) - the key store is unavailable
/// (or disabled by AURELIA_DISABLE_KEYRING), the file stayed open.
async fn secure_session() -> anyhow::Result<bool> {
    use aurelia::core::config::{load_launcher_config, load_session, save_launcher_config, save_session, session_path};
    use aurelia::core::session_crypto::{keyring_secret_or_create, EncryptedSession};
    let key = tokio::task::spawn_blocking(keyring_secret_or_create).await.ok().flatten();
    if key.is_none() {
        return Ok(false);
    }
    let mut cfg = load_launcher_config().await?;
    if !cfg.encrypt_session {
        cfg.encrypt_session = true;
        save_launcher_config(&cfg).await?;
    }
    let path = session_path()?;
    let raw = tokio::fs::read_to_string(&path).await.unwrap_or_default();
    if raw.is_empty() || EncryptedSession::from_json(&raw).is_some() {
        return Ok(true);
    }
    let session = load_session().await?;
    save_session(&session).await?;
    let raw = tokio::fs::read_to_string(&path).await.unwrap_or_default();
    Ok(EncryptedSession::from_json(&raw).is_some())
}

/// Merge the loaded chat history with what is already on screen: the history is the base, and
/// lines missing from it that are not older than its last message (received or sent
/// while the history loaded) are appended.
/// A line with a server identity (exact) is a duplicate only of the history line with the same
/// direction, time and ordinal: two separate "yes" are two messages even within one second.
/// Only a line with a local fallback time (Steam sent no time) is matched by direction, text and
/// a time within SAME_MESSAGE_SECS. Each history line covers at most one on-screen line.
const SAME_MESSAGE_SECS: u64 = 2;

pub(crate) fn merge_history(history: Vec<ChatLine>, current: Vec<ChatLine>) -> Vec<ChatLine> {
    let newest = history.iter().map(|l| l.time).max().unwrap_or(0);
    let hist_len = history.len();
    let mut used = vec![false; hist_len];
    let mut out = history;
    for l in current {
        if l.time + 120 < newest {
            continue; // old - already in the history
        }
        let m = (0..hist_len).find(|&i| {
            let h = &out[i];
            !used[i]
                && h.from_self == l.from_self
                && if l.exact && h.exact {
                    h.time == l.time && h.ordinal == l.ordinal
                } else {
                    h.text == l.text && h.time.abs_diff(l.time) <= SAME_MESSAGE_SECS
                }
        });
        match m {
            Some(i) => used[i] = true,
            None => out.push(l),
        }
    }
    out.sort_by_key(|l| (l.time, l.ordinal));
    out
}

/// Connection check session number: a new login stops the check of the previous session.
static PROBE_GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Login number: background tasks of the previous login (chat messages) no longer write anything
/// after a logout or a new login.
static SESSION_GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

tokio::task_local! {
    /// Login number under which a background task was started (see spawn_session).
    static TASK_SESSION: u64;
}

/// Start a background task bound to the current login: if while it runs the user
/// logs out or into another account, its writes to NetState are discarded (upd). This way a response
/// for the previous account (family, recent games, profile...) does not reach the new one.
pub(crate) fn spawn_session<F>(fut: F) -> tokio::task::JoinHandle<F::Output>
where
    F: std::future::Future + Send + 'static,
    F::Output: Send + 'static,
{
    // A task started from another login task gets the PARENT's number (task-local is not
    // inherited by itself): otherwise a subtask started after the account switch would write its response.
    let gen = TASK_SESSION.try_with(|g| *g).unwrap_or_else(|_| SESSION_GEN.load(Ordering::Relaxed));
    tokio::spawn(TASK_SESSION.scope(gen, fut))
}

/// State lock, if the code is not running in a previous-login task (otherwise None - the response is stale).
/// The check is under the lock: a new login changes the login number before writing its state, and the write
/// waits for the same lock, so a stale response cannot land on top of a new one.
pub(crate) fn lock_current(st: &Shared) -> Option<NetGuard<'_>> {
    let g = st.lock().unwrap();
    (!stale_task()).then_some(g)
}

/// Whether the code runs in a previous-login task.
pub(crate) fn stale_task() -> bool {
    TASK_SESSION.try_with(|g| *g != SESSION_GEN.load(Ordering::Relaxed)).unwrap_or(false)
}

/// Every 20 s check whether Steam is reachable (a connection to steamcommunity.com:443). Two
/// failures in a row - offline mode, as when starting without network: the "No connection to Steam" bar,
/// and offline_tick in the UI tries to log in again once a minute. Previously Vaporust noticed
/// a lost internet connection only after a restart.
fn start_link_probe(st: &Shared, ctx: &egui::Context) {
    let gen = PROBE_GEN.fetch_add(1, Ordering::Relaxed) + 1;
    let (st, ctx) = (st.clone(), ctx.clone());
    tokio::spawn(async move {
        let mut fails = 0;
        loop {
            tokio::time::sleep(crate::lite::secs(20, 60)).await;
            if PROBE_GEN.load(Ordering::Relaxed) != gen {
                return;
            }
            let ok = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                tokio::net::TcpStream::connect("steamcommunity.com:443"),
            )
            .await
            .is_ok_and(|r| r.is_ok());
            if ok {
                fails = 0;
                continue;
            }
            fails += 1;
            if fails < 2 || PROBE_GEN.load(Ordering::Relaxed) != gen {
                continue;
            }
            let mut went = false;
            upd(&st, &ctx, |s| {
                if s.account.is_some() {
                    s.offline = true;
                    s.offline_reason = tr!("Нет подключения к интернету", "No internet connection").into();
                    s.account = None;
                    went = true;
                }
            });
            if went {
                crate::errlog::write(tr!("сеть", "network"), tr!("связь со Steam пропала (проверка соединения)", "connection to Steam lost (connection check)"));
            }
            return;
        }
    });
}

/// Reread the latest conversation messages (after sending an image).
/// Second-tier commands (profile, chat, community, store, family, Workshop).
async fn second_plan(client: &SteamClient, cmd: Cmd, st: &Shared, ctx: &egui::Context) {
    use crate::actions as a;
    let msg = |r: Result<(), String>, ok: &str| match r {
        Ok(()) => ok.to_string(),
        Err(e) => trf!("Не получилось: {e}", "Failed: {e}"),
    };
    let country = {
        let s = st.lock().unwrap().store.clone();
        let c = s.lock().unwrap().country.clone();
        if c.is_empty() { "US".to_string() } else { c }
    };
    match cmd {
        Cmd::ProfileEditLoad => {
            let r = crate::profileedit::load(client).await;
            upd(st, ctx, |s| s.profile_edit = Some(r));
        }
        Cmd::ProfileEditSave(e) => {
            let r = crate::profileedit::save(client, &e).await;
            let fresh = crate::profileedit::load(client).await;
            upd(st, ctx, |s| {
                s.profile_edit_msg = Some(msg(r, tr!("Профиль сохранен.", "Profile saved.")));
                s.profile_edit = Some(fresh);
            });
        }
        Cmd::PrivacySave(p) => {
            let r = crate::profileedit::save_privacy(client, &p).await;
            let fresh = crate::profileedit::load(client).await;
            upd(st, ctx, |s| {
                s.profile_edit_msg = Some(msg(r, tr!("Настройки приватности сохранены.", "Privacy settings saved.")));
                s.profile_edit = Some(fresh);
            });
        }
        Cmd::AvatarUpload(p) => {
            upd(st, ctx, |s| s.profile_edit_msg = Some(tr!("Загружаю аватар...", "Uploading the avatar...").into()));
            let r = crate::profileedit::upload_avatar(client, &p).await;
            let fresh = crate::profileedit::load(client).await;
            upd(st, ctx, |s| {
                s.profile_edit_msg = Some(msg(r, tr!("Аватар обновлен (в списках друзей появится не сразу).", "Avatar updated (friend lists show it with a delay).")));
                s.profile_edit = Some(fresh);
            });
        }
        Cmd::SetNickname(friend, nick) => {
            let r = crate::profileedit::set_nickname(client, friend, &nick).await;
            upd(st, ctx, |s| {
                if r.is_ok() {
                    if let Some(f) = s.roster.write().unwrap().get_mut(&friend) {
                        f.nickname = if nick.trim().is_empty() { None } else { Some(nick.trim().to_string()) };
                    }
                }
                s.friend_msg = Some(msg(r, if nick.trim().is_empty() { tr!("Ник убран.", "Nickname removed.") } else { tr!("Ник сохранен.", "Nickname saved.") }));
            });
        }
        Cmd::ChatReaction { friend, time, ordinal, kind, name, add } => {
            let r = a::chat_reaction(client, friend, time, ordinal, kind, &name, add).await;
            if let Err(e) = r {
                upd(st, ctx, |s| Arc::make_mut(&mut s.chats).entry(friend).or_default().error = Some(trf!("Реакция не поставлена: {e}", "Reaction not added: {e}")));
            }
            reload_history(client, friend, st, ctx).await;
        }
        Cmd::CreateChatGroup { name, invitees } => {
            let r = a::create_chat_group(client, &name, &invitees).await;
            let groups = client.my_chat_groups().await;
            upd(st, ctx, |s| {
                s.action_msg = Some(match r {
                    Ok(_) => trf!("Групповой чат «{name}» создан, приглашения отправлены.", "Group chat \"{name}\" created, invitations sent."),
                    Err(e) => trf!("Не получилось: {e}", "Failed: {e}"),
                });
                if let Ok(g) = groups {
                    s.chat_groups = Some(Ok(g));
                }
            });
        }
        Cmd::InviteToChatGroup { group, friend } => {
            let r = a::invite_to_chat_group(client, group, friend).await;
            upd(st, ctx, |s| s.action_msg = Some(msg(r, tr!("Приглашение отправлено.", "Invitation sent."))));
        }
        Cmd::ReviewLoad(appid) => {
            let r = a::review_load(client, appid).await;
            upd(st, ctx, |s| {
                s.my_reviews.insert(appid, r);
            });
        }
        Cmd::ReviewSave { appid, review } => {
            let r = a::review_save(client, appid, &review).await;
            let fresh = a::review_load(client, appid).await;
            upd(st, ctx, |s| {
                s.action_msg = Some(msg(r, tr!("Обзор опубликован.", "Review published.")));
                s.my_reviews.insert(appid, fresh);
            });
        }
        Cmd::CreateTopic { appid, title, text } => {
            let r = a::create_topic(client, appid, &title, &text).await;
            upd(st, ctx, |s| {
                s.action_msg = Some(match r {
                    Ok(url) => trf!("Тема создана: {url}", "Topic created: {url}"),
                    Err(e) => trf!("Не получилось: {e}", "Failed: {e}"),
                });
            });
        }
        Cmd::AwardConfig => {
            let r = a::award_config(client).await;
            upd(st, ctx, |s| s.award_config = Some(r));
        }
        Cmd::Award { target_type, target, reaction } => {
            let r = a::award(client, target_type, &target, reaction).await;
            upd(st, ctx, |s| s.action_msg = Some(msg(r, tr!("Награда отправлена.", "Award sent."))));
        }
        Cmd::WishlistReorder(items) => {
            let r = a::wishlist_reorder(client, &items).await;
            upd(st, ctx, |s| s.action_msg = Some(msg(r, tr!("Порядок желаемого сохранен.", "Wishlist order saved."))));
        }
        Cmd::CartLoad => {
            let r = a::cart_get(client, &country).await;
            upd(st, ctx, |s| s.cart = Some(r));
        }
        Cmd::CartAdd(packageid) => {
            let r = a::cart_add(client, packageid, &country).await;
            let cart = a::cart_get(client, &country).await;
            upd(st, ctx, |s| {
                s.action_msg = Some(msg(r, tr!("Добавлено в корзину.", "Added to the cart.")));
                s.cart = Some(cart);
            });
        }
        Cmd::CartRemove(line) => {
            let r = a::cart_remove(client, line, &country).await;
            let cart = a::cart_get(client, &country).await;
            upd(st, ctx, |s| {
                s.action_msg = Some(msg(r, tr!("Убрано из корзины.", "Removed from the cart.")));
                s.cart = Some(cart);
            });
        }
        Cmd::AppPackages(appid) => {
            let r = a::app_packages(appid, &country).await;
            upd(st, ctx, |s| {
                s.app_packages.insert(appid, r);
            });
        }
        Cmd::Curators(appid) => {
            let r = a::curators_for_app(appid).await;
            let store = st.lock().unwrap().store.clone();
            store.lock().unwrap().curators.insert(appid, r);
            ctx.request_repaint();
        }
        Cmd::FamilyInvite { family, steamid, role } => {
            let r = a::family_invite(client, &family, steamid, role).await;
            let me = client.my_steam_id().unwrap_or(0);
            let fresh = crate::family::load(client, me).await;
            upd(st, ctx, |s| {
                s.action_msg = Some(msg(r, tr!("Приглашение в семью отправлено (Steam попросит подтвердить по почте).", "Family invitation sent (Steam asks to confirm by email).")));
                s.family_group = Some(fresh);
            });
        }
        Cmd::FamilyCancel { family, steamid } => {
            let r = a::family_cancel(client, &family, steamid).await;
            let me = client.my_steam_id().unwrap_or(0);
            let fresh = crate::family::load(client, me).await;
            upd(st, ctx, |s| {
                s.action_msg = Some(msg(r, tr!("Приглашение отменено.", "Invitation cancelled.")));
                s.family_group = Some(fresh);
            });
        }
        Cmd::WsCollections { appid, text } => {
            let r = a::ws_collections(client, appid, &text).await;
            upd(st, ctx, |s| {
                s.ws_collections.insert((appid, text), r);
            });
        }
        Cmd::WsCollectionItems(id) => {
            let r = a::ws_collection_items(id).await;
            upd(st, ctx, |s| {
                s.ws_collection_items.insert(id, r);
            });
        }
        Cmd::WsVote { id, up } => {
            let r = a::ws_vote(client, id, up).await;
            upd(st, ctx, |s| s.action_msg = Some(msg(r, if up { tr!("Вы оценили мод: нравится.", "You rated the mod: like.") } else { tr!("Вы оценили мод: не нравится.", "You rated the mod: dislike.") })));
        }
        Cmd::WsFavorite { id, appid, add } => {
            let r = a::ws_favorite(client, id, appid, add).await;
            upd(st, ctx, |s| s.action_msg = Some(msg(r, if add { tr!("Добавлено в избранное.", "Added to favorites.") } else { tr!("Убрано из избранного.", "Removed from favorites.") })));
        }
        _ => {}
    }
}

async fn reload_history(client: &SteamClient, friend: u64, st: &Shared, ctx: &egui::Context) {
    if let Ok((msgs, more)) = client.chat_history(friend, 50, None).await {
        upd(st, ctx, |s| {
            let conv = Arc::make_mut(&mut s.chats).entry(friend).or_default();
            if !conv.history_loaded {
                conv.oldest = msgs.iter().map(|m| (m.timestamp, m.ordinal)).min();
                conv.more_history = more;
            }
            let history: Vec<ChatLine> = msgs
                .into_iter()
                .rev()
                .map(|m| ChatLine {
                    from_self: m.from_self,
                    text: m.message,
                    time: m.timestamp as u64,
                    ordinal: m.ordinal,
                    exact: true,
                    reactions: m.reactions,
                })
                .collect();
            // Earlier messages loaded with "Show earlier messages" are older than these 50: they stay
            // (replacing the list wiped them and left a hole in the conversation).
            let first = history.first().map(|l| (l.time, l.ordinal));
            let current = std::mem::take(&mut conv.lines);
            let (older, recent): (Vec<ChatLine>, Vec<ChatLine>) = match first {
                Some(f) => current.into_iter().partition(|l| l.exact && (l.time, l.ordinal) < f),
                None => (Vec::new(), current),
            };
            let mut lines = older;
            lines.extend(merge_history(history, recent));
            conv.lines = lines;
            conv.history_loaded = true;
        });
    }
}

/// Read an image for sending: (bytes, type for Steam, width, height).
/// PNG/JPEG/GIF/WebP are sent as is, others (BMP and the like) are re-encoded to PNG.
fn prepare_image(path: &std::path::Path) -> Result<(Vec<u8>, &'static str, u32, u32), String> {
    use image::ImageFormat;
    let bytes = std::fs::read(path).map_err(|e| trf!("Не удалось прочитать файл: {e}", "Could not read the file: {e}"))?;
    if bytes.len() > 20 * 1024 * 1024 {
        return Err(tr!("Файл больше 20 МБ - Steam такой не примет.", "The file is over 20 MB - Steam does not accept it.").into());
    }
    let format = image::guess_format(&bytes).map_err(|_| tr!("Это не картинка (или формат не поддерживается).", "This is not an image (or the format is not supported).").to_string())?;
    let reader = image::ImageReader::with_format(std::io::Cursor::new(&bytes), format);
    let (w, h) = reader
        .into_dimensions()
        .map_err(|e| trf!("Не удалось прочитать картинку: {e}", "Could not read the image: {e}"))?;
    let ext = match format {
        ImageFormat::Png => "png",
        ImageFormat::Jpeg => "jpeg",
        ImageFormat::Gif => "gif",
        ImageFormat::WebP => "webp",
        _ => {
            let img = image::load_from_memory(&bytes).map_err(|e| trf!("Не удалось прочитать картинку: {e}", "Could not read the image: {e}"))?;
            let mut out = std::io::Cursor::new(Vec::new());
            img.write_to(&mut out, ImageFormat::Png)
                .map_err(|e| trf!("Не удалось перекодировать картинку: {e}", "Could not re-encode the image: {e}"))?;
            return Ok((out.into_inner(), "png", w, h));
        }
    };
    Ok((bytes, ext, w, h))
}

pub fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Background tasks for the session: friend list and incoming messages.
/// Both end by themselves when the connection closes (logout).
fn start_social(client: &SteamClient, roster: Arc<RwLock<Roster>>, st: &Shared, ctx: &egui::Context) {
    let names = roster.clone(); // sender names for notifications
    // Friends: Aurelia announces the status itself ("invisible" by default) and watches for changes.
    {
        let (client, ctx) = (client.clone(), ctx.clone());
        tokio::spawn(async move {
            // Repaint every 4 s while the watcher runs so friend statuses refresh
            // (more often is not needed: when idle this is the main source of frames).
            let ctx2 = ctx.clone();
            let ticker = tokio::spawn(async move {
                loop {
                    tokio::time::sleep(crate::lite::secs(4, 15)).await;
                    ctx2.request_repaint();
                }
            });
            let _ = client.run_friends_watcher(roster).await;
            ticker.abort();
        });
    }
    // Incoming messages.
    // Our own listener keeps the server ordinal of each message (steam-vent-chat drops it).
    let Ok(events) = client.chat_events() else {
        return;
    };
    let gen = SESSION_GEN.load(Ordering::Relaxed); // the login number is set by logged_in
    let (st, ctx) = (st.clone(), ctx.clone());
    tokio::spawn(async move {
        use aurelia::steam_client::RawChatKind;
        use tokio_stream::StreamExt;
        let mut events = std::pin::pin!(events);
        while let Some(m) = events.next().await {
            if SESSION_GEN.load(Ordering::Relaxed) != gen {
                break;
            }
            let friend = m.friend;
            let (from_self, text) = match m.kind {
                RawChatKind::Message => (false, m.message), // with BBCode: parsed by chatfmt
                // Own message sent from another device (for example from Steam on a phone).
                RawChatKind::Echo => (true, m.message),
                RawChatKind::GameInvite => (false, tr!("[приглашение в игру]", "[game invite]").to_string()),
                RawChatKind::Typing => {
                    upd(&st, &ctx, |s| {
                        Arc::make_mut(&mut s.chats).entry(friend).or_default().typing_at = Some(Instant::now());
                    });
                    continue;
                }
            };
            let line = ChatLine {
                from_self,
                text,
                time: if m.timestamp > 0 { m.timestamp } else { now_unix() },
                ordinal: m.ordinal,
                exact: m.timestamp > 0,
                ..Default::default()
            };
            upd(&st, &ctx, |s| {
                let conv = Arc::make_mut(&mut s.chats).entry(friend).or_default();
                // Our own message comes back as an echo: SendChat may have added it already
                // (same server time and number) - then it is not added twice.
                let dup = line.exact
                    && conv.lines.iter().rev().take(50).any(|l| l.exact && l.from_self == line.from_self && l.time == line.time && l.ordinal == line.ordinal);
                if !dup {
                    conv.lines.push(line);
                }
                conv.typing_at = None;
                if !from_self {
                    conv.unread += 1;
                }
            });
            if !from_self {
                // Sound, taskbar flashing, tray dot and a popup notification.
                let (name, accent) = names
                    .read()
                    .ok()
                    .and_then(|r| {
                        r.get(&friend).map(|f| {
                            let in_game = f.game_app_id.is_some_and(|g| g != 0);
                            (f.persona_name.clone(), crate::popup::status_color(f.persona_state, in_game))
                        })
                    })
                    .unwrap_or((None, None));
                let text = conv_text(&st, friend);
                if !text.trim().is_empty() {
                    crate::notify::new_message(friend, name, text, accent);
                }
            }
        }
    });
}

/// Own game list cache - a separate file per account (Aurelia has one for everybody).
/// The library is taken from it when there is no connection to Steam.
fn library_cache_file(steam_id: u64) -> Option<std::path::PathBuf> {
    let base = if crate::portable::enabled() { crate::portable::data_dir() } else { crate::portable::normal_dir() };
    base.map(|d| d.join(format!("library_{steam_id}.json")))
}

/// Write the cache only while the response belongs to the current login (under the state lock):
/// a late response for the previous account must not get into the new one's cache.
fn save_library_cache(st: &Shared, steam_id: Option<u64>, games: &[aurelia::core::models::OwnedGame]) {
    let Some(p) = steam_id.and_then(library_cache_file) else { return };
    let Some(_guard) = lock_current(st) else { return };
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let tmp = p.with_extension("json.tmp");
    if serde_json::to_vec(games).ok().is_some_and(|b| std::fs::write(&tmp, b).is_ok()) {
        let _ = std::fs::rename(&tmp, &p);
    }
}

/// The account's game list cache (for working offline).
fn load_library_cache(steam_id: Option<u64>) -> Vec<aurelia::core::models::OwnedGame> {
    steam_id
        .and_then(library_cache_file)
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

async fn load_owned(client: &mut SteamClient, st: &Shared, ctx: &egui::Context) {
    upd(st, ctx, |s| s.message = tr!("Загружаю библиотеку...", "Loading the library...").into());
    match client.fetch_owned_games().await {
        Ok(games) => {
            save_library_cache(st, client.my_steam_id(), &games);
            upd(st, ctx, |s| {
                s.message.clear();
                s.owned = Arc::new(games);
                s.busy = false;
            })
        }
        Err(e) => upd(st, ctx, |s| {
            s.message = trf!("Не удалось получить список игр: {e:#}", "Could not get the game list: {e:#}");
            s.busy = false;
        }),
    }
    // Family library: an error does not affect the rest (the account may not be in a family).
    if let Ok(family) = client.fetch_family_shared_apps().await {
        upd(st, ctx, |s| s.family = Arc::new(family));
    }
}

fn set_install(st: &Shared, ctx: &egui::Context, f: impl FnOnce(&mut Install)) {
    upd(st, ctx, |s| {
        if let Some(i) = s.install.as_mut() {
            f(i);
        }
    });
}

pub fn fmt_bytes(b: u64) -> String {
    let b = b as f64;
    const MB: f64 = 1024.0 * 1024.0;
    if b >= 1024.0 * MB {
        trf!("{:.2} ГБ", "{:.2} GB", b / 1024.0 / MB)
    } else if b >= MB {
        trf!("{:.1} МБ", "{:.1} MB", b / MB)
    } else {
        trf!("{:.0} КБ", "{:.0} KB", b / 1024.0)
    }
}

/// Is there enough space: the game needs `disk_size`, `already` is downloaded (resuming an install
/// needs no space for it), `free` is free (None - could not be determined, do not block).
pub(crate) fn space_error(disk_size: u64, already: u64, free: Option<u64>) -> Option<String> {
    let free = free?;
    let need = disk_size.saturating_sub(already);
    (need > free).then(|| trf!("Не хватает места на диске: нужно {}, свободно {}.", "Not enough disk space: {} needed, {} free.", fmt_bytes(need), fmt_bytes(free)))
}

#[allow(non_snake_case)]
fn CANCELLED() -> &'static str {
    tr!("Установка отменена. Скачанное сохранено, ее можно продолжить.", "Installation cancelled. What was downloaded is kept, it can be resumed.")
}

async fn run_install(
    client: SteamClient,
    appid: u32,
    library: PathBuf,
    abort: Arc<AtomicBool>,
    st: &Shared,
    ctx: &egui::Context,
) -> Result<(), String> {
    // If the download was hidden from Steam (pause/cancel) - put the appmanifest back.
    crate::local::unstash_manifest(&library, appid);
    set_install(st, ctx, |i| i.stage = tr!("Узнаю размер игры...", "Checking the game size...").into());
    let est = client
        .estimate_install_size(appid, DepotPlatform::Windows)
        .await
        .map_err(|e| trf!("Не удалось узнать размер игры: {e:#}", "Could not get the game size: {e:#}"))?;
    // What is already downloaded (resuming an install) needs no space.
    let game_files = crate::local::installed_size(&library, appid);
    if let Some(e) = space_error(est.disk_size, game_files, crate::local::free_space(&library)) {
        return Err(e);
    }
    set_install(st, ctx, |i| i.total = est.disk_size);
    if abort.load(Ordering::Relaxed) {
        return Err(CANCELLED().into());
    }

    let state = Arc::new(RwLock::new(DownloadState::default()));
    // We replace Aurelia's cancel flag with ours so the "Cancel" button works immediately.
    if let Ok(mut s) = state.write() {
        s.abort_signal = abort.clone();
    }
    let opts = InstallOptions {
        library_override: Some(library.to_string_lossy().into_owned()),
        ..Default::default()
    };
    let rx = client
        .install_game(appid, DepotPlatform::Windows, opts, state)
        .await
        .map_err(|e| trf!("Не удалось начать установку: {e:#}", "Could not start the installation: {e:#}"))?;
    drain_progress(rx, &abort, CANCELLED(), st, ctx).await
}

/// Common Aurelia progress loop (install, file verification, move): updates the progress
/// bar at the bottom of the window, computes speed. `cancelled` - text for a cancellation.
async fn drain_progress(
    mut rx: tokio::sync::mpsc::Receiver<aurelia::core::models::DownloadProgress>,
    abort: &Arc<AtomicBool>,
    cancelled: &str,
    st: &Shared,
    ctx: &egui::Context,
) -> Result<(), String> {
    // Speed: measured every ~2 s with smoothing.
    let mut last: Option<(Instant, u64)> = None;
    let mut speed = 0.0f64;
    while let Some(p) = rx.recv().await {
        match p.state {
            DownloadProgressState::Queued => {
                set_install(st, ctx, |i| i.stage = tr!("Подготовка загрузки...", "Preparing the download...").into());
            }
            DownloadProgressState::Downloading
            | DownloadProgressState::Verifying
            | DownloadProgressState::Moving => {
                let net_bytes = p.bytes_downloaded.saturating_sub(p.reused_bytes);
                let now = Instant::now();
                match last {
                    Some((t, b)) if now.duration_since(t) >= Duration::from_secs(2) => {
                        let secs = now.duration_since(t).as_secs_f64();
                        let inst = net_bytes.saturating_sub(b) as f64 / secs;
                        speed = if speed == 0.0 { inst } else { speed * 0.6 + inst * 0.4 };
                        last = Some((now, net_bytes));
                    }
                    None => last = Some((now, net_bytes)),
                    _ => {}
                }
                let stage = match p.state {
                    DownloadProgressState::Verifying => tr!("Проверка файлов", "Verifying files"),
                    DownloadProgressState::Moving => tr!("Перенос файлов", "Moving files"),
                    _ => tr!("Загрузка", "Download"),
                };
                set_install(st, ctx, |i| {
                    i.stage = stage.into();
                    i.done = p.bytes_downloaded;
                    if p.total_bytes > 0 {
                        i.total = p.total_bytes;
                    }
                    i.speed = speed;
                });
            }
            DownloadProgressState::Completed => return Ok(()),
            DownloadProgressState::Failed => {
                return Err(if abort.load(Ordering::Relaxed) {
                    cancelled.into()
                } else {
                    trf!("Ошибка: {}", "Error: {}", p.current_file)
                });
            }
        }
    }
    Err(if abort.load(Ordering::Relaxed) {
        cancelled.into()
    } else {
        tr!("Операция прервалась без результата.", "The operation stopped without a result.").into()
    })
}

/// A long operation on a game in the `install` slot (one at a time): progress bar at the bottom,
/// at the end - the result text and a disk rescan.
fn start_op<F, Fut>(appid: u32, name: String, done_text: &str, st: &Shared, ctx: &egui::Context, f: F)
where
    F: FnOnce(Arc<AtomicBool>) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<(), String>> + Send + 'static,
{
    if st.lock().unwrap().install.as_ref().is_some_and(|i| i.active()) {
        return;
    }
    let abort = Arc::new(AtomicBool::new(false));
    upd(st, ctx, |s| {
        s.install = Some(Install {
            appid,
            name,
            stage: tr!("Подготовка...", "Preparing...").into(),
            done: 0,
            total: 0,
            speed: 0.0,
            result: None,
            abort: abort.clone(),
            done_text: done_text.into(),
            download: false,
        });
    });
    let (st, ctx) = (st.clone(), ctx.clone());
    tokio::spawn(async move {
        let res = f(abort).await;
        upd(&st, &ctx, |s| {
            if let Some(i) = s.install.as_mut() {
                i.result = Some(res);
                i.speed = 0.0;
            }
            s.disk_changed += 1;
        });
    });
}

/// Aurelia error (anyhow) into text with causes.
fn es<E: std::fmt::Display>(e: E) -> String {
    format!("{e:#}")
}

/// Load everything for the "Properties" window (in parallel, as it becomes ready).
fn load_props(client: &SteamClient, appid: u32, st: &Shared, ctx: &egui::Context) {
    upd(st, ctx, |s| {
        s.props.insert(appid, PropsData::default());
    });
    let set = move |st: &Shared, ctx: &egui::Context, f: Box<dyn FnOnce(&mut PropsData) + Send>| {
        upd(st, ctx, |s| f(s.props.entry(appid).or_default()));
    };

    {
        let (c, st, ctx) = (client.clone(), st.clone(), ctx.clone());
        spawn_session(async move {
            let r = c.fetch_game_languages(appid).await.map_err(es);
            set(&st, &ctx, Box::new(move |p| p.languages = Some(r)));
        });
    }
    {
        let (c, st, ctx) = (client.clone(), st.clone(), ctx.clone());
        spawn_session(async move {
            // Steam does not show private (password) branches anyway until they are unlocked.
            let r = c
                .branches_detailed(appid)
                .await
                .map(|v| {
                    let mut v: Vec<_> = v.into_iter().filter(|b| !b.2).map(|b| (b.0, b.1)).collect();
                    if !v.iter().any(|b| b.0 == "public") {
                        v.push(("public".into(), None));
                    }
                    v
                })
                .map_err(es);
            set(&st, &ctx, Box::new(move |p| p.branches = Some(r)));
        });
    }
    {
        let (c, st, ctx) = (client.clone(), st.clone(), ctx.clone());
        spawn_session(async move {
            let r = load_dlc(&c, appid).await;
            set(&st, &ctx, Box::new(move |p| p.dlc = Some(r)));
        });
    }
    {
        let (c, st, ctx) = (client.clone(), st.clone(), ctx.clone());
        spawn_session(async move {
            let r = c.depot_descriptions(appid).await.map_err(es);
            set(&st, &ctx, Box::new(move |p| p.depots = Some(r)));
        });
    }
    {
        let (c, st, ctx) = (client.clone(), st.clone(), ctx.clone());
        spawn_session(async move {
            let r = async {
                let mut files = c.cloud_client().map_err(es)?.get_file_list(appid).await.map_err(es)?;
                files.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
                Ok(files
                    .into_iter()
                    .map(|f| CloudFile { name: f.filename, size: f.size, time: f.timestamp as u64 })
                    .collect())
            }
            .await;
            set(&st, &ctx, Box::new(move |p| p.cloud = Some(r)));
        });
    }
}

async fn load_dlc(c: &SteamClient, appid: u32) -> Result<Vec<DlcRow>, String> {
    let ext = c.get_extended_app_info(appid).await.map_err(|e| format!("{e:#}"))?;
    let states = c.dlc_states(appid, &ext.dlcs).await.map_err(|e| format!("{e:#}"))?;
    Ok(states
        .into_iter()
        .map(|s| DlcRow { app_id: s.app_id, owned: s.owned, installed: s.installed, disabled: s.disabled })
        .collect())
}

#[cfg(test)]
mod live {
    /// Manual check on a real account (read only):
    /// cargo test live_emoticons -- --ignored --nocapture
    #[test]
    #[ignore]
    fn live_emoticons() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let (e, s) = c.fetch_emoticons_and_stickers().await.unwrap();
            println!("emoticons {}: {:?}", e.len(), &e[..e.len().min(15)]);
            println!("stickers {}: {:?}", s.len(), &s[..s.len().min(15)]);
        });
    }
}

#[cfg(test)]
mod live_props {
    /// cargo test live_props_data -- --ignored --nocapture (read only)
    #[test]
    #[ignore]
    fn live_props_data() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            for app in [70u32, 2280] {
                println!("== {app}");
                println!("langs {:?}", c.fetch_game_languages(app).await.map_err(|e| e.to_string()));
                println!("branches {:?}", c.fetch_branches(app).await.map_err(|e| e.to_string()));
                let ext = c.get_extended_app_info(app).await.unwrap();
                println!("dlcs {:?} branch {}", ext.dlcs, ext.active_branch);
                let st = c.dlc_states(app, &ext.dlcs).await.map_err(|e| e.to_string());
                println!("dlc states {:?}", st.map(|v| v.iter().map(|s| (s.app_id, s.owned, s.installed, s.disabled)).collect::<Vec<_>>()));
                let cloud = c.cloud_client().unwrap().get_file_list(app).await.map(|f| f.len()).map_err(|e| e.to_string());
                println!("cloud files {:?}", cloud);
            }
        });
    }
}

#[cfg(test)]
mod live_updates {
    /// cargo test live_updates_check -- --ignored --nocapture (read only)
    #[test]
    #[ignore]
    fn live_updates_check() {
        let steam = crate::local::find_steam().unwrap();
        let games = crate::local::scan_games(&steam.dir);
        let list: Vec<_> = games.iter().filter(|g| g.ready).map(|g| (g.appid, g.dir.clone())).collect();
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let t = std::time::Instant::now();
            let ids = c.games_needing_update(&list).await.unwrap();
            let names: Vec<_> = ids.iter().filter_map(|i| games.iter().find(|g| g.appid == *i)).map(|g| g.name.clone()).collect();
            println!("checked {} in {:.1}s, need update: {:?}", list.len(), t.elapsed().as_secs_f32(), names);
        });
    }
}

#[cfg(test)]
mod live_store {
    /// cargo test live_store_data -- --ignored --nocapture (read only)
    #[test]
    #[ignore]
    fn live_store_data() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let cc = c.store_country();
            println!("country {cc} me {:?}", c.my_steam_id());
            let s = c.search_store("doom", 5, "russian", &cc).await.unwrap();
            println!("search {:?} total {}", s.app_ids, s.total);
            let apps = c.fetch_store_apps(&[70, 2280], "russian", &cc).await.unwrap();
            for a in &apps {
                println!("== {} {} price {:?} orig {:?} disc {} review {:?} rel {:?}", a.app_id, a.name, a.price, a.original_price, a.discount_pct, a.review_summary, a.release_date);
                println!("dev {:?} pub {:?} tags {:?}", a.developers, a.publishers, a.tags.iter().take(5).map(|t| t.name.clone()).collect::<Vec<_>>());
                println!("assets {:?}", a.assets);
                println!("shots {} first {:?}", a.screenshots.len(), a.screenshots.first());
                println!("short {:?}", a.short_description.chars().take(150).collect::<String>());
                println!("full {:?}", a.full_description.chars().take(300).collect::<String>());
            }
            let w = c.wishlist(c.my_steam_id().unwrap()).await.map(|w| w.len());
            println!("wishlist {:?}", w.map_err(|e| e.to_string()));
            let d = c.query_deals(aurelia::steam_client::DealsScope::DiscountedTopSellers, 1, 0, 10, "russian", &cc).await.map(|p| p.app_ids.len());
            println!("deals {:?}", d.map_err(|e| e.to_string()));
            let sim = c.similar_apps(70, 5, "russian", &cc).await.map(|p| p.app_ids);
            println!("similar {:?}", sim.map_err(|e| e.to_string()));
            println!("players {:?}", c.current_players(70).await.map_err(|e| e.to_string()));
            let http = aurelia::core::net::steam_web_client().unwrap();
            let news = aurelia::web::discovery::fetch_news(&http, 70, 3, 300).await.map(|n| n.iter().map(|x| x.title.clone()).collect::<Vec<_>>());
            println!("news {:?}", news.map_err(|e| e.to_string()));
        });
    }
}

#[cfg(test)]
mod live_collections {
    /// cargo test live_collections_pull -- --ignored --nocapture (read only from Steam)
    #[test]
    #[ignore]
    fn live_collections_pull() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let store = aurelia::library::collections::pull(&c).await.unwrap();
            for col in &store.collections {
                println!("{} | {} | added {} | dynamic {} | builtin {}", col.id, col.name, col.added.len(), col.is_dynamic(), col.is_builtin());
            }
            println!("total {} version {}", store.collections.len(), store.namespace_version);
        });
    }
}

#[cfg(test)]
mod live_social {
    /// cargo test live_social_data -- --ignored --nocapture (read only)
    #[test]
    #[ignore]
    fn live_social_data() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let me = c.my_steam_id().unwrap();
            match c.user_profile(me).await {
                Ok(p) => println!("profile name {:?} url {} avatar {:?} vis {} created {:?} loc {:?} headline {:?} summary {:?} state {:?} game {:?} sources {:?}",
                    p.persona_name, p.profile_url, p.avatar_url.is_some(), p.visibility, p.created, p.location, p.headline, p.summary.map(|s| s.chars().take(60).collect::<String>()), p.persona_state, p.game_name, p.sources),
                Err(e) => println!("profile err {e:#}"),
            }
            let page = c.query_workshop_files(550, None, 0, "*", 5, &[]).await;
            match page {
                Ok(p) => println!("workshop total {} items {:?}", p.total, p.items.iter().map(|i| (i.id, i.title.clone(), i.file_size)).collect::<Vec<_>>()),
                Err(e) => println!("workshop err {e:#}"),
            }
            println!("subscribed {:?}", c.fetch_subscribed_items(550).await.map(|v| v.len()).map_err(|e| e.to_string()));
        });
    }
}

#[cfg(test)]
mod limit_live {
    /// Live speed limit test: downloads a game into a temporary folder (VAPORUST_DUMP)
    /// with the VAPORUST_LIMIT kbit/s limit for 20 seconds and prints what the progress counted.
    #[test]
    #[ignore]
    fn download_limit_live() {
        use std::sync::{atomic::Ordering, Arc, RwLock};
        let dir = std::env::var("VAPORUST_DUMP").unwrap();
        let limit: u64 = std::env::var("VAPORUST_LIMIT").unwrap().parse().unwrap();
        let appid: u32 = std::env::var("VAPORUST_APP").unwrap_or("3620".into()).parse().unwrap();
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut cfg = aurelia::core::config::LauncherConfig::load().await.unwrap();
            cfg.download_limit_kbit = limit;
            cfg.save().await.unwrap();
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let state = Arc::new(RwLock::new(aurelia::core::models::DownloadState::default()));
            let abort = state.read().unwrap().abort_signal.clone();
            let opts = aurelia::steam_client::InstallOptions {
                library_override: Some(dir),
                ..Default::default()
            };
            let mut rx = c
                .install_game(appid, aurelia::core::models::DepotPlatform::Windows, opts, state)
                .await
                .unwrap();
            let t0 = std::time::Instant::now();
            let mut last = 0u64;
            let mut mark: Option<(f64, u64)> = None;
            let switch: Option<u64> = std::env::var("VAPORUST_LIMIT2").ok().and_then(|v| v.parse().ok());
            while t0.elapsed().as_secs() < 25 {
                if let (Some(l2), None) = (switch, mark) {
                    if t0.elapsed().as_secs() >= 8 {
                        let mut cfg = aurelia::core::config::LauncherConfig::load().await.unwrap();
                        cfg.download_limit_kbit = l2;
                        cfg.save().await.unwrap();
                        aurelia::steam_client::set_download_limit_now(l2);
                        println!("limit changed: {last} bytes in 8 s");
                        mark = Some((t0.elapsed().as_secs_f64(), last));
                    }
                }
                match tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv()).await {
                    Ok(Some(p)) => last = p.bytes_downloaded.saturating_sub(p.reused_bytes),
                    Ok(None) => break,
                    Err(_) => {}
                }
            }
            println!("progress bytes: {last} in {:.1} s", t0.elapsed().as_secs_f64());
            if let Some((t, b)) = mark {
                println!("after the change: {:.2} MB/s", (last - b) as f64 / 1e6 / (t0.elapsed().as_secs_f64() - t));
            }
            abort.store(true, Ordering::Relaxed);
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        });
    }
}

#[cfg(test)]
mod family_live {
    /// Live test: games of the Steam family group (without own ones).
    #[test]
    #[ignore]
    fn family_apps_live() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            match c.fetch_family_shared_apps().await {
                Ok(v) => {
                    println!("family games: {}", v.len());
                    for a in v.iter().take(40) {
                        println!("{} {} owner={}", a.app_id, a.name, a.owner_steamid.is_some());
                    }
                }
                Err(e) => println!("error: {e:#}"),
            }
        });
    }
}

/// Last message of a conversation (with BBCode; for the notification).
fn conv_text(st: &Shared, friend: u64) -> String {
    let s = st.lock().unwrap();
    s.chats
        .get(&friend)
        .and_then(|c| c.lines.last())
        .map(|l| l.text.clone())
        .unwrap_or_default()
}

#[cfg(test)]
mod effects_live {
    /// Live test: which item types are in the Steam inventory (753/6) - looking for chat effects.
    #[test]
    #[ignore]
    fn inventory_types_live() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let web = c.web_session().await.unwrap();
            let items = aurelia::steam_client::inventory_via_lang(&web, 753, 6, "english").await.unwrap();
            let mut seen = std::collections::BTreeMap::<String, Vec<String>>::new();
            for i in items {
                let e = seen.entry(i.item_type.clone()).or_default();
                if e.len() < 4 {
                    e.push(format!("{} | {}", i.name, i.icon_url.unwrap_or_default()));
                }
            }
            for (t, v) in seen {
                if t.contains("Card") || t.contains("Background") {
                    continue;
                }
                println!("== {t}");
                for x in v {
                    println!("   {x}");
                }
            }
        });
    }
}

#[cfg(test)]
mod logon_burst_live {
    /// Live test: which messages the server sends right after login (buffer of unhandled ones).
    #[test]
    #[ignore]
    fn logon_burst_live() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let _ = c.connection().unwrap().take_unprocessed();
            c.announce_persona(7).await.unwrap(); // invisible, like the user's
            for wait in [0u64, 3, 6] {
                tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
                let kinds: Vec<String> =
                    c.connection().unwrap().take_unprocessed().iter().map(|r| format!("{:?}", r.kind)).collect();
                println!("+{wait}s: {kinds:?}");
            }
        });
    }
}

#[cfg(test)]
mod shots_live {
    /// Live test: own screenshots in the Steam cloud.
    #[test]
    #[ignore]
    fn shots_live() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            use steam_vent_proto::steammessages_publishedfile_steamclient::*;
            use steam_vent::ConnectionTrait;
            let conn = c.connection().unwrap();
            for (ty, ft) in [("myfiles", Some(4u32))] {
                let mut req = CPublishedFile_GetUserFiles_Request::new();
                req.set_steamid(u64::from(conn.steam_id()));
                req.set_appid(0);
                req.set_numperpage(6);
                req.set_type(ty.to_string());
                if let Some(ft) = ft {
                    req.set_filetype(ft);
                }
                let r: Result<CPublishedFile_GetUserFiles_Response, _> = conn.service_method(req).await;
                match r {
                    Ok(r) => {
                        println!("== {ty} {ft:?}: total {}", r.total());
                        for d in &r.publishedfiledetails {
                            println!("  {} type={} {} [{}] file={} url={} img={} prev={} fn={} {}x{}", d.publishedfileid(), d.file_type(), d.app_name(), d.title(), d.file_url(), d.url(), d.image_url(), d.preview_url(), d.filename(), d.image_width(), d.image_height());
                        }
                    }
                    Err(e) => println!("== {ty}: err {e}"),
                }
            }
        });
    }
}


/// After an Aurelia update: restore the original appmanifest (language, SharedDepots, InstallScripts...)
/// and, if the update succeeded, write the new depot versions and build from Aurelia's file into it.
pub fn merge_after_update(acf: &std::path::Path, original: &str, ok: bool) -> Result<(), String> {
    let updated = crate::local::installed_depots(acf);
    let build = crate::local::acf_buildid(acf);
    let tmp = acf.with_extension("acf.tmp");
    std::fs::write(&tmp, original)
        .and_then(|_| std::fs::rename(&tmp, acf))
        .map_err(|e| trf!("Не удалось вернуть файл игры: {e}", "Could not restore the game file: {e}"))?;
    if ok {
        crate::local::write_depot_versions(acf, &updated, build.as_deref(), 4)?;
    }
    Ok(())
}

#[cfg(test)]
mod update_direct_live {
    /// Live test of updating without Steam in a SEPARATE library (VAPORUST_DUMP), your games are not touched:
    /// own Aurelia settings folder,
    /// "old" version = a fake number in appmanifest + language and SharedDepots, then update and check.
    /// WARNING: Aurelia also looks for appmanifest in C:\Program Files (x86)\Steam (alphabetical sort) -
    /// so only a game that is not in the real libraries (Ricochet by default, 5 MB).
    #[test]
    #[ignore]
    fn update_direct_live() {
        use aurelia::core::models::{DepotPlatform, DownloadProgressState, DownloadState};
        let root = std::path::PathBuf::from(std::env::var("VAPORUST_DUMP").unwrap());
        let cfg_dir = root.join("cfg");
        let lib = root.join("lib");
        std::fs::create_dir_all(&cfg_dir).unwrap();
        std::fs::create_dir_all(lib.join("steamapps")).unwrap();
        let real = dirs_config();
        for f in ["session.json"] {
            std::fs::copy(real.join(f), cfg_dir.join(f)).unwrap();
        }
        std::env::set_var("AURELIA_CONFIG_DIR", &cfg_dir);
        // Only a game that is NOT in the Steam libraries: Aurelia always also looks in C:\Program Files (x86)\Steam.
        let appid: u32 = std::env::var("VAPORUST_APP").ok().and_then(|v| v.parse().ok()).unwrap_or(60);
        let steam = crate::local::find_steam().unwrap();
        assert!(
            !crate::local::scan_games(&steam.dir).iter().any(|g| g.appid == appid),
            "game {appid} is installed in Steam - the test does not touch it"
        );
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut cfg = aurelia::core::config::LauncherConfig::load().await.unwrap();
            cfg.steam_library_path = lib.to_string_lossy().into_owned();
            cfg.windows_steam_discovery_enabled = false;
            cfg.download_limit_kbit = 0;
            cfg.save().await.unwrap();
            aurelia::steam_client::set_download_limit_now(0);
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let acf = lib.join("steamapps").join(format!("appmanifest_{appid}.acf"));
            async fn drain(mut rx: tokio::sync::mpsc::Receiver<aurelia::core::models::DownloadProgress>) -> bool {
                while let Some(p) = rx.recv().await {
                    match p.state {
                        DownloadProgressState::Completed => return true,
                        DownloadProgressState::Failed => {
                            println!("error: {}", p.current_file);
                            return false;
                        }
                        _ => {}
                    }
                }
                false
            }
            if !acf.exists() {
                let st = std::sync::Arc::new(std::sync::RwLock::new(DownloadState::default()));
                let opts = aurelia::steam_client::InstallOptions {
                    library_override: Some(lib.to_string_lossy().into_owned()),
                    ..Default::default()
                };
                let rx = c.install_game(appid, DepotPlatform::Windows, opts, st).await.unwrap();
                assert!(drain(rx).await, "the install failed");
            }
            // "Old" version + what Aurelia loses when rewriting.
            let real_versions = crate::local::installed_depots(&acf);
            let fake: std::collections::BTreeMap<u32, u64> = real_versions.keys().map(|d| (*d, 1111)).collect();
            crate::local::write_depot_versions(&acf, &fake, Some("1"), 6).unwrap();
            let mut text = std::fs::read_to_string(&acf).unwrap();
            let end = text.rfind('}').unwrap();
            text.insert_str(end, "\t\"UserConfig\"\n\t{\n\t\t\"language\"\t\t\"russian\"\n\t}\n\t\"SharedDepots\"\n\t{\n\t\t\"228988\"\t\t\"228980\"\n\t}\n");
            std::fs::write(&acf, &text).unwrap();
            let original = std::fs::read_to_string(&acf).unwrap();
            // Update - like Cmd::UpdateDirect.
            let st = std::sync::Arc::new(std::sync::RwLock::new(DownloadState::default()));
            let t = std::time::Instant::now();
            let rx = c.update_game(appid, st).await.unwrap();
            let ok = drain(rx).await;
            super::merge_after_update(&acf, &original, ok).unwrap();
            let after = std::fs::read_to_string(&acf).unwrap();
            println!("update: {ok}, {:.1} s\n{after}", t.elapsed().as_secs_f32());
            assert!(ok);
            assert_eq!(crate::local::installed_depots(&acf), real_versions, "versions not updated");
            assert!(after.contains("\"language\"\t\t\"russian\""), "language lost");
            assert!(after.contains("SharedDepots"), "SharedDepots lost");
            assert!(after.contains("\"StateFlags\"\t\t\"4\""));
        });
    }

    fn dirs_config() -> std::path::PathBuf {
        std::path::PathBuf::from(std::env::var("USERPROFILE").unwrap()).join(".config").join("Aurelia")
    }
}

#[cfg(test)]
mod small_games_live {
    /// Small owned/family games that are not on disk (for safe download tests).
    #[test]
    #[ignore]
    fn small_games_live() {
        let steam = crate::local::find_steam().unwrap();
        let installed: std::collections::HashSet<u32> = crate::local::scan_games(&steam.dir).iter().map(|g| g.appid).collect();
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let mut ids: Vec<(u32, String)> = c.fetch_owned_games().await.unwrap().into_iter().map(|g| (g.app_id, g.name)).collect();
            ids.extend(c.fetch_family_shared_apps().await.unwrap_or_default().into_iter().map(|a| (a.app_id, a.name)));
            let mut out = Vec::new();
            for (id, name) in ids.into_iter().filter(|(id, _)| !installed.contains(id)).take(120) {
                if let Ok(e) = c.estimate_install_size(id, aurelia::core::models::DepotPlatform::Windows).await {
                    if e.download_size > 0 && e.download_size < 60_000_000 {
                        out.push((e.download_size, id, name));
                    }
                }
            }
            out.sort();
            for (s, id, n) in out.iter().take(10) {
                println!("{id} {n}: {} MB", s / 1_000_000);
            }
        });
    }
}

#[cfg(test)]
mod lang_depots_live {
    /// Language depots of games (VAPORUST_APPS, comma-separated).
    #[test]
    #[ignore]
    fn lang_depots_live() {
        let ids: Vec<u32> = std::env::var("VAPORUST_APPS").unwrap().split(',').filter_map(|s| s.trim().parse().ok()).collect();
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            for id in ids {
                let mut v = c.language_depots(id, "public").await.unwrap_or_default();
                v.sort_by_key(|x| x.0);
                println!("== {id}");
                for (dep, lang, os, from, dlc, man) in v {
                    println!("  {dep} lang={lang} os={os:?} from={from:?} dlc={dlc:?} manifest={man:?}");
                }
            }
        });
    }
}

/// Language change without Steam. `lang` - the new language (as in UserConfig: "russian", "english"...).
///
/// 1. Old-language depots are removed from appmanifest; if there were any - file verification
///    (the old language may have overwritten base files - verification restores the originals).
/// 2. Own new-language depots are downloaded (explicitly with a version: otherwise Aurelia takes only English).
///    Shared new-language depots (depotfromapp) - only as a link in SharedDepots, if the owner game
///    is already installed in the same library and has the depot; otherwise refused (Steam can do it).
/// 3. The original appmanifest is restored (Aurelia writes its own from scratch) and edited: depots, language
///    in UserConfig and MountedConfig, StateFlags 4.
pub async fn switch_language(
    c: &SteamClient,
    appid: u32,
    acf: &std::path::Path,
    lang: &str,
    abort: &Arc<AtomicBool>,
    st: &Shared,
    ctx: &egui::Context,
) -> Result<(), String> {
    let new_state = || {
        let s = Arc::new(RwLock::new(DownloadState::default()));
        if let Ok(mut w) = s.write() {
            w.abort_signal = abort.clone();
        }
        s
    };
    use crate::local;
    let info = local::read_acf(acf);
    let branch = if info.user_beta.is_empty() { "public".to_string() } else { info.user_beta.clone() };
    let depots = c
        .language_depots(appid, &branch)
        .await
        .map_err(|e| trf!("Не удалось узнать языковые файлы игры: {e:#}", "Could not get the game's language files: {e:#}"))?;
    let library = acf.parent().and_then(|p| p.parent()).ok_or(tr!("Не найдена папка библиотеки", "Library folder not found"))?.to_path_buf();
    let installed = local::installed_depots(acf);
    let shared = local::shared_depots(acf);
    let lang_of = |d: u32| depots.iter().find(|x| x.0 == d).map(|x| x.1.clone());
    let windows_ok = |os: &Option<String>| os.as_ref().is_none_or(|o| o.to_lowercase().contains("windows"));

    let mut edit = local::DepotEdit::default();
    // Old language: own depots - remove (with file verification), shared - remove the link.
    edit.remove_installed = installed.keys().copied().filter(|d| lang_of(*d).is_some_and(|l| l != lang)).collect();
    edit.remove_shared = shared.keys().copied().filter(|d| lang_of(*d).is_some_and(|l| l != lang)).collect();
    // New language.
    let mut new_own: Vec<(u32, u64)> = Vec::new();
    for (d, l, os, from, dlc, manifest) in &depots {
        if l != lang || dlc.is_some() || !windows_ok(os) || installed.contains_key(d) || shared.contains_key(d) {
            continue;
        }
        match from {
            None => {
                let m = manifest.ok_or(trf!("Нет версии языкового хранилища {d} на ветке {branch}", "No version of language depot {d} on branch {branch}"))?;
                new_own.push((*d, m));
            }
            Some(owner) => {
                // Shared depot: fits only if the owner game is next to it and has it installed.
                let owner_acf = library.join("steamapps").join(format!("appmanifest_{owner}.acf"));
                if local::installed_depots(&owner_acf).contains_key(d) {
                    edit.add_shared.push((*d, *owner));
                } else {
                    return Err(trf!(
                        "Этот язык ставится вместе с другой игрой (appid {owner}), которой нет в этой библиотеке \
                         или в ней нет этого языка. Смените язык через Steam: «Свойства» -> «Обновить через Steam».", "This language is installed with another game (appid {owner}) that is not in this library or lacks this language. Change the language through Steam: \"Properties\" -> \"Update via Steam\"."
                    ));
                }
            }
        }
    }

    let mut text = std::fs::read_to_string(acf).map_err(|e| trf!("Не удалось прочитать файл игры: {e}", "Could not read the game file: {e}"))?;
    let restore = |t: &str| -> Result<(), String> {
        let tmp = acf.with_extension("acf.tmp");
        std::fs::write(&tmp, t).and_then(|_| std::fs::rename(&tmp, acf)).map_err(|e| trf!("Не удалось вернуть файл игры: {e}", "Could not restore the game file: {e}"))
    };

    // 1. Remove the old language and verify files.
    if !edit.remove_installed.is_empty() {
        // As Steam does: delete old-language files that are not in the game's remaining depots
        // (shared ones included). If some list could not be obtained - delete nothing.
        if let Err(e) = remove_language_files(c, appid, acf, &library, &installed, &shared, &edit.remove_installed).await {
            eprintln!("old language files not removed: {e}");
        }
        local::edit_depot_lists(acf, &local::DepotEdit { remove_installed: edit.remove_installed.clone(), ..Default::default() })?;
        text = std::fs::read_to_string(acf).map_err(|e| trf!("Не удалось прочитать файл игры: {e}", "Could not read the game file: {e}"))?;
        set_install(st, ctx, |i| i.stage = tr!("Проверка файлов (после смены языка)", "Verifying files (after the language change)").into());
        let rx = c.verify_game(appid, new_state()).await.map_err(|e| trf!("Не удалось начать проверку: {e:#}", "Could not start the verification: {e:#}"))?;
        let res = drain_progress(rx, abort, tr!("Смена языка отменена.", "Language change cancelled."), st, ctx).await;
        restore(&text)?;
        res?;
    }

    // 2. Download own new-language depots.
    if !new_own.is_empty() {
        let opts = InstallOptions {
            library_override: Some(library.to_string_lossy().into_owned()),
            filter_depots: Some(new_own.iter().map(|(d, _)| *d as u64).collect()),
            manifest_overrides: Some(new_own.iter().copied().collect()),
            branch: Some(branch.clone()).filter(|b| b != "public"),
            ..Default::default()
        };
        let rx = c
            .install_game(appid, DepotPlatform::Windows, opts, new_state())
            .await
            .map_err(|e| trf!("Не удалось начать загрузку языка: {e:#}", "Could not start downloading the language: {e:#}"))?;
        let res = drain_progress(rx, abort, tr!("Смена языка отменена.", "Language change cancelled."), st, ctx).await;
        let sizes = local::installed_depot_sizes(acf);
        let got = local::installed_depots(acf);
        restore(&text)?;
        res?;
        for (d, _) in &new_own {
            let m = *got.get(d).ok_or(trf!("Языковое хранилище {d} не скачалось", "Language depot {d} was not downloaded"))?;
            edit.add_installed.push((*d, m, sizes.get(d).copied().unwrap_or(0)));
        }
    }

    // 3. Write the changes into the original appmanifest.
    local::edit_depot_lists(acf, &edit)?;
    local::set_user_config(acf, "language", lang)?;
    local::set_mounted_config(acf, "language", lang)?;
    // set_user_config marks "update needed" - the files are already in place.
    local::write_depot_versions(acf, &Default::default(), None, 4)?;
    Ok(())
}

#[cfg(test)]
mod language_live {
    /// Language change without Steam in a SEPARATE library (VAPORUST_DUMP) on Ricochet (60), which is not in Steam:
    /// english -> french (own depot 62 is downloaded, shared 72 from Half-Life - as a link to a fake
    /// appmanifest_70 in the temp folder) -> english (62 is removed, files verified).
    #[test]
    #[ignore]
    fn language_live() {
        use aurelia::core::models::{DepotPlatform, DownloadProgressState, DownloadState};
        let appid = 60u32;
        let steam = crate::local::find_steam().unwrap();
        assert!(!crate::local::scan_games(&steam.dir).iter().any(|g| g.appid == appid), "Ricochet is installed in Steam");
        let root = std::path::PathBuf::from(std::env::var("VAPORUST_DUMP").unwrap());
        let (cfg_dir, lib) = (root.join("cfg"), root.join("lib"));
        std::fs::create_dir_all(&cfg_dir).unwrap();
        std::fs::create_dir_all(lib.join("steamapps")).unwrap();
        let real = std::path::PathBuf::from(std::env::var("USERPROFILE").unwrap()).join(".config").join("Aurelia");
        std::fs::copy(real.join("session.json"), cfg_dir.join("session.json")).unwrap();
        std::env::set_var("AURELIA_CONFIG_DIR", &cfg_dir);
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut cfg = aurelia::core::config::LauncherConfig::load().await.unwrap();
            cfg.steam_library_path = lib.to_string_lossy().into_owned();
            cfg.windows_steam_discovery_enabled = false;
            cfg.save().await.unwrap();
            aurelia::steam_client::set_download_limit_now(0);
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let acf = lib.join("steamapps").join(format!("appmanifest_{appid}.acf"));
            if !acf.exists() {
                let st = std::sync::Arc::new(std::sync::RwLock::new(DownloadState::default()));
                let opts = aurelia::steam_client::InstallOptions {
                    library_override: Some(lib.to_string_lossy().into_owned()),
                    ..Default::default()
                };
                let mut rx = c.install_game(appid, DepotPlatform::Windows, opts, st).await.unwrap();
                let mut ok = false;
                while let Some(p) = rx.recv().await {
                    if p.state == DownloadProgressState::Completed {
                        ok = true;
                        break;
                    }
                    if p.state == DownloadProgressState::Failed {
                        break;
                    }
                }
                assert!(ok, "the install failed");
                crate::local::set_user_config(&acf, "language", "english").unwrap();
                crate::local::set_mounted_config(&acf, "language", "english").unwrap();
            }
            // A fake "Half-Life" next to it: it has the French depot 72.
            std::fs::write(
                lib.join("steamapps").join("appmanifest_70.acf"),
                "\"AppState\"\n{\n\t\"appid\"\t\t\"70\"\n\t\"InstalledDepots\"\n\t{\n\t\t\"72\"\n\t\t{\n\t\t\t\"manifest\"\t\t\"491735997354925434\"\n\t\t\t\"size\"\t\t\"1\"\n\t\t}\n\t}\n}\n",
            )
            .unwrap();
            let shared: super::Shared = Default::default();
            let ctx = eframe::egui::Context::default();
            let abort = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let dir = lib.join("steamapps").join("common").join("Half-Life");
            let count = |d: &std::path::Path| walk(d);
            let before = count(&dir);
            super::switch_language(&c, appid, &acf, "french", &abort, &shared, &ctx).await.unwrap();
            let fr = std::fs::read_to_string(&acf).unwrap();
            println!("--- french, files {} -> {}\n{fr}", before, count(&dir));
            assert!(crate::local::installed_depots(&acf).contains_key(&62), "62 not installed");
            assert_eq!(crate::local::shared_depots(&acf).get(&72), Some(&70));
            let i = crate::local::read_acf(&acf);
            assert_eq!((i.user_language.as_str(), i.mounted_language.as_str(), i.state_flags), ("french", "french", 4));
            super::switch_language(&c, appid, &acf, "english", &abort, &shared, &ctx).await.unwrap();
            let en = std::fs::read_to_string(&acf).unwrap();
            let after = count(&dir);
            println!("--- english, files {after}\n{en}");
            assert_eq!(after, before, "French files not removed");
            assert!(!crate::local::installed_depots(&acf).contains_key(&62));
            assert!(!crate::local::shared_depots(&acf).contains_key(&72));
            let i = crate::local::read_acf(&acf);
            assert_eq!((i.user_language.as_str(), i.mounted_language.as_str(), i.state_flags), ("english", "english", 4));
        });
    }

    fn walk(d: &std::path::Path) -> usize {
        std::fs::read_dir(d)
            .map(|r| r.flatten().map(|e| if e.path().is_dir() { walk(&e.path()) } else { 1 }).sum())
            .unwrap_or(0)
    }
}

/// Delete files of the `removed` depots that are in none of the game's remaining depots (own and shared).
async fn remove_language_files(
    c: &SteamClient,
    appid: u32,
    acf: &std::path::Path,
    library: &std::path::Path,
    installed: &std::collections::BTreeMap<u32, u64>,
    shared: &std::collections::BTreeMap<u32, u32>,
    removed: &[u32],
) -> Result<usize, String> {
    let text = std::fs::read_to_string(acf).map_err(es)?;
    let installdir = text
        .lines()
        .find_map(|l| {
            let t = l.trim();
            t.to_lowercase().starts_with("\"installdir\"").then(|| t.rsplit('"').nth(1).unwrap_or("").to_string())
        })
        .filter(|d| !d.is_empty())
        .ok_or(tr!("нет installdir", "no installdir"))?;
    let game_dir = library.join("steamapps").join("common").join(installdir);
    let files = |app: u32, depot: u32, manifest: u64| async move {
        c.depot_file_names(app, depot, manifest)
            .await
            .map_err(|e| trf!("список файлов хранилища {depot}: {e:#}", "file list of depot {depot}: {e:#}"))
    };
    let mut keep = std::collections::HashSet::new();
    for (d, m) in installed.iter().filter(|(d, _)| !removed.contains(d)) {
        keep.extend(files(appid, *d, *m).await?);
    }
    for (d, owner) in shared {
        let owner_acf = library.join("steamapps").join(format!("appmanifest_{owner}.acf"));
        let m = *crate::local::installed_depots(&owner_acf)
            .get(d)
            .ok_or(trf!("неизвестна версия общего хранилища {d}", "unknown version of shared depot {d}"))?;
        keep.extend(files(*owner, *d, m).await?);
    }
    let mut n = 0;
    for d in removed {
        let Some(m) = installed.get(d) else { continue };
        for f in files(appid, *d, *m).await? {
            if keep.contains(&f) {
                continue;
            }
            let p = game_dir.join(f.replace('/', "\\"));
            if p.is_file() && std::fs::remove_file(&p).is_ok() {
                n += 1;
            }
        }
    }
    Ok(n)
}

#[cfg(test)]
mod installed_langs_live {
    /// Installed games with their own language depots (for testing language change without Steam).
    #[test]
    #[ignore]
    fn installed_langs_live() {
        let steam = crate::local::find_steam().unwrap();
        let games = crate::local::scan_games(&steam.dir);
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            for g in games.iter().filter(|g| g.ready) {
                let Ok(d) = c.language_depots(g.appid, "public").await else { continue };
                let own: Vec<String> = d
                    .iter()
                    .filter(|x| x.3.is_none() && x.4.is_none())
                    .map(|x| x.1.clone())
                    .collect::<std::collections::BTreeSet<_>>()
                    .into_iter()
                    .collect();
                let shared = d.iter().filter(|x| x.3.is_some()).count();
                if !own.is_empty() || shared > 0 {
                    let acf = crate::local::read_acf(&crate::local::appmanifest_path(g));
                    println!(
                        "{} ({}, {} MB): own languages {:?}, shared {}; current language \"{}\"",
                        g.name, g.appid, g.size / 1_000_000, own, shared, acf.mounted_language
                    );
                }
            }
        });
    }
}

#[cfg(test)]
mod offline_live {
    /// "No network" without disconnecting the internet: a copy of Aurelia settings in a temp folder and all requests
    /// through a nonexistent proxy. Expected: login failed exactly as "offline", the library cache exists.
    #[test]
    #[ignore]
    fn offline_live() {
        let dir = std::env::temp_dir().join(format!("vaporust_offline_cfg_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let real = std::path::PathBuf::from(std::env::var("USERPROFILE").unwrap()).join(".config").join("Aurelia");
        for f in ["session.json", "library_cache.json"] {
            let _ = std::fs::copy(real.join(f), dir.join(f));
        }
        std::env::set_var("AURELIA_CONFIG_DIR", &dir);
        for v in ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "http_proxy", "https_proxy", "all_proxy"] {
            std::env::set_var(v, "http://127.0.0.1:9");
        }
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            let t = std::time::Instant::now();
            let r = c.restore_session().await;
            let cached = aurelia::core::config::load_library_cache().await.unwrap_or_default();
            if let Err(e) = &r {
                println!("Vaporust treats this as a network problem: {}", super::network_failure(e).await);
            }
            println!(
                "login: {}; error kind: {:?}; offline: {}; cached games: {}; {:.1} s",
                r.as_ref().map(|_| "ok".to_string()).unwrap_or_else(|e| format!("error ({e:#})")),
                r.as_ref().err().map(|e| aurelia::core::error::classify(e).kind),
                c.is_offline(),
                cached.len(),
                t.elapsed().as_secs_f32()
            );
        });
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// Did the login fail because of the network, not the login itself? Aurelia does not always tell them apart
/// (without internet it has a fallback server list, and the error may be a "timeout" of kind Unknown).
/// With a saved login, any error counts as a network one unless it is a login refusal.
async fn network_failure(e: &anyhow::Error) -> bool {
    use aurelia::core::error::ErrorKind as K;
    let kind = aurelia::core::error::classify(e).kind;
    if matches!(kind, K::Offline | K::NetworkTimeout | K::Timeout) {
        return true;
    }
    if matches!(kind, K::AuthRequired | K::LoginDenied | K::LoginExpired | K::AccessDenied) {
        return false;
    }
    let has_session = aurelia::core::config::load_session()
        .await
        .is_ok_and(|s| s.account_name.is_some() && s.refresh_token.is_some());
    let text = format!("{e:#}").to_lowercase();
    let auth_text = ["invalid", "expired", "revoked", "denied", "access", "no persisted"].iter().any(|w| text.contains(w));
    has_session && !auth_text
}

#[cfg(test)]
mod ach_live {
    /// Live test (read only): own L4D2 achievements and those of a few friends. Prints numbers only.
    #[test]
    #[ignore]
    fn friend_achievements_live() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let mine = c.fetch_achievements(550, "russian").await.unwrap();
            let icons = mine.iter().filter(|a| !a.icon_unlocked.is_empty()).count();
            println!("mine: {} of {}, icons {icons}", mine.iter().filter(|a| a.unlocked).count(), mine.len());
            let friends = c.collect_friends(std::time::Duration::from_secs(5)).await.unwrap();
            for f in friends.iter().filter(|f| (f.relationship == 3 && (f.steam_id >> 52) & 0xF == 1)).take(6) {
                match c.fetch_achievements_for(550, "russian", f.steam_id).await {
                    Ok(v) => println!("friend: {} of {}", v.iter().filter(|a| a.unlocked).count(), v.len()),
                    Err(e) => println!("friend: error {e:#}"),
                }
            }
        });
    }
}

/// The game's appmanifest in steamapps: normal or hidden from Steam (an unfinished Vaporust download).
fn manifest_of(steamapps: &std::path::Path, appid: u32) -> std::path::PathBuf {
    let acf = steamapps.join(format!("appmanifest_{appid}.acf"));
    if acf.exists() {
        return acf;
    }
    steamapps.join(format!("appmanifest_{appid}{}", crate::local::STASH_EXT))
}

/// Uninstall a game that shares its folder with other games (for example Counter-Strike, Half-Life,
/// Blue Shift and Opposing Force live in one common\Half-Life folder). Only files of this game's
/// depots that the neighbor games do not have are deleted; folders left empty are removed too.
/// File lists come from Steam; if even one cannot be obtained - nothing is deleted.
async fn uninstall_shared(
    c: &SteamClient,
    appid: u32,
    game_dir: &std::path::Path,
    others: &[u32],
) -> Result<usize, String> {
    let steamapps = game_dir
        .parent()
        .and_then(std::path::Path::parent)
        .ok_or(tr!("неверная папка игры", "wrong game folder"))?
        .to_path_buf();
    let remove = shared_remove_list(c, appid, &steamapps, others).await?;
    remove_listed(appid, game_dir, &steamapps, &remove)
}

/// Which files (paths inside the game folder) to delete for a game in a shared folder: its depot files
/// that the neighbors do not have. Deletes nothing.
async fn shared_remove_list(
    c: &SteamClient,
    appid: u32,
    steamapps: &std::path::Path,
    others: &[u32],
) -> Result<Vec<String>, String> {
    let steamapps = steamapps.to_path_buf();
    let my_acf = manifest_of(&steamapps, appid);
    let installed = crate::local::installed_depots(&my_acf);
    let files = |app: u32, depot: u32, manifest: u64| async move {
        c.depot_file_names(app, depot, manifest)
            .await
            .map_err(|e| trf!("список файлов хранилища {depot}: {e:#}", "file list of depot {depot}: {e:#}"))
    };
    // Neighbors' files (their own and shared depots) - not touched.
    let mut keep = std::collections::HashSet::new();
    // This game's depots used by neighbors (their SharedDepots) - not touched either.
    let mut protected = std::collections::HashSet::new();
    for &other in others {
        let acf = manifest_of(&steamapps, other);
        for (d, m) in crate::local::installed_depots(&acf) {
            keep.extend(files(other, d, m).await?.into_iter().map(|f| f.to_lowercase()));
        }
        for (d, owner) in crate::local::shared_depots(&acf) {
            if owner == appid {
                protected.insert(d);
            }
            let owner_acf = manifest_of(&steamapps, owner);
            if let Some(m) = crate::local::installed_depots(&owner_acf).get(&d) {
                keep.extend(files(owner, d, *m).await?.into_iter().map(|f| f.to_lowercase()));
            }
        }
    }
    // Collect all lists first, then delete: on a network error the disk stays as it was.
    let mut remove = Vec::new();
    for (d, m) in installed.iter().filter(|(d, _)| !protected.contains(d)) {
        for f in files(appid, *d, *m).await? {
            if !keep.contains(&f.to_lowercase()) {
                remove.push(f);
            }
        }
    }
    Ok(remove)
}

/// Delete the listed files, emptied folders, the unfinished download and the appmanifest.
fn remove_listed(
    appid: u32,
    game_dir: &std::path::Path,
    steamapps: &std::path::Path,
    remove: &[String],
) -> Result<usize, String> {
    let mut n = 0;
    let mut dirs = std::collections::BTreeSet::new();
    for f in remove {
        let p = game_dir.join(f.replace('/', "\\"));
        if p.is_file() && std::fs::remove_file(&p).is_ok() {
            n += 1;
        }
        // Folders that held deleted files are candidates for removal if they became empty.
        let mut parent = p.parent();
        while let Some(dir) = parent {
            if dir == game_dir || !dir.starts_with(game_dir) {
                break;
            }
            dirs.insert(dir.to_path_buf());
            parent = dir.parent();
        }
    }
    // Deepest first; remove_dir does not delete non-empty folders.
    let mut dirs: Vec<_> = dirs.into_iter().collect();
    dirs.sort_by_key(|d| std::cmp::Reverse(d.components().count()));
    for d in dirs {
        let _ = std::fs::remove_dir(&d);
    }
    let _ = std::fs::remove_dir_all(steamapps.join("downloading").join(appid.to_string()));
    for f in [
        steamapps.join(format!("appmanifest_{appid}.acf")),
        steamapps.join(format!("appmanifest_{appid}{}", crate::local::STASH_EXT)),
    ] {
        if f.exists() {
            std::fs::remove_file(&f).map_err(|e| trf!("не удалось удалить {}: {e}", "could not delete {}: {e}", f.display()))?;
        }
    }
    Ok(n)
}

#[cfg(test)]
mod shared_uninstall_live {
    /// What a new uninstall would delete for a game in a shared folder (Counter-Strike, 10 by default).
    /// DELETES NOTHING. VAPORUST_APP - another game.
    #[test]
    #[ignore]
    fn shared_plan_live() {
        let appid: u32 = std::env::var("VAPORUST_APP").ok().and_then(|v| v.parse().ok()).unwrap_or(10);
        let steam = crate::local::find_steam().unwrap();
        let games = crate::local::scan_games(&steam.dir);
        let me = games.iter().find(|g| g.appid == appid).expect("the game is not installed");
        let dir = me.dir.to_string_lossy().to_lowercase();
        let others: Vec<u32> =
            games.iter().filter(|g| g.appid != appid && g.dir.to_string_lossy().to_lowercase() == dir).map(|g| g.appid).collect();
        println!("{} ({appid}), neighbours: {others:?}", me.name);
        let steamapps = me.dir.parent().unwrap().parent().unwrap().to_path_buf();
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut c = aurelia::steam_client::SteamClient::new().unwrap();
            c.restore_session().await.unwrap();
            let list = super::shared_remove_list(&c, appid, &steamapps, &others).await.unwrap();
            let mut tops = std::collections::BTreeMap::<String, usize>::new();
            for f in &list {
                *tops.entry(f.split(['/', '\\']).next().unwrap_or("").to_lowercase()).or_default() += 1;
            }
            println!("files that would be removed: {}; by top folders: {tops:?}", list.len());
        });
    }
}

#[cfg(test)]
mod session_tests {
    use super::*;

    /// Both tests change the shared login number - one at a time.
    static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// A response of a previous-login task arriving after a logout or a new login is not written;
    /// tasks of the current login and normal code write as before.
    #[test]
    fn stale_task_writes_are_dropped() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let rt = tokio::runtime::Runtime::new().unwrap();
        let st: Shared = Arc::new(SharedNet::default());
        let ctx = egui::Context::default();
        rt.block_on(async {
            let (tx, rx) = tokio::sync::oneshot::channel::<()>();
            let (st2, ctx2) = (st.clone(), ctx.clone());
            let old = spawn_session(async move {
                let _ = rx.await; // a "long request" of the old account
                upd(&st2, &ctx2, |s| s.message = "old account".into());
            });
            // While the request was running, another account logged in.
            SESSION_GEN.fetch_add(1, Ordering::Relaxed);
            let _ = tx.send(());
            old.await.unwrap();
            assert!(st.lock().unwrap().message.is_empty(), "an answer from the previous login is dropped");
            let (st3, ctx3) = (st.clone(), ctx.clone());
            spawn_session(async move { upd(&st3, &ctx3, |s| s.message = "new".into()) }).await.unwrap();
            assert_eq!(st.lock().unwrap().message, "new");
        });
        upd(&st, &ctx, |s| s.message = "outside tasks".into());
        assert_eq!(st.lock().unwrap().message, "outside tasks");
    }

    /// QA-03: a subtask started from a login task also counts as that login's task - even if
    /// it started after the account switch.
    #[test]
    fn child_tasks_inherit_session() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let rt = tokio::runtime::Runtime::new().unwrap();
        let st: Shared = Arc::new(SharedNet::default());
        let ctx = egui::Context::default();
        rt.block_on(async {
            let (tx, rx) = tokio::sync::oneshot::channel::<()>();
            let (st2, ctx2) = (st.clone(), ctx.clone());
            let parent = spawn_session(async move {
                let _ = rx.await;
                // The subtask is created after the login switch.
                spawn_session(async move { upd(&st2, &ctx2, |s| s.message = "subtask answer".into()) }).await.unwrap();
            });
            SESSION_GEN.fetch_add(1, Ordering::Relaxed);
            let _ = tx.send(());
            parent.await.unwrap();
            assert!(st.lock().unwrap().message.is_empty(), "a subtask of the previous login wrote nothing");
        });
    }
}

#[cfg(test)]
mod merge_tests {
    use super::{merge_history, ChatLine};

    fn l(from_self: bool, text: &str, time: u64) -> ChatLine {
        ChatLine { from_self, text: text.into(), time, ..Default::default() }
    }

    /// A late history response does not remove a message sent while it was loading.
    #[test]
    fn late_history_keeps_new_lines() {
        let history = vec![l(false, "hi", 1000), l(true, "how are you", 1010)];
        let current = vec![l(true, "how are you", 1012), l(true, "new", 1100)];
        let m = merge_history(history, current);
        let texts: Vec<&str> = m.iter().map(|x| x.text.as_str()).collect();
        assert_eq!(texts, vec!["hi", "how are you", "new"], "no duplicates, the new line is kept");
        // Old lines that are not in the history (long ago) are not added.
        let m = merge_history(vec![l(false, "a", 5000)], vec![l(false, "old", 100)]);
        assert_eq!(m.len(), 1);
    }

    /// Two identical messages in a row are two messages: a history line covers only one.
    #[test]
    fn same_text_twice_is_not_dup() {
        let current = vec![l(true, "yes", 1000), l(true, "yes", 1060)];
        assert_eq!(merge_history(vec![l(false, "hi", 990)], current.clone()).len(), 3);
        assert_eq!(merge_history(vec![l(true, "yes", 1000)], current).len(), 2);
    }

    /// A new "yes" sent while the history (with an older "yes" a minute ago) was loading stays on screen.
    #[test]
    fn new_message_not_swallowed_by_older_same_text() {
        let merged = merge_history(vec![l(true, "yes", 1000)], vec![l(true, "yes", 1060)]);
        assert_eq!(merged.iter().map(|m| m.time).collect::<Vec<_>>(), vec![1000, 1060]);
    }

    fn x(from_self: bool, text: &str, time: u64, ordinal: u32) -> ChatLine {
        ChatLine { from_self, text: text.into(), time, ordinal, exact: true, ..Default::default() }
    }

    /// Lines with a server identity: two separate "yes" one second (or zero seconds) apart are
    /// two messages, even when the history arrives late and holds only the first one.
    #[test]
    fn quick_repeats_with_server_ids() {
        for from_self in [true, false] {
            // History has "yes" at 1000; on screen only the new "yes" at 1001 (arrived after the request).
            let m = merge_history(vec![x(from_self, "yes", 1000, 0)], vec![x(from_self, "yes", 1001, 0)]);
            assert_eq!(m.len(), 2, "one second later");
            // Same second, next ordinal.
            let m = merge_history(vec![x(from_self, "yes", 1000, 0)], vec![x(from_self, "yes", 1000, 1)]);
            assert_eq!(m.len(), 2, "in the same second");
            assert_eq!(m.iter().map(|l| l.ordinal).collect::<Vec<_>>(), vec![0, 1]);
            // The same message seen twice is one line.
            let m = merge_history(vec![x(from_self, "yes", 1000, 1)], vec![x(from_self, "yes", 1000, 1)]);
            assert_eq!(m.len(), 1, "the same message");
            // Server rewrote the text (sticker command): still one message by its identity.
            let m = merge_history(vec![x(from_self, "[sticker]", 1000, 0)], vec![x(from_self, "/sticker A", 1000, 0)]);
            assert_eq!(m.len(), 1, "text changed by the server");
        }
    }
}
