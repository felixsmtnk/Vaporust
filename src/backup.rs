//! Резервные копии сохранений игр: файлы облака Steam игры (там, где они лежат на этом ПК)
//! складываются в zip в «Документы\Vaporust\Сохранения\<игра>\<дата>.zip». В архиве - список
//! исходных путей (vaporust_backup.txt), по нему копия восстанавливается на место.
//! Архивы делает сама программа (библиотека zip), без внешних программ.

use std::fs;
use std::path::{Path, PathBuf};

const MANIFEST: &str = "vaporust_backup.txt";

/// Папка копий всех игр.
pub fn root() -> PathBuf {
    let home = std::env::var_os("USERPROFILE").map(PathBuf::from).unwrap_or_default();
    home.join("Documents").join("Vaporust").join("Сохранения")
}

/// Имя папки игры без запрещенных в Windows символов.
fn safe(name: &str) -> String {
    let s: String = name.chars().map(|c| if "<>:\"/\\|?*".contains(c) || c.is_control() { '_' } else { c }).collect();
    let s = s.trim().trim_end_matches('.').to_string();
    if s.is_empty() { "game".into() } else { s }
}

pub fn game_dir(game: &str) -> PathBuf {
    root().join(safe(game))
}

/// Сделать копию: только существующие файлы. Возвращает путь к архиву и число файлов.
/// Архив пишет сама программа (библиотека zip): системный tar.exe терял русские и другие
/// не-латинские буквы в путях на Windows с другим языком.
pub fn backup_to(dir: &Path, files: &[PathBuf]) -> Result<(PathBuf, usize), String> {
    use std::io::Write;
    let files: Vec<&PathBuf> = files.iter().filter(|f| f.is_file()).collect();
    if files.is_empty() {
        return Err("Файлов сохранений на этом ПК нет - копировать нечего.".into());
    }
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let stamp = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S").to_string();
    let out = dir.join(format!("{stamp}.zip"));
    let r = (|| -> Result<(), String> {
        let file = fs::File::create(&out).map_err(|e| format!("Архив не создан: {e}"))?;
        let mut z = zip::ZipWriter::new(file);
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let mut manifest = String::new();
        for (i, f) in files.iter().enumerate() {
            let name = format!("{i}_{}", f.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default());
            let data = fs::read(f).map_err(|e| format!("Не прочитан {}: {e}", f.display()))?;
            z.start_file(format!("files/{name}"), opts).map_err(|e| e.to_string())?;
            z.write_all(&data).map_err(|e| e.to_string())?;
            manifest.push_str(&format!("{name}\t{}\n", f.display()));
        }
        z.start_file(MANIFEST, opts).map_err(|e| e.to_string())?;
        z.write_all(manifest.as_bytes()).map_err(|e| e.to_string())?;
        z.finish().map_err(|e| e.to_string())?;
        Ok(())
    })();
    if r.is_err() {
        let _ = fs::remove_file(&out);
    }
    r.map(|_| (out, files.len()))
}

pub fn backup(game: &str, files: &[PathBuf]) -> Result<(PathBuf, usize), String> {
    backup_to(&game_dir(game), files)
}

/// Копии игры: (архив, время изменения, размер), новые первыми.
pub fn list(game: &str) -> Vec<(PathBuf, std::time::SystemTime, u64)> {
    let mut v: Vec<_> = fs::read_dir(game_dir(game))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "zip"))
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            Some((e.path(), m.modified().ok()?, m.len()))
        })
        .collect();
    v.sort_by(|a, b| b.1.cmp(&a.1));
    v
}

/// Вернуть файлы из копии на их места (перезаписывая). Возвращает число файлов.
pub fn restore(zip_path: &Path) -> Result<usize, String> {
    use std::io::Read;
    let file = fs::File::open(zip_path).map_err(|e| format!("Архив не открыт: {e}"))?;
    let mut z = zip::ZipArchive::new(file).map_err(|e| format!("Это не zip-архив: {e}"))?;
    let mut manifest = String::new();
    z.by_name(MANIFEST)
        .map_err(|_| "В архиве нет списка файлов - это не копия Vaporust.".to_string())?
        .read_to_string(&mut manifest)
        .map_err(|e| e.to_string())?;
    let mut n = 0;
    for line in manifest.lines() {
        let Some((name, dest)) = line.split_once('\t') else { continue };
        let dest = PathBuf::from(dest);
        let mut data = Vec::new();
        z.by_name(&format!("files/{name}"))
            .map_err(|e| format!("В архиве нет {name}: {e}"))?
            .read_to_end(&mut data)
            .map_err(|e| e.to_string())?;
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        fs::write(&dest, data).map_err(|e| format!("Не восстановлен {}: {e}", dest.display()))?;
        n += 1;
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backup_and_restore() {
        let base = std::env::temp_dir().join(format!("vaporust_bk_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let saves = base.join("Сохранения игры");
        fs::create_dir_all(saves.join("sub")).unwrap();
        let a = saves.join("slot1.sav");
        let b = saves.join("sub").join("профиль.dat");
        fs::write(&a, "первый").unwrap();
        fs::write(&b, "второй").unwrap();
        let (zip, n) = backup_to(&base.join("копии"), &[a.clone(), b.clone(), saves.join("нет.sav")]).unwrap();
        assert_eq!(n, 2);
        assert!(zip.is_file());
        fs::write(&a, "испорчен").unwrap();
        fs::remove_file(&b).unwrap();
        assert_eq!(restore(&zip).unwrap(), 2);
        assert_eq!(fs::read_to_string(&a).unwrap(), "первый");
        assert_eq!(fs::read_to_string(&b).unwrap(), "второй");
        assert_eq!(safe("A: B/C?"), "A_ B_C_");
        let _ = fs::remove_dir_all(&base);
    }
}
