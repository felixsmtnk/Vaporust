//! Экспорт библиотеки в таблицу CSV (открывается в Excel и LibreOffice): разделитель «;»
//! и метка UTF-8 в начале - так русский Excel сам понимает столбцы и кириллицу.

use crate::net::NetState;
use crate::LauncherApp;

/// Поле CSV: в кавычках, если внутри есть ; " или перевод строки.
fn field(s: &str) -> String {
    if s.contains([';', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// Строки таблицы: (appid, название, установлена, размер, минут, семья, коллекции).
pub fn csv(rows: &[(u32, String, bool, u64, Option<u32>, bool, String)]) -> String {
    let mut out = String::from("\u{feff}appid;Название;Установлена;Размер, ГБ;Часов в игре;Семейная библиотека;Коллекции\r\n");
    for (appid, name, installed, size, min, family, colls) in rows {
        let gb = if *size > 0 { format!("{:.2}", *size as f64 / 1_073_741_824.0).replace('.', ",") } else { String::new() };
        let hours = min.map(|m| format!("{:.1}", m as f64 / 60.0).replace('.', ",")).unwrap_or_default();
        out.push_str(&format!(
            "{appid};{};{};{gb};{hours};{};{}\r\n",
            field(name),
            if *installed { "да" } else { "" },
            if *family { "да" } else { "" },
            field(colls)
        ));
    }
    out
}

impl LauncherApp {
    /// Спросить, куда сохранить, и записать таблицу всех игр. Возвращает текст для пользователя.
    pub(crate) fn export_library(&self, net: &NetState) -> Option<String> {
        let path = rfd::FileDialog::new()
            .set_title("Сохранить список игр")
            .set_file_name("Vaporust - мои игры.csv")
            .add_filter("Таблица CSV", &["csv"])
            .save_file()?;
        let colls = net.collections.as_deref().unwrap_or(&[]);
        let mut rows: Vec<_> = self
            .row_map(net)
            .into_values()
            .map(|r| {
                let c: Vec<&str> = colls.iter().filter(|c| c.apps.contains(&r.appid)).map(|c| c.name.as_str()).collect();
                (r.appid, r.name, r.installed, r.size, r.playtime_min, r.family, c.join(", "))
            })
            .collect();
        rows.sort_by_key(|r| r.1.to_lowercase());
        Some(match std::fs::write(&path, csv(&rows)) {
            Ok(()) => format!("Сохранено игр: {} - {}", rows.len(), path.display()),
            Err(e) => format!("Не удалось сохранить: {e}"),
        })
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn csv_format() {
        let t = super::csv(&[
            (70, "Half-Life".into(), true, 1_073_741_824, Some(90), false, "Избранное".into()),
            (10, "Игра; с \"кавычками\"".into(), false, 0, None, true, String::new()),
        ]);
        assert!(t.starts_with('\u{feff}'));
        let lines: Vec<&str> = t.trim_end().split("\r\n").collect();
        assert_eq!(lines[1], "70;Half-Life;да;1,00;1,5;;Избранное");
        assert_eq!(lines[2], "10;\"Игра; с \"\"кавычками\"\"\";;;;да;");
    }
}
