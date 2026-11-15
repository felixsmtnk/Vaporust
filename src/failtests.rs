//! Тесты на сбои: нехватка места, обрыв загрузки, испорченные файлы Steam и архивы.
//! Главное, что проверяется: программа не падает и ничего не портит и не удаляет, если
//! что-то пошло не так.

#![cfg(test)]

use std::fs;
use std::path::PathBuf;

/// Своя временная папка на тест (удаляется в конце).
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let p = std::env::temp_dir().join(format!("vaporust_fail_{name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }
    fn file(&self, name: &str, data: &[u8]) -> PathBuf {
        let p = self.0.join(name);
        if let Some(d) = p.parent() {
            fs::create_dir_all(d).unwrap();
        }
        fs::write(&p, data).unwrap();
        p
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const ACF: &str = "\"AppState\"\n{\n\t\"appid\"\t\t\"70\"\n\t\"StateFlags\"\t\t\"4\"\n\t\"installdir\"\t\t\"Half-Life\"\n\
\t\"SizeOnDisk\"\t\t\"1000\"\n\t\"buildid\"\t\t\"42\"\n\t\"InstalledDepots\"\n\t{\n\t\t\"1\"\n\t\t{\n\t\t\t\"manifest\"\t\t\"111\"\n\
\t\t\t\"size\"\t\t\"900\"\n\t\t}\n\t}\n\t\"UserConfig\"\n\t{\n\t\t\"language\"\t\t\"english\"\n\t}\n}\n";

/// Всевозможный мусор вместо файла.
fn junk_variants() -> Vec<Vec<u8>> {
    let mut v: Vec<Vec<u8>> = vec![
        Vec::new(),
        b"\0\0\0\0".to_vec(),
        vec![0xff, 0xfe, 0x00, 0xd8],
        b"{{{{{{{{".to_vec(),
        b"}}}}}}}}".to_vec(),
        b"\"AppState\"\n{\n\t\"appid\"".to_vec(),
        "\u{feff}\"AppState\"\n{\n\t\"StateFlags\"\t\t\"не число\"\n}\n".as_bytes().to_vec(),
    ];
    // Обрезанный посередине настоящий файл (оборвалась запись).
    for n in 1..8 {
        v.push(ACF.as_bytes()[..ACF.len() * n / 8].to_vec());
    }
    v
}

#[test]
fn not_enough_space() {
    const GB: u64 = 1 << 30;
    // 10 ГБ игры, свободно 5 - отказ с понятным текстом.
    let e = crate::net::space_error(10 * GB, 0, Some(5 * GB)).expect("должен быть отказ");
    assert!(e.contains("Не хватает места") && e.contains("10.00 ГБ") && e.contains("5.00 ГБ"), "{e}");
    // Продолжение: 8 ГБ уже скачано, нужно еще 2 - хватает.
    assert!(crate::net::space_error(10 * GB, 8 * GB, Some(5 * GB)).is_none());
    // Ровно впритык - хватает; узнать свободное место не удалось - не мешаем.
    assert!(crate::net::space_error(5 * GB, 0, Some(5 * GB)).is_none());
    assert!(crate::net::space_error(10 * GB, 0, None).is_none());
    // Скачано больше, чем весит игра (обновление уменьшило ее) - не переполнение.
    assert!(crate::net::space_error(GB, 3 * GB, Some(0)).is_none());
}

#[test]
fn download_interrupted() {
    use crate::should_resume;
    // Обрыв связи - продолжаем сами, но не больше 3 раз подряд.
    for e in [
        "operation timed out",
        "error sending request: connection reset by peer",
        "Не удалось начать установку: CDN unavailable",
        "dns error: no such host",
        "Нет связи со Steam",
    ] {
        assert!(should_resume(e, false, 0), "{e}");
        assert!(!should_resume(e, false, 3), "{e}: после 3 попыток хватит");
        assert!(!should_resume(e, true, 0), "{e}: пользователь сам остановил");
    }
    // Не сеть - не повторяем (иначе крутилось бы зря).
    for e in [
        "Не хватает места на диске: нужно 10.00 ГБ, свободно 5.00 ГБ.",
        "There is not enough space on the disk. (os error 112)",
        "Access is denied. (os error 5)",
        "Установка отменена. Скачанное сохранено, ее можно продолжить.",
        "app not owned",
    ] {
        assert!(!should_resume(e, false, 0), "{e}");
    }
}

#[test]
fn broken_appmanifest() {
    let t = TempDir::new("acf");
    // Нет файла.
    let missing = t.0.join("appmanifest_1.acf");
    let info = crate::local::read_acf(&missing);
    assert_eq!(info.state_flags, 0);
    assert!(crate::local::acf_top(&missing, "installdir").is_none());
    assert!(crate::local::set_acf_top(&missing, "AutoUpdateBehavior", "1").is_err());
    assert!(!missing.exists(), "запись в отсутствующий файл не создает его");
    // Мусор и обрезанные файлы: чтение не падает.
    for (i, junk) in junk_variants().into_iter().enumerate() {
        let p = t.file(&format!("appmanifest_{i}.acf"), &junk);
        let _ = crate::local::read_acf(&p);
        let _ = crate::local::installed_depots(&p);
        let _ = crate::local::installed_depot_sizes(&p);
        let _ = crate::local::shared_depots(&p);
        let _ = crate::local::acf_buildid(&p);
        let _ = crate::local::acf_top(&p, "installdir");
    }
    // Целый файл читается правильно.
    let p = t.file("appmanifest_70.acf", ACF.as_bytes());
    let info = crate::local::read_acf(&p);
    assert_eq!(info.state_flags, 4);
    assert_eq!(info.size_on_disk, 1000);
    assert_eq!(info.user_language, "english");
    assert_eq!(crate::local::installed_depots(&p).get(&1), Some(&111));
    assert_eq!(crate::local::acf_buildid(&p).as_deref(), Some("42"));
}

#[test]
fn broken_libraryfolders() {
    let t = TempDir::new("libs");
    let steam = t.0.join("Steam");
    for junk in junk_variants() {
        t.file("Steam/steamapps/libraryfolders.vdf", &junk);
        let dirs = crate::local::library_dirs(&steam);
        assert_eq!(dirs.first(), Some(&steam), "папка самого Steam есть всегда");
        let _ = crate::local::scan_games(&steam);
    }
}

#[test]
fn broken_localconfig_is_not_overwritten() {
    let t = TempDir::new("localconfig");
    let good = "\"UserLocalConfigStore\"\n{\n\t\"Software\"\n\t{\n\t\t\"Valve\"\n\t\t{\n\t\t\t\"Steam\"\n\t\t\t{\n\t\t\t\t\"apps\"\n\t\t\t\t{\n\
                \t\t\t\t\t\"70\"\n\t\t\t\t\t{\n\t\t\t\t\t\t\"LaunchOptions\"\t\t\"-gl\"\n\t\t\t\t\t}\n\t\t\t\t}\n\t\t\t}\n\t\t}\n\t}\n}\n";
    // Файл оборвался до конца раздела apps: правка отказывается и файл не трогает.
    let cut = &good[..good.find("\"70\"").unwrap()];
    let p = t.file("localconfig.vdf", cut.as_bytes());
    let _ = crate::launchopts::read(&p, 70);
    let _ = crate::launchopts::read_last_played(&p);
    assert!(crate::launchopts::write(&p, 70, "-novid").is_err());
    assert_eq!(fs::read_to_string(&p).unwrap(), cut, "испорченный файл не переписан");
    // Мусор - тоже без падений и без записи.
    for junk in junk_variants() {
        let p = t.file("junk.vdf", &junk);
        let _ = crate::launchopts::read(&p, 70);
        let _ = crate::launchopts::read_last_played(&p);
        let r = crate::launchopts::write(&p, 70, "-novid");
        if r.is_err() {
            assert_eq!(fs::read(&p).unwrap(), junk, "при ошибке файл не изменен");
        }
    }
}

#[test]
fn broken_shortcuts_vdf() {
    use crate::shortcuts::Node;
    let root = vec![(
        "shortcuts".to_string(),
        Node::Map(vec![(
            "0".to_string(),
            Node::Map(vec![
                ("appid".into(), Node::Int(3_000_000_123)),
                ("AppName".into(), Node::Str("Игра".into())),
                ("Exe".into(), Node::Str("\"C:\\Games\\game.exe\"".into())),
            ]),
        )]),
    )];
    let bytes = crate::shortcuts::write(&root);
    assert!(crate::shortcuts::parse(&bytes).is_ok());
    // Любой обрезанный кусок - ошибка, а не падение и не «пустой список» (иначе при добавлении
    // ярлыка мы бы переписали файл без старых ярлыков).
    for n in 0..bytes.len() {
        assert!(crate::shortcuts::parse(&bytes[..n]).is_err(), "обрезано до {n} байт");
    }
    let t = TempDir::new("shortcuts");
    let p = t.file("shortcuts.vdf", &bytes[..bytes.len() / 2]);
    assert!(crate::shortcuts::read(&p).is_err());
    assert!(crate::shortcuts::add(&p, "Новая", "\"C:\\new.exe\"", "").is_err());
    assert_eq!(fs::read(&p).unwrap(), &bytes[..bytes.len() / 2], "испорченный файл не переписан");
}

fn zip_with(files: &[(&str, &[u8])]) -> Vec<u8> {
    use std::io::Write;
    let mut z = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, data) in files {
        z.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
        z.write_all(data).unwrap();
    }
    z.finish().unwrap().into_inner()
}

#[test]
fn broken_save_backup() {
    let t = TempDir::new("backup");
    let save = t.file("save/slot1.sav", b"progress");
    // Испорченный архив, архив без списка файлов, пустой файл - отказ, сохранения не тронуты.
    for (name, data) in [
        ("junk.zip", b"not a zip at all".to_vec()),
        ("empty.zip", Vec::new()),
        ("nomanifest.zip", zip_with(&[("slot1.sav", b"old")])),
    ] {
        let z = t.file(name, &data);
        assert!(crate::backup::restore(&z).is_err(), "{name}");
        assert_eq!(fs::read(&save).unwrap(), b"progress", "{name}");
    }
    // Копия папки без файлов - понятный отказ, не пустой архив.
    let missing = vec![t.0.join("nothing.sav")];
    assert!(crate::backup::backup_to(&t.0.join("copies"), &missing).is_err());
    // Копия в недоступное место (вместо папки - файл) - ошибка, а не тишина.
    let blocker = t.file("blocker", b"x");
    assert!(crate::backup::backup_to(&blocker, &[save.clone()]).is_err());
    // Обычная копия и восстановление работают.
    let (zip, n) = crate::backup::backup_to(&t.0.join("copies"), &[save.clone()]).unwrap();
    assert_eq!(n, 1);
    fs::write(&save, b"broken").unwrap();
    assert_eq!(crate::backup::restore(&zip).unwrap(), 1);
    assert_eq!(fs::read(&save).unwrap(), b"progress");
}

#[test]
fn broken_update_archive() {
    let t = TempDir::new("update");
    let exe = t.file("vaporust.exe", b"current version");
    let me = fs::read(std::env::current_exe().unwrap()).unwrap();
    let other_arch = {
        // Тот же exe, но с пометкой «32 бит» вместо «64» (или наоборот).
        let mut x = me.clone();
        let pe = u32::from_le_bytes(x[0x3c..0x40].try_into().unwrap()) as usize;
        let m = if cfg!(target_pointer_width = "64") { 0x014cu16 } else { 0x8664u16 };
        x[pe + 4..pe + 6].copy_from_slice(&m.to_le_bytes());
        x
    };
    for (what, data) in [
        ("мусор", b"garbage".to_vec()),
        ("пусто", Vec::new()),
        ("нет exe", zip_with(&[("README.md", b"hi")])),
        ("не программа", zip_with(&[("vaporust.exe", b"text, not a program")])),
        ("другая разрядность", zip_with(&[("vaporust.exe", &other_arch)])),
    ] {
        assert!(crate::updater::replace_exe(&exe, &data).is_err(), "{what}");
        assert_eq!(fs::read(&exe).unwrap(), b"current version", "{what}: программа не тронута");
        assert!(!exe.with_extension("exe.old").exists(), "{what}");
    }
    // Настоящий exe нашей разрядности ставится.
    crate::updater::replace_exe(&exe, &zip_with(&[("Vaporust/vaporust.exe", &me)])).unwrap();
    assert_eq!(fs::read(&exe).unwrap().len(), me.len());
    assert_eq!(fs::read(exe.with_extension("exe.old")).unwrap(), b"current version");
}

#[test]
fn staging_conflict_keeps_files() {
    // Во временной папке остался кусок прошлой попытки: перенос отказывается и ничего не теряет.
    let t = TempDir::new("staging");
    let lib = t.0.clone();
    let acf = "\"AppState\"\n{\n\t\"installdir\"\t\t\"Game\"\n}\n";
    t.file("steamapps/appmanifest_60.acf", acf.as_bytes());
    t.file("steamapps/common/Game/data.bin", b"new");
    let st = crate::local::staging_dir(&lib).join("steamapps");
    fs::create_dir_all(st.join("common").join("Game")).unwrap();
    fs::write(st.join("common").join("Game").join("data.bin"), b"old").unwrap();
    assert!(crate::local::to_staging(&lib, 60).is_err());
    assert_eq!(fs::read(lib.join("steamapps/common/Game/data.bin")).unwrap(), b"new");
    assert!(lib.join("steamapps/appmanifest_60.acf").exists(), "appmanifest на месте");
    // Обратный перенос, когда в библиотеке уже есть такая папка, - тоже отказ без потерь.
    fs::write(st.join("appmanifest_60.acf"), acf).unwrap();
    assert!(crate::local::unstage(&lib, 60).is_err());
    assert_eq!(fs::read(st.join("common/Game/data.bin")).unwrap(), b"old");
    assert_eq!(fs::read(lib.join("steamapps/common/Game/data.bin")).unwrap(), b"new");
    // Временной папки нет совсем.
    let empty = TempDir::new("staging_empty");
    assert!(crate::local::unstage(&empty.0, 60).is_err());
}

#[test]
fn restore_is_all_or_nothing() {
    let t = TempDir::new("restore_atomic");
    let a = t.file("save/a.sav", b"A-now");
    let b = t.file("save/b.sav", b"B-now");
    // В списке два файла, а в архиве только первый: ничего не заменяется.
    let manifest = format!("0_a.sav\t{}\n1_b.sav\t{}\n", a.display(), b.display());
    let z = t.file(
        "partial.zip",
        &zip_with(&[("files/0_a.sav", b"A-old"), ("vaporust_backup.txt", manifest.as_bytes())]),
    );
    assert!(crate::backup::restore(&z).is_err());
    assert_eq!(fs::read(&a).unwrap(), b"A-now", "первый файл не тронут");
    assert_eq!(fs::read(&b).unwrap(), b"B-now");
    // На месте второго файла папка (не читается): отказ ДО записи, первый не тронут.
    let blocked = t.0.join("save").join("dir.sav");
    fs::create_dir_all(blocked.join("inner")).unwrap();
    let manifest = format!("0_a.sav\t{}\n1_d.sav\t{}\n", a.display(), blocked.display());
    let z = t.file(
        "blocked.zip",
        &zip_with(&[("files/0_a.sav", b"A-old"), ("files/1_d.sav", b"D-old"), ("vaporust_backup.txt", manifest.as_bytes())]),
    );
    let e = crate::backup::restore(&z).unwrap_err();
    assert!(e.contains("Ничего не восстановлено"), "{e}");
    assert_eq!(fs::read(&a).unwrap(), b"A-now");
    // Сбой при записи второго файла (он только для чтения): первый возвращается как был.
    let ro = t.file("save/ro.sav", b"RO-now");
    let mut perm = fs::metadata(&ro).unwrap().permissions();
    perm.set_readonly(true);
    fs::set_permissions(&ro, perm.clone()).unwrap();
    let manifest = format!("0_a.sav\t{}\n1_r.sav\t{}\n", a.display(), ro.display());
    let z = t.file(
        "readonly.zip",
        &zip_with(&[("files/0_a.sav", b"A-old"), ("files/1_r.sav", b"RO-old"), ("vaporust_backup.txt", manifest.as_bytes())]),
    );
    let e = crate::backup::restore(&z).unwrap_err();
    assert!(e.contains("возвращены как были"), "{e}");
    assert_eq!(fs::read(&a).unwrap(), b"A-now", "откат первого файла");
    assert_eq!(fs::read(&ro).unwrap(), b"RO-now");
    assert!(!t.0.join("save").join("a.sav.vaporust-tmp").exists(), "временных файлов не осталось");
    // Новый файл, которого раньше не было, при откате удаляется.
    let fresh = t.0.join("save").join("new.sav");
    let manifest = format!("0_n.sav\t{}\n1_r.sav\t{}\n", fresh.display(), ro.display());
    let z = t.file(
        "fresh.zip",
        &zip_with(&[("files/0_n.sav", b"N"), ("files/1_r.sav", b"R"), ("vaporust_backup.txt", manifest.as_bytes())]),
    );
    assert!(crate::backup::restore(&z).is_err());
    assert!(!fresh.exists());
    #[allow(clippy::permissions_set_readonly_false)]
    perm.set_readonly(false);
    fs::set_permissions(&ro, perm).unwrap();
}

/// QA-01: файл, который нельзя прочитать (игра держит его открытым без общего чтения), не должен
/// считаться «несуществующим» и удаляться при откате.
#[cfg(windows)]
#[test]
fn restore_keeps_unreadable_file() {
    use std::os::windows::fs::OpenOptionsExt;
    let t = TempDir::new("restore_locked");
    let a = t.file("save/a.sav", b"A-now");
    let b_dir = t.0.join("save").join("b.sav");
    fs::create_dir_all(&b_dir).unwrap();
    let manifest = format!("0_a.sav\t{}\n1_b.sav\t{}\n", a.display(), b_dir.display());
    let z = t.file(
        "locked.zip",
        &zip_with(&[("files/0_a.sav", b"A-old"), ("files/1_b.sav", b"B"), ("vaporust_backup.txt", manifest.as_bytes())]),
    );
    // Чтение запрещено, запись и удаление разрешены (FILE_SHARE_WRITE | FILE_SHARE_DELETE).
    let lock = fs::OpenOptions::new().read(true).share_mode(0x2 | 0x4).open(&a).unwrap();
    let e = crate::backup::restore(&z).unwrap_err();
    drop(lock);
    assert!(e.contains("Ничего не восстановлено"), "{e}");
    assert_eq!(fs::read(&a).unwrap(), b"A-now", "файл на месте и не изменен");
}

#[test]
fn backups_in_one_second_do_not_overwrite() {
    let t = TempDir::new("backup_names");
    let save = t.file("s.sav", b"first");
    let dir = t.0.join("copies");
    let (z1, _) = crate::backup::backup_to(&dir, &[save.clone()]).unwrap();
    fs::write(&save, b"second").unwrap();
    let (z2, _) = crate::backup::backup_to(&dir, &[save.clone()]).unwrap();
    fs::write(&save, b"third").unwrap();
    let (z3, _) = crate::backup::backup_to(&dir, &[save.clone()]).unwrap();
    assert!(z1 != z2 && z2 != z3 && z1 != z3, "{z1:?} {z2:?} {z3:?}");
    crate::backup::restore(&z1).unwrap();
    assert_eq!(fs::read(&save).unwrap(), b"first", "первая копия не затерта второй");
    crate::backup::restore(&z2).unwrap();
    assert_eq!(fs::read(&save).unwrap(), b"second");
}
