//! Настройки, сохраняемые между запусками (eframe persistence: %APPDATA%\Vaporust\data\app.ron).
//! Размер и положение окна eframe запоминает сам.

use serde::{Deserialize, Serialize};

/// Порядок игр в библиотеке.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Default, Debug)]
pub enum Sort {
    /// Установленные сверху, затем по имени.
    #[default]
    Name,
    /// По времени в игре (больше - выше).
    Playtime,
    /// Недавно запущенные сверху.
    LastPlayed,
    /// По размеру на диске (больше - выше).
    Size,
}

impl Sort {
    pub const ALL: [Sort; 4] = [Sort::Name, Sort::LastPlayed, Sort::Playtime, Sort::Size];

    pub fn label(self) -> &'static str {
        match self {
            Sort::Name => "по имени",
            Sort::Playtime => "по времени в игре",
            Sort::LastPlayed => "недавно запущенные",
            Sort::Size => "по размеру на диске",
        }
    }
}

/// Вид библиотеки.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Default, Debug)]
pub enum View {
    /// Компактный список (как в первых версиях).
    #[default]
    List,
    /// Плитка с обложками.
    Grid,
}

/// Статус для друзей (EPersonaState Steam).
pub const PERSONAS: [(u32, &str); 4] = [(1, "В сети"), (3, "Отошел"), (2, "Не беспокоить"), (7, "Невидимка")];

pub fn persona_label(state: u32) -> &'static str {
    PERSONAS.iter().find(|(s, _)| *s == state).map_or("Невидимка", |(_, l)| l)
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Settings {
    pub freeze_webhelper: bool,
    /// Флаги запуска скрытого Steam (по умолчанию local::DEFAULT_STEAM_ARGS).
    pub steam_args: String,
    pub only_installed: bool,
    pub show_friends: bool,
    /// Папка библиотеки для установки новых игр.
    pub library: Option<String>,
    pub sort: Sort,
    /// Статус для друзей (1 в сети, 3 отошел, 2 не беспокоить, 7 невидимка).
    pub persona: u32,
    /// Крестик окна сворачивает в трей, а не закрывает программу.
    pub close_to_tray: bool,
    /// Звук при новом сообщении, когда окно не активно.
    pub notify_sound: bool,
    /// Уведомления: общая галочка и отдельные виды.
    pub notify_on: bool,
    pub notify_msg: bool,
    pub notify_friend_game: bool,
    pub notify_friend_online: bool,
    pub notify_wishlist: bool,
    pub notify_download: bool,
    /// Новые уведомления Steam (подарки, предметы, обмены, комментарии).
    pub notify_steam: bool,
    /// Скидки из списка желаемого, о которых уже сообщили: appid -> конец скидки (unix).
    pub notified_sales: std::collections::BTreeMap<u32, u64>,
    /// Свои эффекты комнаты (последний известный список - чтобы панель открывалась сразу).
    pub chat_effects: Vec<String>,
    /// Обновлять игры через скрытый Steam (по умолчанию - без Steam, загрузчиком Vaporust).
    pub update_via_steam: bool,
    /// Vaporust включил Steam автономный режим (loginusers.vdf) - снять, когда Steam закроется.
    pub steam_offline_set: bool,
    /// Очередь установок; сохраняется между запусками.
    pub downloads: Vec<QueueItem>,
    /// Старый формат очереди (alpha.2 до правки): только читаем и переносим в `downloads`.
    #[serde(skip_serializing)]
    pub queue: Vec<(u32, String)>,
    /// Загрузка, которая шла в момент закрытия Vaporust (или сбоя): при следующем запуске
    /// встает первой в очередь.
    pub current_download: Option<QueueItem>,
    /// Прерванные загрузки продолжаются сами (иначе встают в очередь на паузе).
    pub resume_downloads: bool,
    /// Вся очередь придержана кнопкой «Пауза очереди».
    pub queue_paused: bool,
    /// Регион цен в магазине (код страны); None - по IP подключения к Steam.
    pub store_country: Option<String>,
    /// Список или плитка.
    pub view: View,
    /// Игры, которые Vaporust начал качать и еще не докачал: пока загрузка не идет, их
    /// appmanifest прячется от Steam, иначе Steam сам начнет их качать.
    pub holding: Vec<u32>,
    /// Закрепленные версии игр: appid -> настоящие версии хранилищ (depot -> manifest ID).
    /// В appmanifest таких игр Vaporust пишет последние версии, чтобы Steam их не обновлял.
    pub pinned: std::collections::BTreeMap<u32, std::collections::BTreeMap<u32, u64>>,
    /// Проверять обновления Vaporust при запуске.
    pub check_updates: bool,
    /// Игры, которые запускаются без Steam (галочка в «Свойствах»).
    pub no_steam: std::collections::BTreeSet<u32>,
    /// Показывать игры семейной библиотеки в «Все игры».
    pub show_family: bool,
    /// Расписание загрузок: часы окна [sched_from, sched_to).
    pub sched_from: u8,
    pub sched_to: u8,
    /// Качать только в часы окна.
    pub sched_only: bool,
    /// В часы окна - свое ограничение скорости (КБ/с, 0 - без ограничения).
    pub sched_limit_on: bool,
    pub sched_limit_kb: u64,
    /// Запомненные таблицы запуска для игр без Steam (чтобы запускались и без интернета).
    pub direct_specs: std::collections::BTreeMap<u32, crate::direct::LaunchSpec>,
    /// Ограничение скорости загрузки игр, КБ/с (0 - без ограничения).
    pub download_limit_kb: u64,
    /// Прокси для запросов Aurelia (http://хост:порт или socks5://…), пусто - без прокси.
    pub proxy: String,
    /// Команды перед запуском и после выхода для каждой игры (свойства игры -> «Общие»).
    pub scripts: std::collections::BTreeMap<u32, crate::scripts::GameScripts>,
    /// Избранные друзья (наверху списка) и свои заметки о друзьях.
    /// Кнопка «Во что поиграть?» над библиотекой.
    pub random_button: bool,
    /// Обычная рамка окон Windows вместо своей.
    pub system_frame: bool,
    /// Производительность: анимации (стикеры, GIF, эффекты чата, значки загрузки).
    pub animations: bool,
    /// Обложки и фоны игр в библиотеке.
    pub covers: bool,
    /// Экономия памяти: картинки мельче, кэш картинок меньше.
    pub economy: bool,
    /// Отдавать память системе, когда окно в трее или идет игра.
    pub trim_memory: bool,
    pub fav_friends: std::collections::BTreeSet<u64>,
    pub friend_notes: std::collections::BTreeMap<u64, String>,
    /// Сторонние игры (не из Steam), добавленные кнопкой «Добавить игру».
    pub custom_games: Vec<crate::custom::CustomGame>,
}

/// Игра в очереди установки.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct QueueItem {
    pub appid: u32,
    pub name: String,
    /// Поставлена на паузу: очередь ее пропускает, пока не нажмут ▶.
    #[serde(default)]
    pub paused: bool,
}

impl Settings {
    /// Перенести очередь старого формата.
    pub fn migrate(&mut self) {
        // Прошлый запуск закрылся посреди загрузки - продолжить ее первой.
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
            system_frame: false,
            animations: true,
            covers: true,
            economy: false,
            trim_memory: true,
            fav_friends: Default::default(),
            friend_notes: Default::default(),
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
        assert!(s.downloads[0].paused, "без галочки - на паузе");
    }
}
