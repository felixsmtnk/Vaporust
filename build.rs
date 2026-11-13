//! Ресурсы vaporust.exe для Windows: значок (assets/vaporust.ico, рисуется в src/icons.rs, тест
//! write_ico пересобирает .ico) и сведения о версии (название «Vaporust» видно в Проводнике,
//! Диспетчере задач и программам, которые ищут окна по названию приложения).

fn main() {
    println!("cargo:rerun-if-changed=assets/vaporust.ico");
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
    // «0.2.0-alpha» -> 0,2,0,0 (числовая версия ресурса).
    let nums: Vec<u16> = version
        .split(|c: char| c == '.' || c == '-')
        .take(3)
        .map(|p| p.parse().unwrap_or(0))
        .collect();
    let n = |i: usize| nums.get(i).copied().unwrap_or(0);
    let icon = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("assets")
        .join("vaporust.ico")
        .display()
        .to_string()
        .replace('\\', "\\\\");
    let rc = format!(
        r#"#include <winver.h>
1 ICON "{icon}"
1 VERSIONINFO
FILEVERSION {a},{b},{c},0
PRODUCTVERSION {a},{b},{c},0
FILEOS VOS_NT_WINDOWS32
FILETYPE VFT_APP
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "CompanyName", "Vaporust contributors"
      VALUE "FileDescription", "Vaporust"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "vaporust"
      VALUE "LegalCopyright", "GPL-3.0-or-later"
      VALUE "OriginalFilename", "vaporust.exe"
      VALUE "ProductName", "Vaporust"
      VALUE "ProductVersion", "{version}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#,
        a = n(0),
        b = n(1),
        c = n(2),
    );
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("vaporust.rc");
    std::fs::write(&out, rc).unwrap();
    let _ = embed_resource::compile(&out, embed_resource::NONE);
}
