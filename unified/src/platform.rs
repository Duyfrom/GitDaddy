//! Isolated OS-specific helpers for macOS and Windows:
//! - Opening folders in Finder (macOS) or File Explorer (Windows)
//! - Opening a terminal in a repository folder or for GitHub CLI (`gh`) sign-in
//! - Moving folders to the Trash (macOS) or Recycle Bin (Windows)
//! - OS-specific UI labels (`⌘` vs `Ctrl+`, `Finder` vs `Explorer`, `Trash` vs `Recycle Bin`)

use std::fs;
use std::io;
use std::path::Path;
#[cfg(windows)]
use std::path::PathBuf;
use std::process::Command;

#[cfg(windows)]
use crate::git::hide_console;

#[cfg(target_os = "macos")]
pub const FILE_MANAGER_NAME: &str = "Finder";
#[cfg(not(target_os = "macos"))]
pub const FILE_MANAGER_NAME: &str = "File Explorer";

#[cfg(target_os = "macos")]
pub const LOCAL_COPY_TAB: &str = "On this Mac";
#[cfg(not(target_os = "macos"))]
pub const LOCAL_COPY_TAB: &str = "On this PC";

#[cfg(target_os = "macos")]
pub const TRASH_BUTTON_LABEL: &str = "Move to Trash";
#[cfg(not(target_os = "macos"))]
pub const TRASH_BUTTON_LABEL: &str = "Move to Recycle Bin";

#[cfg(target_os = "macos")]
pub const TRASH_NOUN: &str = "the Trash";
#[cfg(not(target_os = "macos"))]
pub const TRASH_NOUN: &str = "the Recycle Bin";

#[cfg(target_os = "macos")]
pub const MOD_KEY: &str = "⌘";
#[cfg(not(target_os = "macos"))]
pub const MOD_KEY: &str = "Ctrl+";

#[cfg(target_os = "macos")]
pub const MOD_SHIFT_KEY: &str = "⇧⌘";
#[cfg(not(target_os = "macos"))]
pub const MOD_SHIFT_KEY: &str = "Ctrl+Shift+";

/// Opens `path` in Finder on macOS or File Explorer on Windows.
pub fn open_in_explorer(path: &str) {
    #[cfg(target_os = "macos")]
    {
        let _ = Command::new("open").arg(path).spawn();
    }
    #[cfg(windows)]
    {
        let _ = Command::new("explorer.exe").arg(path).spawn();
    }
    #[cfg(all(not(windows), not(target_os = "macos")))]
    {
        let _ = Command::new("xdg-open").arg(path).spawn();
    }
}

/// Opens a terminal in `path`: Terminal.app (or iTerm) on macOS, Windows Terminal / PowerShell on Windows.
pub fn open_in_terminal(path: &str) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        let app = if Path::new("/Applications/iTerm.app").exists() { "iTerm" } else { "Terminal" };
        Command::new("open").args(["-a", app, path]).spawn().map(|_| ())
    }
    #[cfg(windows)]
    {
        if let Some(wt) = find_on_path("wt.exe") {
            let mut cmd = Command::new(wt);
            cmd.arg("-d").arg(path);
            hide_console(&mut cmd);
            if cmd.spawn().is_ok() {
                return Ok(());
            }
        }
        let shell = find_on_path("pwsh.exe").unwrap_or_else(|| PathBuf::from("powershell.exe"));
        let mut cmd = Command::new(shell);
        cmd.arg("-NoExit").current_dir(path);
        new_console(&mut cmd);
        cmd.spawn().map(|_| ())
    }
    #[cfg(all(not(windows), not(target_os = "macos")))]
    {
        Command::new("x-terminal-emulator").current_dir(path).spawn().map(|_| ())
    }
}

/// Opens a terminal window running `gh auth login --hostname github.com --web`.
pub fn open_gh_sign_in_terminal(gh: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let escaped_gh = gh.display().to_string().replace('\'', "'\\''");
        let shell_cmd = format!("'{escaped_gh}' auth login --hostname github.com --web");
        let apple_script = format!(
            "tell application \"Terminal\"\nactivate\ndo script \"{}\"\nend tell",
            shell_cmd.replace('\\', "\\\\").replace('"', "\\\"")
        );
        Command::new("osascript")
            .args(["-e", &apple_script])
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("Could not open Terminal.app: {e}"))
    }
    #[cfg(not(target_os = "macos"))]
    {
        let script =
            format!("& '{}' auth login --hostname github.com --web", gh.display().to_string().replace('\'', "''"));
        let mut cmd = Command::new("powershell.exe");
        cmd.args(["-NoExit", "-NoProfile", "-Command", &script]);
        new_console(&mut cmd);
        cmd.spawn().map(|_| ()).map_err(|e| format!("Could not open a terminal: {e}"))
    }
}

#[cfg(windows)]
pub fn new_console(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
    cmd.creation_flags(CREATE_NEW_CONSOLE);
}

#[cfg(windows)]
fn find_on_path(exe: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    std::env::split_paths(&path_var).map(|dir| dir.join(exe)).find(|candidate| candidate.is_file())
}

pub fn move_to_recycle_bin(path: &str) -> Result<(), String> {
    trash::delete(path).map_err(|e| e.to_string())
}

/// Deletes a folder for good. Git marks object files read-only, so clear that and retry on failure.
pub fn delete_permanently(path: &str) -> Result<(), String> {
    let path = Path::new(path);
    if fs::remove_dir_all(path).is_ok() {
        return Ok(());
    }
    clear_readonly(path);
    fs::remove_dir_all(path).map_err(|e| e.to_string())
}

fn clear_readonly(path: &Path) {
    let Ok(meta) = fs::symlink_metadata(path) else { return };
    if meta.is_dir()
        && !meta.file_type().is_symlink()
        && let Ok(entries) = fs::read_dir(path)
    {
        for entry in entries.flatten() {
            clear_readonly(&entry.path());
        }
    }
    let mut perms = meta.permissions();
    if perms.readonly() {
        #[allow(clippy::permissions_set_readonly_false)]
        perms.set_readonly(false);
        let _ = fs::set_permissions(path, perms);
    }
}
