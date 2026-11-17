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

mod chat;
mod files;
mod library;
#[cfg(test)]
mod livetests;
mod netactions;
pub(crate) use chat::*;
pub(crate) use files::*;
pub(crate) use library::*;
use netactions::*;

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
    #[allow(dead_code)] // ready in the network part, no button in the interface yet
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
    if crate::notify::chat_window_open() {
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
                let g = crate::store::gen();
                let res = client.fetch_achievements_for(appid, crate::lang::steam(), who).await.map_err(|e| format!("{e:#}"));
                if crate::store::gen() != g {
                    return; // names are in the old language; the page asks again
                }
                upd(&st, &ctx, |s| {
                    Arc::make_mut(&mut s.friend_achievements).insert((appid, who), res);
                });
            });
        }
        Cmd::Achievements(appid) => {
            // In the background: a slow response does not hold up other commands.
            let (client, st, ctx) = (client.clone(), st.clone(), ctx.clone());
            spawn_session(async move {
                let g = crate::store::gen();
                let res = client
                    .fetch_achievements(appid, crate::lang::steam())
                    .await
                .or_else(|e| {
                    let msg = format!("{e:#}");
                    // The game has no achievements: Steam answers with a NoMatch error, not an empty list.
                    if msg.contains("NoMatch") { Ok(Vec::new()) } else { Err(msg) }
                });
                if crate::store::gen() != g {
                    return;
                }
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
                let g = crate::store::gen();
                let r = crate::actions::download_regions().await;
                if crate::store::gen() == g {
                    upd(&st, &ctx, |s| s.download_regions = Some(r));
                }
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
        // Another account than before (for example "Sign in again" after a lost connection): nothing
        // of the previous one may stay - purchase history, profile settings, inventory, store state.
        // Only what does not belong to an account is kept: a running download and game names.
        if s.my_id.is_some() && s.my_id != my_id {
            let fresh = NetState {
                install: s.install.take(),
                app_names: s.app_names.clone(),
                ..Default::default()
            };
            *s = fresh;
        }
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

/// Merge the loaded chat history with what is already on screen: the history is the base, and
/// lines missing from it that are not older than its last message (received or sent
/// while the history loaded) are appended.
/// A line with a server identity (exact) is a duplicate only of the history line with the same
/// direction, time and ordinal: two separate "yes" are two messages even within one second.
/// Only a line with a local fallback time (Steam sent no time) is matched by direction, text and
/// a time within SAME_MESSAGE_SECS. Each history line covers at most one on-screen line.
const SAME_MESSAGE_SECS: u64 = 2;

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

pub fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
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

/// Aurelia error (anyhow) into text with causes.
fn es<E: std::fmt::Display>(e: E) -> String {
    format!("{e:#}")
}

