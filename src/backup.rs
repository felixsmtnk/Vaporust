//! Save backups: a game's Steam Cloud files (where they live on this PC) are packed into
//! a zip in "Documents\Vaporust\Сохранения\<game>\<date>.zip". The archive contains a list
//! of the source paths (vaporust_backup.txt), used to restore the copy in place.
//! The program builds archives itself (the zip crate), no external programs.

use std::fs;
use std::path::{Path, PathBuf};

const MANIFEST: &str = "vaporust_backup.txt";

/// Folder with backups of all games.
pub fn root() -> PathBuf {
    crate::util::documents().join("Vaporust").join("Сохранения")
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
        return Err(tr!("Файлов сохранений на этом ПК нет - копировать нечего.", "There are no save files on this PC - nothing to copy.").into());
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
                Err(e) => Some(Err(trf!("Архив не создан: {e}", "Archive not created: {e}"))),
            }
        })
        .unwrap_or_else(|| Err(tr!("Архив не создан: слишком много копий за одну секунду.", "Archive not created: too many copies within one second.").into()))?;
    let r = (|| -> Result<(), String> {
        let mut z = zip::ZipWriter::new(file);
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let mut manifest = String::new();
        for (i, f) in files.iter().enumerate() {
            let name = format!("{i}_{}", f.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default());
            let data = fs::read(f).map_err(|e| trf!("Не прочитан {}: {e}", "Not read {}: {e}", f.display()))?;
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
    let r = backup_to(&game_dir(game), files);
    forget_list(); // the new archive shows in the list at once
    r
}

/// The cloud tab draws the list every frame: the folder is read at most every 3 s (and right
/// after a backup is made, see forget_list).
static LIST: std::sync::Mutex<Option<(String, std::time::Instant, Vec<(PathBuf, std::time::SystemTime, u64)>)>> = std::sync::Mutex::new(None);

pub fn forget_list() {
    *LIST.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// Backups of a game: (archive, modification time, size), newest first.
pub fn list(game: &str) -> Vec<(PathBuf, std::time::SystemTime, u64)> {
    if let Some((g, t, v)) = LIST.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        if g == game && t.elapsed() < std::time::Duration::from_secs(3) {
            return v.clone();
        }
    }
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
    *LIST.lock().unwrap_or_else(|e| e.into_inner()) = Some((game.to_string(), std::time::Instant::now(), v.clone()));
    v
}

/// Put files from a backup back in place. All or nothing: the archive is read and checked first
/// in full (a single file missing - nothing is touched), then files are replaced one by one, and on
/// an error midway the already replaced ones are restored. Returns the number of files.
/// Where a file from the backup goes: only to one of `allowed` (the save paths of this very game
/// on this PC). An exact path first; otherwise the allowed path with the longest common ending
/// (folder names + file name), so a backup made under another Windows user name still fits.
/// A path that matches nothing is refused: an archive from the internet may list anything
/// (the Startup folder, for example).
fn target_for(dest: &Path, allowed: &[PathBuf]) -> Option<PathBuf> {
    let low = |p: &Path| p.to_string_lossy().replace('/', "\\").to_lowercase();
    let d = low(dest);
    if let Some(a) = allowed.iter().find(|a| low(a) == d) {
        return Some(a.clone());
    }
    let comps = |s: &str| s.split('\\').filter(|c| !c.is_empty()).map(str::to_string).collect::<Vec<_>>();
    let dc = comps(&d);
    let score = |a: &PathBuf| comps(&low(a)).iter().rev().zip(dc.iter().rev()).take_while(|(x, y)| x == y).count();
    let best = allowed.iter().map(score).max().unwrap_or(0);
    // The file name alone is not enough: "slotA\save.dat" must not land in "slotB\save.dat".
    // At least the folder above it has to match too (unless the path is just a file name).
    if best == 0 || (best < 2 && dc.len() >= 2) {
        return None;
    }
    let mut hits = allowed.iter().filter(|a| score(a) == best);
    let first = hits.next()?.clone();
    // Two allowed files fit equally well - it is not clear which one is meant.
    if hits.next().is_some() {
        return None;
    }
    Some(first)
}

pub fn restore(zip_path: &Path, allowed: &[PathBuf]) -> Result<usize, String> {
    use std::io::Read;
    let file = fs::File::open(zip_path).map_err(|e| trf!("Архив не открыт: {e}", "Archive not opened: {e}"))?;
    let mut z = zip::ZipArchive::new(file).map_err(|e| trf!("Это не zip-архив: {e}", "This is not a zip archive: {e}"))?;
    let mut manifest = String::new();
    z.by_name(MANIFEST)
        .map_err(|_| tr!("В архиве нет списка файлов - это не копия Vaporust.", "The archive has no file list - this is not a Vaporust backup.").to_string())?
        .take(1 << 20)
        .read_to_string(&mut manifest)
        .map_err(|e| e.to_string())?;
    // Everything is unpacked into a temporary folder on the disk, not into memory: a large
    // backup does not fill the RAM. The folder is removed at the end whatever happens.
    let stage = Stage::new().map_err(|e| trf!("Ничего не восстановлено: нет временной папки ({e}).", "Nothing restored: no temporary folder ({e})."))?;
    // 1. Unpack all files of the backup. An error here - saves are untouched.
    // Limits: a strange archive (thousands of files, gigabytes unpacked, a "zip bomb") is refused.
    const MAX_FILES: usize = 5000;
    const MAX_TOTAL: u64 = 2 * 1024 * 1024 * 1024;
    let mut total: u64 = 0;
    let mut items: Vec<(PathBuf, PathBuf)> = Vec::new(); // (target, unpacked file)
    for line in manifest.lines() {
        let Some((name, dest)) = line.split_once('\t') else { continue };
        if items.len() >= MAX_FILES {
            return Err(tr!("Ничего не восстановлено: в копии слишком много файлов.", "Nothing restored: the backup has too many files.").into());
        }
        let Some(target) = target_for(Path::new(dest), allowed) else {
            return Err(trf!(
                "Ничего не восстановлено: файл {dest} из копии не относится к сохранениям этой игры.", "Nothing restored: file {dest} from the backup is not one of this game's saves."
            ));
        };
        // Each target once: two files of the backup going to one place would overwrite each other silently.
        if items.iter().any(|(t, _)| *t == target) {
            return Err(trf!(
                "Ничего не восстановлено: два файла копии подходят к одному месту ({}). Восстановите сохранения вручную из архива.",
                "Nothing restored: two files of the backup fit the same place ({}). Restore the saves by hand from the archive.",
                target.display()
            ));
        }
        let mut entry = z
            .by_name(&format!("files/{name}"))
            .map_err(|e| trf!("Копия повреждена, ничего не восстановлено: в архиве нет {name} ({e}).", "The backup is damaged, nothing restored: the archive has no {name} ({e})."))?;
        if suspicious_ratio(entry.size(), entry.compressed_size()) {
            return Err(tr!("Ничего не восстановлено: архив выглядит странно (слишком сильно сжат).", "Nothing restored: the archive looks strange (compressed far too much).").into());
        }
        let left = MAX_TOTAL - total;
        let staged = stage.path(items.len(), "new");
        let mut out = fs::File::create(&staged).map_err(|e| trf!("Ничего не восстановлено: {e}.", "Nothing restored: {e}."))?;
        // Read at most what is left of the budget: the size in the archive header can lie.
        let n = std::io::copy(&mut (&mut entry).take(left + 1), &mut out)
            .map_err(|e| trf!("Копия повреждена, ничего не восстановлено: {e}.", "The backup is damaged, nothing restored: {e}."))?;
        total += n;
        if total > MAX_TOTAL {
            return Err(tr!("Ничего не восстановлено: копия слишком большая.", "Nothing restored: the backup is too large.").into());
        }
        items.push((target, staged));
    }
    if items.is_empty() {
        return Err(tr!("В копии нет файлов.", "The backup has no files.").into());
    }
    // 2. Copy the previous contents of ALL targets aside before the first write. "File did not exist"
    // only when it really does not exist; a file that cannot be read (locked by the game, no access)
    // stops the restore before replacing: otherwise rollback would delete it as "never existed".
    let mut olds: Vec<Option<PathBuf>> = Vec::with_capacity(items.len());
    for (i, (dest, _)) in items.iter().enumerate() {
        match fs::File::open(dest) {
            Ok(mut f) => {
                let keep = stage.path(i, "old");
                let res = fs::File::create(&keep).and_then(|mut o| std::io::copy(&mut f, &mut o));
                if let Err(e) = res {
                    return Err(trf!(
                        "Ничего не восстановлено: файл {} не удалось прочитать ({e}).", "Nothing restored: file {} could not be read ({e}).",
                        dest.display()
                    ));
                }
                olds.push(Some(keep));
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => olds.push(None),
            Err(e) => {
                return Err(trf!(
                    "Ничего не восстановлено: файл {} занят или недоступен ({e}). Закройте игру и Steam и попробуйте снова.", "Nothing restored: file {} is busy or unavailable ({e}). Close the game and Steam and try again.",
                    dest.display()
                ))
            }
        }
    }
    // 3. Replace, remembering what is already replaced.
    let mut done: Vec<(PathBuf, Option<PathBuf>)> = Vec::new();
    let res = (|| -> Result<(), String> {
        for ((dest, staged), old) in items.iter().zip(olds) {
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            crate::util::copy_atomic(staged, dest).map_err(|e| trf!("Не восстановлен {}: {e}", "Not restored {}: {e}", dest.display()))?;
            done.push((dest.clone(), old));
        }
        Ok(())
    })();
    if let Err(e) = res {
        // 4. Rollback: put the replaced files back.
        let mut lost = Vec::new();
        for (dest, old) in done.into_iter().rev() {
            let ok = match old {
                Some(keep) => crate::util::copy_atomic(&keep, &dest).is_ok(),
                None => fs::remove_file(&dest).is_ok(),
            };
            if !ok {
                lost.push(dest.display().to_string());
            }
        }
        return Err(if lost.is_empty() {
            trf!("{e}. Сохранения возвращены как были.", "{e}. The saves were put back as they were.")
        } else {
            trf!("{e}. Не удалось вернуть: {} - возьмите их из страховочной копии.", "{e}. Could not put back: {} - take them from the safety backup.", lost.join(", "))
        });
    }
    Ok(items.len())
}

/// A "zip bomb": a small packed file that unpacks into something enormous. Real saves
/// compress maybe 10-50 times; more than 200 times on a large file is not a save.
fn suspicious_ratio(size: u64, compressed: u64) -> bool {
    size > (8 << 20) && size / compressed.max(1) > 200
}

/// A temporary folder for unpacking, removed when dropped.
struct Stage(PathBuf);

impl Stage {
    fn new() -> std::io::Result<Self> {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        let p = std::env::temp_dir().join(format!("vaporust-restore-{}-{t}", std::process::id()));
        fs::create_dir_all(&p)?;
        Ok(Stage(p))
    }
    fn path(&self, i: usize, kind: &str) -> PathBuf {
        self.0.join(format!("{i}.{kind}"))
    }
}

impl Drop for Stage {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zip_bomb_ratio() {
        assert!(!suspicious_ratio(1000, 1)); // tiny files are fine whatever the ratio
        assert!(!suspicious_ratio(50 << 20, 5 << 20)); // 10x is a normal save
        assert!(suspicious_ratio(1 << 30, 1 << 20)); // 1 GB from 1 MB is not
    }

    #[test]
    fn stage_folder_is_removed() {
        let p = {
            let s = Stage::new().unwrap();
            std::fs::write(s.path(0, "new"), b"x").unwrap();
            s.0.clone()
        };
        assert!(!p.exists());
    }

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
        let allowed = vec![a.clone(), b.clone()];
        assert_eq!(restore(&zip, &allowed).unwrap(), 2);
        assert_eq!(fs::read_to_string(&a).unwrap(), "первый");
        assert_eq!(fs::read_to_string(&b).unwrap(), "второй");
        // A path outside the save list is refused.
        assert!(restore(&zip, &[a.clone()]).is_err());
        assert_eq!(safe("A: B/C?"), "A_ B_C_");
        let _ = fs::remove_dir_all(&base);
    }

    /// Two slots with the same file name: when the game lists only one of them now, the other slot's
    /// file must not be written over it (it used to be, with "restored: 2").
    #[test]
    fn slots_do_not_overwrite_each_other() {
        let base = std::env::temp_dir().join(format!("vaporust_bk_slots_{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let a = base.join("saves").join("slotA").join("save.dat");
        let b = base.join("saves").join("slotB").join("save.dat");
        fs::create_dir_all(a.parent().unwrap()).unwrap();
        fs::create_dir_all(b.parent().unwrap()).unwrap();
        fs::write(&a, "A").unwrap();
        fs::write(&b, "B").unwrap();
        let (zip, _) = backup_to(&base.join("copies"), &[a.clone(), b.clone()]).unwrap();
        fs::write(&b, "B-now").unwrap();
        assert!(restore(&zip, &[b.clone()]).is_err());
        assert_eq!(fs::read_to_string(&b).unwrap(), "B-now", "nothing was written");
        // The same backup under another Windows user name still fits by the common ending.
        let other = base.join("Users").join("Other").join("saves").join("slotB").join("save.dat");
        assert_eq!(target_for(&b, std::slice::from_ref(&other)), Some(other.clone()));
        let _ = fs::remove_dir_all(&base);
    }
}

/// For tests: restore to the paths the archive itself lists (as if they were the game's save paths).
#[cfg(test)]
pub fn restore_any(zip_path: &Path) -> Result<usize, String> {
    use std::io::Read;
    let allowed: Vec<PathBuf> = (|| {
        let mut z = zip::ZipArchive::new(fs::File::open(zip_path).ok()?).ok()?;
        let mut m = String::new();
        z.by_name(MANIFEST).ok()?.read_to_string(&mut m).ok()?;
        Some(m.lines().filter_map(|l| l.split_once('\t')).map(|(_, d)| PathBuf::from(d)).collect())
    })()
    .unwrap_or_default();
    restore(zip_path, &allowed)
}
