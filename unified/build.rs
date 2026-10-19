//! Embeds the app icon, version info, and manifest into GitDaddy.exe.
//! Needs a resource compiler (rc.exe from the Windows SDK, or windres); without one the
//! build still succeeds and the exe simply uses the default icon.

use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=assets/GitDaddy.ico");
    println!("cargo:rerun-if-changed=assets/GitDaddy.manifest");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR");
    let asset =
        |name: &str| Path::new(&manifest_dir).join("assets").join(name).display().to_string().replace('\\', "\\\\");

    let version = std::env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION");
    let mut parts: Vec<u16> = version.split(['.', '-', '+']).filter_map(|p| p.parse().ok()).take(3).collect();
    parts.resize(4, 0);
    let numeric = parts.iter().map(u16::to_string).collect::<Vec<_>>().join(",");

    let rc = format!(
        r#"#pragma code_page(65001)
1 ICON "{icon}"
{manifest}1 VERSIONINFO
FILEVERSION {numeric}
PRODUCTVERSION {numeric}
FILEOS 0x40004
FILETYPE 0x1
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "CompanyName", "GitDaddy"
      VALUE "FileDescription", "GitDaddy - Git Repository Update Manager"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "GitDaddy"
      VALUE "LegalCopyright", "MIT License"
      VALUE "OriginalFilename", "GitDaddy.exe"
      VALUE "ProductName", "GitDaddy"
      VALUE "ProductVersion", "{version}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#,
        icon = asset("GitDaddy.ico"),
        // MinGW links its own default manifest, and two manifests cannot be merged.
        manifest = if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
            format!("1 24 \"{}\"\n", asset("GitDaddy.manifest"))
        } else {
            String::new()
        },
    );
    let rc_path = Path::new(&out_dir).join("GitDaddy.rc");
    std::fs::write(&rc_path, rc).expect("write resource script");

    if let Err(err) = embed_resource::compile(&rc_path, embed_resource::NONE).manifest_optional() {
        println!("cargo:warning=Could not embed the icon and version resources: {err}");
    }
}
