//! Save backups: a game's Steam Cloud files (where they live on this PC) are packed into
//! a zip in "Documents\Vaporust\Сохранения\<game>\<date>.zip". The archive contains a list
//! of the source paths (vaporust_backup.txt), used to restore the copy in place.
//! The program builds archives itself (the zip crate), no external programs.

use std::fs;
use std::path::{Path, PathBuf};

const MANIFEST: &str = "vaporust_backup.txt";

/// Folder with backups of all games.
pub fn root() -> PathBuf {
    let home = std::env::var_os("USERPROFILE").map(PathBuf::from).unwrap_or_default();
    home.join("Documents").join("Vaporust").join("Сохранения")
}

/// Game folder name without characters forbidden in Windows.
fn safe(name: &str) -> String {
    let s: String = name.chars().map(|c| if "<>:\"/\\|?*".contains(c) || c.is_control() { '_' } else { c }).collect();
    let s = s.trim().trim_end_matches('.').to_string();
    if s.is_empty() { "game".into() } else { s }
}

pub fn game_dir(game: &str) -> PathBuf {
    root().join(safe(game))
}

/// Make a backup: existing files only. Returns the archive path and the file count.
/// The archive is written by the program itself (zip crate): the system tar.exe lost Russian and other
/// non-Latin letters in paths on Windows with a different language.
pub fn backup_to(dir: &Path, files: &[PathBuf]) -> Result<(PathBuf, usize), String> {
    use std::io::Write;
    let files: Vec<&PathBuf> = files.iter().filter(|f| f.is_file()).collect();
    if files.is_empty() {
        return Err("Файлов сохранений на этом ПК нет - копировать нечего.".into());
    }
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    // Name by time; if a backup with this name already exists (two in one second) - "_2", "_3"...
    // The file is only created new (create_new): an existing backup is never overwritten.
    let stamp = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S").to_string();
    let (out, file) = (1..1000)
        .find_map(|n| {
            let name = if n == 1 { format!("{stamp}.zip") } else { format!("{stamp}_{n}.zip") };
            let p = dir.join(name);
            match fs::OpenOptions::new().write(true).create_new(true).open(&p) {
                Ok(f) => Some(Ok((p, f))),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => None,
                Err(e) => Some(Err(format!("Архив не создан: {e}"))),
            }
        })
        .unwrap_or_else(|| Err("Архив не создан: слишком много копий за одну секунду.".into()))?;
    let r = (|| -> Result<(), String> {
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

/// Backups of a game: (archive, modification time, size), newest first.
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

/// Put files from a backup back in place. All or nothing: the archive is read and checked first
/// in full (a single file missing - nothing is touched), then files are replaced one by one, and on
/// an error midway the already replaced ones are restored. Returns the number of files.
pub fn restore(zip_path: &Path) -> Result<usize, String> {
    use std::io::Read;
    let file = fs::File::open(zip_path).map_err(|e| format!("Архив не открыт: {e}"))?;
    let mut z = zip::ZipArchive::new(file).map_err(|e| format!("Это не zip-архив: {e}"))?;
    let mut manifest = String::new();
    z.by_name(MANIFEST)
        .map_err(|_| "В архиве нет списка файлов - это не копия Vaporust.".to_string())?
        .read_to_string(&mut manifest)
        .map_err(|e| e.to_string())?;
    // 1. Read all files of the backup. An error here - saves are untouched.
    let mut items: Vec<(PathBuf, Vec<u8>)> = Vec::new();
    for line in manifest.lines() {
        let Some((name, dest)) = line.split_once('\t') else { continue };
        let mut data = Vec::new();
        z.by_name(&format!("files/{name}"))
            .map_err(|e| format!("Копия повреждена, ничего не восстановлено: в архиве нет {name} ({e})."))?
            .read_to_end(&mut data)
            .map_err(|e| format!("Копия повреждена, ничего не восстановлено: {e}."))?;
        items.push((PathBuf::from(dest), data));
    }
    if items.is_empty() {
        return Err("В копии нет файлов.".into());
    }
    // 2. Read the previous contents of ALL targets before the first write. "File did not exist" only when
    // it really does not exist; a file that cannot be read (locked by the game, no access) stops
    // the restore before replacing: otherwise rollback would delete it as "never existed".
    let mut olds: Vec<Option<Vec<u8>>> = Vec::with_capacity(items.len());
    for (dest, _) in &items {
        match fs::read(dest) {
            Ok(d) => olds.push(Some(d)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => olds.push(None),
            Err(e) => {
                return Err(format!(
                    "Ничего не восстановлено: файл {} занят или недоступен ({e}). Закройте игру и Steam и попробуйте снова.",
                    dest.display()
                ))
            }
        }
    }
    // 3. Replace, remembering what is already replaced.
    let mut done: Vec<(PathBuf, Option<Vec<u8>>)> = Vec::new();
    let res = (|| -> Result<(), String> {
        for ((dest, data), old) in items.iter().zip(olds) {
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            write_replace(dest, data).map_err(|e| format!("Не восстановлен {}: {e}", dest.display()))?;
            done.push((dest.clone(), old));
        }
        Ok(())
    })();
    if let Err(e) = res {
        // 4. Rollback: put the replaced files back.
        let mut lost = Vec::new();
        for (dest, old) in done.into_iter().rev() {
            let ok = match old {
                Some(data) => write_replace(&dest, &data).is_ok(),
                None => fs::remove_file(&dest).is_ok(),
            };
            if !ok {
                lost.push(dest.display().to_string());
            }
        }
        return Err(if lost.is_empty() {
            format!("{e}. Сохранения возвращены как были.")
        } else {
            format!("{e}. Не удалось вернуть: {} - возьмите их из страховочной копии.", lost.join(", "))
        });
    }
    Ok(items.len())
}

/// Write a file via a temporary neighbor and rename: the file is never left truncated.
fn write_replace(dest: &Path, data: &[u8]) -> std::io::Result<()> {
    let mut tmp = dest.as_os_str().to_owned();
    tmp.push(".vaporust-tmp");
    let tmp = PathBuf::from(tmp);
    fs::write(&tmp, data)?;
    fs::rename(&tmp, dest).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })
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
