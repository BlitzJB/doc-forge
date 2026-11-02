//! Opening files from outside the window: the `df` command, Finder's "Open With",
//! and a second launch of the app (which hands its arguments to the running one).
//!
//! Paths are queued and the frontend is pinged; it drains the queue when it is ready,
//! so nothing is lost if a path arrives before the page has loaded.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, Runtime};

#[derive(Default)]
pub struct Pending(Mutex<Vec<PathBuf>>);

/// File arguments from a command line: skips the executable and flags (macOS can add
/// `-psn_…`), and resolves relative paths against the caller's working directory.
pub fn paths_from_args(args: &[String], cwd: &Path) -> Vec<PathBuf> {
    args.iter()
        .skip(1)
        .filter(|a| !a.is_empty() && !a.starts_with('-'))
        .map(|a| {
            let p = PathBuf::from(a);
            if p.is_absolute() {
                p
            } else {
                cwd.join(p)
            }
        })
        .collect()
}

pub fn open_paths<R: Runtime>(app: &AppHandle<R>, paths: Vec<PathBuf>) {
    if paths.is_empty() {
        return;
    }
    if let Some(pending) = app.try_state::<Pending>() {
        pending.0.lock().unwrap().extend(paths);
    }
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
    let _ = app.emit("open-paths", ());
}

/// Paths waiting to be opened, oldest first. Each is returned once.
#[tauri::command]
pub fn take_opens(pending: tauri::State<'_, Pending>) -> Vec<String> {
    std::mem::take(&mut *pending.0.lock().unwrap())
        .into_iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_files_and_resolves_relative_paths() {
        let args = ["/x/doc-forge", "-psn_0_123", "notes.md", "/abs/plan.md", ""]
            .map(String::from)
            .to_vec();
        assert_eq!(
            paths_from_args(&args, Path::new("/home/me")),
            vec![
                PathBuf::from("/home/me/notes.md"),
                PathBuf::from("/abs/plan.md")
            ]
        );
    }
}
