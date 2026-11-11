//! Настройки, сохраняемые между запусками (eframe persistence: %APPDATA%\Vaporust\data\app.ron).
//! Размер и положение окна eframe запоминает сам.

use serde::{Deserialize, Serialize};

/// Порядок игр в библиотеке.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Default, Debug)]
pub enum Sort {
    /// Установленные сверху, затем по имени.
    #[default]
    Name,
    /// По времени в игре (больше — выше).
    Playtime,
    /// Недавно запущенные сверху.
    LastPlayed,
    /// По размеру на диске (больше — выше).
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

/// Статус для друзей (EPersonaState Steam).
pub const PERSONAS: [(u32, &str); 4] = [(1, "В сети"), (3, "Отошёл"), (2, "Не беспокоить"), (7, "Невидимка")];

pub fn persona_label(state: u32) -> &'static str {
    PERSONAS.iter().find(|(s, _)| *s == state).map_or("Невидимка", |(_, l)| l)
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Settings {
    pub freeze_webhelper: bool,
    pub only_installed: bool,
    pub show_friends: bool,
    /// Папка библиотеки для установки новых игр.
    pub library: Option<String>,
    pub sort: Sort,
    /// Статус для друзей (1 в сети, 3 отошёл, 2 не беспокоить, 7 невидимка).
    pub persona: u32,
    /// Крестик окна сворачивает в трей, а не закрывает программу.
    pub close_to_tray: bool,
    /// Звук при новом сообщении, когда окно не активно.
    pub notify_sound: bool,
    /// Очередь установок; сохраняется между запусками.
    pub downloads: Vec<QueueItem>,
    /// Старый формат очереди (alpha.2 до правки): только читаем и переносим в `downloads`.
    #[serde(skip_serializing)]
    pub queue: Vec<(u32, String)>,
    /// Вся очередь придержана кнопкой «Пауза очереди».
    pub queue_paused: bool,
}

/// Игра в очереди установки.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct QueueItem {
    pub appid: u32,
    pub name: String,
    /// Поставлена на паузу: очередь её пропускает, пока не нажмут ▶.
    #[serde(default)]
    pub paused: bool,
}

impl Settings {
    /// Перенести очередь старого формата.
    pub fn migrate(&mut self) {
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
            only_installed: false,
            show_friends: true,
            library: None,
            sort: Sort::Name,
            persona: 7,
            close_to_tray: false,
            notify_sound: true,
            downloads: Vec::new(),
            queue: Vec::new(),
            queue_paused: false,
        }
    }
}

pub const KEY: &str = "vaporust_settings";
