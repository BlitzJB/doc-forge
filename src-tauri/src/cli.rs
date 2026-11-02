//! The `df` terminal command, installed from inside the app (like VS Code's `code`).
//!
//! The script ships in the bundle (Contents/Resources/df). Installing links it into a
//! PATH folder that comes before /bin, so it wins over the system's `df` (which it hands
//! everything else to): /opt/homebrew/bin when writable, else /usr/local/bin, asking for
//! an administrator password through the standard macOS prompt if needed. It never
//! replaces a `df` it didn't put there.

use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

/// Every Doc Forge `df` script carries this line.
const MARKER: &str = "open documents in Doc Forge";
const DIRS: [&str; 2] = ["/opt/homebrew/bin", "/usr/local/bin"];

#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CliStatus {
    /// Where Doc Forge's `df` is linked, if anywhere.
    pub installed_at: Option<String>,
    /// True when that link points at this copy of the app.
    pub current: bool,
    /// A `df` in one of the folders that isn't Doc Forge's (never touched).
    pub foreign: Option<String>,
}

fn is_ours(path: &Path) -> bool {
    fs::read_to_string(path).is_ok_and(|s| s.contains(MARKER))
}

pub fn status(script: &Path) -> CliStatus {
    status_in(script, &DIRS)
}

fn status_in(script: &Path, dirs: &[&str]) -> CliStatus {
    let mut out = CliStatus {
        installed_at: None,
        current: false,
        foreign: None,
    };
    for dir in dirs {
        let link = Path::new(dir).join("df");
        if fs::symlink_metadata(&link).is_err() {
            continue;
        }
        if is_ours(&link) {
            if out.installed_at.is_none() {
                out.current = fs::canonicalize(&link).ok() == fs::canonicalize(script).ok();
                out.installed_at = Some(link.to_string_lossy().into_owned());
            }
        } else if out.foreign.is_none() {
            out.foreign = Some(link.to_string_lossy().into_owned());
        }
    }
    out
}

fn writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".docforge-probe-{}", std::process::id()));
    let ok = fs::write(&probe, b"").is_ok();
    let _ = fs::remove_file(&probe);
    ok
}

/// Single-quote for /bin/sh.
fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

pub fn install(script: &Path) -> Result<String, String> {
    install_in(script, &DIRS)
}

fn install_in(script: &Path, dirs: &[&str]) -> Result<String, String> {
    if !is_ours(script) {
        return Err("The df script is missing from this copy of Doc Forge.".into());
    }
    // A df that isn't ours in either folder would be replaced or would hide ours: refuse.
    for dir in dirs {
        let link = Path::new(dir).join("df");
        if fs::symlink_metadata(&link).is_ok() && !is_ours(&link) {
            return Err(format!(
                "{} already has a df that isn't Doc Forge's, so it was left alone.",
                link.display()
            ));
        }
    }
    for dir in dirs {
        let d = Path::new(dir);
        let link = d.join("df");
        if d.is_dir() && writable(d) {
            if fs::symlink_metadata(&link).is_ok() {
                fs::remove_file(&link).map_err(|e| e.to_string())?;
            }
            std::os::unix::fs::symlink(script, &link).map_err(|e| e.to_string())?;
            return Ok(link.to_string_lossy().into_owned());
        }
    }
    if dirs != DIRS {
        return Err("No writable folder.".into());
    }
    // /usr/local/bin needs an administrator: the standard macOS password prompt.
    let link = Path::new(DIRS[1]).join("df");
    let cmd = format!(
        "mkdir -p {dir} && ln -sfn {src} {dst}",
        dir = sh_quote(DIRS[1]),
        src = sh_quote(&script.to_string_lossy()),
        dst = sh_quote(&link.to_string_lossy()),
    );
    let apple = format!(
        "do shell script \"{}\" with prompt \"Doc Forge wants to install the df command.\" with administrator privileges",
        cmd.replace('\\', "\\\\").replace('"', "\\\"")
    );
    let out = std::process::Command::new("/usr/bin/osascript")
        .args(["-e", &apple])
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(if err.contains("-128") {
            "Installation was cancelled.".into()
        } else {
            format!("Couldn't install df: {}", err.trim())
        });
    }
    Ok(link.to_string_lossy().into_owned())
}

pub fn uninstall() -> Result<(), String> {
    for dir in DIRS {
        let link = Path::new(dir).join("df");
        if fs::symlink_metadata(&link).is_ok() && is_ours(&link) {
            if writable(Path::new(dir)) {
                fs::remove_file(&link).map_err(|e| e.to_string())?;
            } else {
                let apple = format!(
                    "do shell script \"rm -f {}\" with administrator privileges",
                    sh_quote(&link.to_string_lossy())
                );
                let ok = std::process::Command::new("/usr/bin/osascript")
                    .args(["-e", &apple])
                    .status()
                    .is_ok_and(|s| s.success());
                if !ok {
                    return Err("Couldn't remove df.".into());
                }
            }
        }
    }
    Ok(())
}

/// The bundled script: Contents/Resources/df in the app; bin/df in development.
pub fn script(resource_dir: Option<PathBuf>) -> PathBuf {
    if let Some(p) = resource_dir.map(|d| d.join("df")).filter(|p| p.is_file()) {
        return p;
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../bin/df")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installs_links_and_never_touches_a_foreign_df() {
        let root = std::env::temp_dir().join(format!("forge-cli-{}", std::process::id()));
        let (a, b) = (root.join("a"), root.join("b"));
        fs::create_dir_all(&a).unwrap();
        fs::create_dir_all(&b).unwrap();
        let script = root.join("df");
        fs::write(&script, "#!/bin/sh\n# df: open documents in Doc Forge\n").unwrap();
        let dirs = [a.to_str().unwrap(), b.to_str().unwrap()];

        assert_eq!(status_in(&script, &dirs).installed_at, None);
        let at = install_in(&script, &dirs).unwrap();
        assert_eq!(at, a.join("df").to_string_lossy());
        let st = status_in(&script, &dirs);
        assert!(st.current && st.foreign.is_none());
        // Reinstalling replaces our own link.
        install_in(&script, &dirs).unwrap();

        // Someone else's df in either folder: refuse, and leave it be.
        fs::remove_file(a.join("df")).unwrap();
        fs::write(b.join("df"), "#!/bin/sh\necho theirs\n").unwrap();
        let err = install_in(&script, &dirs).unwrap_err();
        assert!(err.contains("isn't Doc Forge's"), "{err}");
        assert_eq!(
            fs::read_to_string(b.join("df")).unwrap(),
            "#!/bin/sh\necho theirs\n"
        );
        assert!(status_in(&script, &dirs).foreign.is_some());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn quotes_paths_with_spaces_and_quotes() {
        assert_eq!(
            sh_quote("/Applications/Doc Forge.app/x"),
            "'/Applications/Doc Forge.app/x'"
        );
        assert_eq!(sh_quote("a'b"), "'a'\\''b'");
    }
}
