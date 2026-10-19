use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::git::hide_console;

pub fn open_in_explorer(path: &str) {
    let _ = Command::new("explorer.exe").arg(path).spawn();
}

/// Opens a terminal in `path`: Windows Terminal when installed, otherwise PowerShell.
pub fn open_in_terminal(path: &str) -> io::Result<()> {
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

#[cfg(windows)]
pub fn new_console(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
    cmd.creation_flags(CREATE_NEW_CONSOLE);
}

#[cfg(not(windows))]
pub fn new_console(_cmd: &mut Command) {}

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
