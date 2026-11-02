//! Shadow history for documents opened in Doc Forge.
//!
//! Every document gets its own bare git repository inside Doc Forge's data folder.
//! Snapshots are written with git *plumbing* only (`hash-object`, `mktree`,
//! `commit-tree`, `update-ref`), fed from the file's bytes. Git is never pointed at
//! the user's directory, so their repository, hooks, `.gitignore`, `.gitattributes`
//! and config are never read or touched. The only shared thing is the file itself.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

const BRANCH: &str = "refs/heads/main";
const ZERO: &str = "0000000000000000000000000000000000000000";
pub const FORGE_EMAIL: &str = "forge@docforge.local";
pub const YOU_EMAIL: &str = "you@docforge.local";

// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct Error(pub String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

fn err<T>(msg: impl Into<String>) -> Result<T> {
    Err(Error(msg.into()))
}

// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Author {
    You,
    Agent,
}

impl Author {
    fn ident(self) -> (&'static str, &'static str) {
        match self {
            Author::You => ("You", YOU_EMAIL),
            Author::Agent => ("Forge", FORGE_EMAIL),
        }
    }
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Version {
    pub sha: String,
    pub parent: Option<String>,
    pub author: Author,
    /// Commit subject.
    pub label: String,
    /// Commit body after the subject (e.g. trailers), possibly empty.
    pub body: String,
    /// Milliseconds since the Unix epoch.
    pub created_at: i64,
    pub content: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DocEntry {
    pub id: String,
    pub path: PathBuf,
    pub opened_at: i64,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct OpenedDoc {
    pub id: String,
    pub path: PathBuf,
    pub name: String,
    pub folder: String,
    pub content: String,
    pub history: Vec<Version>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RecentDoc {
    pub id: String,
    pub path: PathBuf,
    pub name: String,
    pub folder: String,
    pub exists: bool,
    pub opened_at: i64,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Synced {
    /// True when the file changed on disk since the last snapshot.
    pub changed: bool,
    pub content: String,
    pub history: Vec<Version>,
}

// ---------------------------------------------------------------------------

pub struct Store {
    root: PathBuf,
    index: Mutex<Vec<DocEntry>>,
    locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
}

impl Store {
    /// `root` is Doc Forge's private data folder (never a user directory).
    pub fn new(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        fs::create_dir_all(root.join("repos"))?;
        let index_path = root.join("documents.json");
        let index = match fs::read(&index_path) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
            Err(_) => Vec::new(),
        };
        Ok(Store {
            root,
            index: Mutex::new(index),
            locks: Mutex::new(HashMap::new()),
        })
    }

    fn repo(&self, id: &str) -> PathBuf {
        self.root.join("repos").join(format!("{id}.git"))
    }

    fn lock(&self, id: &str) -> Arc<Mutex<()>> {
        self.locks
            .lock()
            .unwrap()
            .entry(id.to_string())
            .or_default()
            .clone()
    }

    fn save_index(&self, index: &[DocEntry]) -> Result<()> {
        let tmp = self.root.join("documents.json.tmp");
        fs::write(
            &tmp,
            serde_json::to_vec_pretty(index).map_err(|e| Error(e.to_string()))?,
        )?;
        fs::rename(tmp, self.root.join("documents.json"))?;
        Ok(())
    }

    fn entry(&self, id: &str) -> Result<DocEntry> {
        match self.index.lock().unwrap().iter().find(|e| e.id == id) {
            Some(e) => Ok(e.clone()),
            None => err(format!("unknown document {id}")),
        }
    }

    pub fn recent(&self) -> Vec<RecentDoc> {
        let mut docs: Vec<RecentDoc> = self
            .index
            .lock()
            .unwrap()
            .iter()
            .map(|e| {
                let (name, folder) = names(&e.path);
                RecentDoc {
                    id: e.id.clone(),
                    path: e.path.clone(),
                    name,
                    folder,
                    exists: e.path.is_file(),
                    opened_at: e.opened_at,
                }
            })
            .collect();
        docs.sort_by(|a, b| b.opened_at.cmp(&a.opened_at));
        docs
    }

    /// Open (and start tracking) a file. Re-opening keeps its history; a file that was
    /// moved while Doc Forge was closed is matched back to its history by content.
    pub fn open(&self, path: &Path) -> Result<OpenedDoc> {
        let path = fs::canonicalize(path)?;
        let content = read_text(&path)?;

        let id = {
            let mut index = self.index.lock().unwrap();
            let found = index.iter().position(|e| e.path == path).or_else(|| {
                // Moved or renamed: an entry whose old path is gone and whose latest
                // snapshot has exactly this content.
                let blob = git(
                    None,
                    &self.root,
                    &["hash-object", "--stdin"],
                    Some(content.as_bytes()),
                    &[],
                )
                .ok()?;
                let blob = String::from_utf8_lossy(&blob).trim().to_string();
                index.iter().position(|e| {
                    !e.path.exists()
                        && head(&self.repo(&e.id))
                            .ok()
                            .flatten()
                            .and_then(|h| tree_blob(&self.repo(&e.id), &h).ok())
                            .as_deref()
                            == Some(blob.as_str())
                })
            });
            let at = now_ms();
            let id = match found {
                Some(i) => {
                    index[i].path = path.clone();
                    index[i].opened_at = at;
                    index[i].id.clone()
                }
                None => {
                    let id = new_id();
                    init_repo(&self.root, &self.repo(&id))?;
                    index.push(DocEntry {
                        id: id.clone(),
                        path: path.clone(),
                        opened_at: at,
                    });
                    id
                }
            };
            self.save_index(&index)?;
            id
        };

        let lock = self.lock(&id);
        let _guard = lock.lock().unwrap();
        let repo = self.repo(&id);
        let message = if head(&repo)?.is_none() {
            "Started tracking in Doc Forge"
        } else {
            "Edited outside Doc Forge"
        };
        snapshot(&repo, &file_name(&path), &content, message, Author::You)?;
        let (name, folder) = names(&path);
        Ok(OpenedDoc {
            id: id.clone(),
            path,
            name,
            folder,
            content,
            history: history(&repo)?,
        })
    }

    /// Write the document to disk and record a snapshot (skipped if nothing changed).
    pub fn save(
        &self,
        id: &str,
        content: &str,
        message: &str,
        author: Author,
    ) -> Result<Vec<Version>> {
        let entry = self.entry(id)?;
        let lock = self.lock(id);
        let _guard = lock.lock().unwrap();
        let repo = self.repo(id);
        if read_text(&entry.path).ok().as_deref() != Some(content) {
            fs::write(&entry.path, content)?;
        }
        snapshot(&repo, &file_name(&entry.path), content, message, author)?;
        history(&repo)
    }

    /// Pick up edits made to the file outside Doc Forge.
    pub fn sync(&self, id: &str) -> Result<Synced> {
        let entry = self.entry(id)?;
        let lock = self.lock(id);
        let _guard = lock.lock().unwrap();
        let repo = self.repo(id);
        let content = read_text(&entry.path)?;
        let changed = snapshot(
            &repo,
            &file_name(&entry.path),
            &content,
            "Edited outside Doc Forge",
            Author::You,
        )?
        .is_some();
        Ok(Synced {
            changed,
            content,
            history: history(&repo)?,
        })
    }

    /// Write an earlier version back to the file; recorded as a new snapshot.
    pub fn restore(&self, id: &str, sha: &str, message: &str) -> Result<Synced> {
        let entry = self.entry(id)?;
        let lock = self.lock(id);
        let _guard = lock.lock().unwrap();
        let repo = self.repo(id);
        if !is_hex_sha(sha) {
            return err("invalid version id");
        }
        let blob = tree_blob(&repo, sha)?;
        let bytes = git(Some(&repo), &repo, &["cat-file", "blob", &blob], None, &[])?;
        let content =
            String::from_utf8(bytes).map_err(|_| Error("version is not UTF-8 text".into()))?;
        fs::write(&entry.path, &content)?;
        snapshot(
            &repo,
            &file_name(&entry.path),
            &content,
            message,
            Author::You,
        )?;
        Ok(Synced {
            changed: true,
            content,
            history: history(&repo)?,
        })
    }

    pub fn history(&self, id: &str) -> Result<Vec<Version>> {
        self.entry(id)?;
        history(&self.repo(id))
    }

    /// Path of a document's shadow repository (for diagnostics and tests).
    pub fn repo_path(&self, id: &str) -> PathBuf {
        self.repo(id)
    }
}

// ---- git plumbing ------------------------------------------------------------

fn null_device() -> &'static str {
    if cfg!(windows) {
        "NUL"
    } else {
        "/dev/null"
    }
}

/// Run git against our repository only, isolated from the user's environment and config.
fn git(
    git_dir: Option<&Path>,
    cwd: &Path,
    args: &[&str],
    input: Option<&[u8]>,
    env: &[(&str, String)],
) -> Result<Vec<u8>> {
    let (status, stdout, stderr) = git_raw(git_dir, cwd, args, input, env)?;
    if status {
        Ok(stdout)
    } else {
        err(format!(
            "git {}: {}",
            args.first().unwrap_or(&""),
            String::from_utf8_lossy(&stderr).trim()
        ))
    }
}

fn git_raw(
    git_dir: Option<&Path>,
    cwd: &Path,
    args: &[&str],
    input: Option<&[u8]>,
    env: &[(&str, String)],
) -> Result<(bool, Vec<u8>, Vec<u8>)> {
    let mut cmd = Command::new("git");
    if let Some(dir) = git_dir {
        cmd.arg("--git-dir").arg(dir);
    }
    cmd.args(args).current_dir(cwd);
    // Anything that could redirect git at another repository or identity.
    for var in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_COMMON_DIR",
        "GIT_NAMESPACE",
        "GIT_CEILING_DIRECTORIES",
        "GIT_DISCOVERY_ACROSS_FILESYSTEM",
        "GIT_AUTHOR_NAME",
        "GIT_AUTHOR_EMAIL",
        "GIT_AUTHOR_DATE",
        "GIT_COMMITTER_NAME",
        "GIT_COMMITTER_EMAIL",
        "GIT_COMMITTER_DATE",
        "GIT_CONFIG",
        "GIT_CONFIG_PARAMETERS",
        "GIT_CONFIG_COUNT",
        "GIT_TEMPLATE_DIR",
    ] {
        cmd.env_remove(var);
    }
    // Ignore system and global config (signing, hooksPath, templates, filters...).
    cmd.env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", null_device())
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C");
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd.stdin(if input.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    })
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
    let mut child = cmd
        .spawn()
        .map_err(|e| Error(format!("could not run git: {e}")))?;
    // Feed stdin on a thread so large outputs can't deadlock against a full pipe.
    let writer = input.map(|data| {
        let mut stdin = child.stdin.take().expect("piped stdin");
        let data = data.to_vec();
        std::thread::spawn(move || stdin.write_all(&data))
    });
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    child
        .stdout
        .take()
        .expect("piped stdout")
        .read_to_end(&mut stdout)?;
    child
        .stderr
        .take()
        .expect("piped stderr")
        .read_to_end(&mut stderr)?;
    let status = child.wait()?;
    if let Some(w) = writer {
        w.join()
            .map_err(|_| Error("git stdin writer panicked".into()))??;
    }
    Ok((status.success(), stdout, stderr))
}

fn init_repo(root: &Path, repo: &Path) -> Result<()> {
    let repo_s = repo.to_string_lossy().to_string();
    git(
        None,
        root,
        &["init", "--bare", "--quiet", "--template=", &repo_s],
        None,
        &[],
    )?;
    git(
        Some(repo),
        root,
        &["symbolic-ref", "HEAD", BRANCH],
        None,
        &[],
    )?;
    // Keep a reflog so every ref update is recoverable.
    git(
        Some(repo),
        root,
        &["config", "core.logAllRefUpdates", "always"],
        None,
        &[],
    )?;
    Ok(())
}

fn head(repo: &Path) -> Result<Option<String>> {
    let (ok, out, _) = git_raw(
        Some(repo),
        repo,
        &["rev-parse", "--quiet", "--verify", BRANCH],
        None,
        &[],
    )?;
    Ok(if ok {
        Some(String::from_utf8_lossy(&out).trim().to_string())
    } else {
        None
    })
}

/// The single blob in a snapshot commit's tree.
fn tree_blob(repo: &Path, commit: &str) -> Result<String> {
    let tree = format!("{commit}^{{tree}}");
    let out = git(Some(repo), repo, &["cat-file", "-p", &tree], None, &[])?;
    let line = String::from_utf8_lossy(&out);
    match line
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(2))
    {
        Some(sha) => Ok(sha.to_string()),
        None => err("empty snapshot tree"),
    }
}

/// Record `content` as a new commit on `main`. Returns the new commit, or None if
/// the content is identical to the latest snapshot.
fn snapshot(
    repo: &Path,
    name: &str,
    content: &str,
    message: &str,
    author: Author,
) -> Result<Option<String>> {
    let blob = String::from_utf8(git(
        Some(repo),
        repo,
        &["hash-object", "-w", "--stdin"],
        Some(content.as_bytes()),
        &[],
    )?)
    .map_err(|e| Error(e.to_string()))?
    .trim()
    .to_string();
    let parent = head(repo)?;
    if let Some(p) = &parent {
        if tree_blob(repo, p)? == blob {
            return Ok(None);
        }
    }
    let entry = format!("100644 blob {blob}\t{name}\n");
    let tree = String::from_utf8_lossy(&git(
        Some(repo),
        repo,
        &["mktree"],
        Some(entry.as_bytes()),
        &[],
    )?)
    .trim()
    .to_string();

    let (author_name, author_email) = author.ident();
    let date = format!("@{} +0000", now_ms() / 1000);
    let env = [
        ("GIT_AUTHOR_NAME", author_name.to_string()),
        ("GIT_AUTHOR_EMAIL", author_email.to_string()),
        ("GIT_AUTHOR_DATE", date.clone()),
        ("GIT_COMMITTER_NAME", "Doc Forge".to_string()),
        ("GIT_COMMITTER_EMAIL", "app@docforge.local".to_string()),
        ("GIT_COMMITTER_DATE", date),
    ];
    let mut args = vec!["commit-tree", tree.as_str()];
    if let Some(p) = &parent {
        args.extend(["-p", p.as_str()]);
    }
    args.extend(["-F", "-"]);
    let message = if message.trim().is_empty() {
        "Snapshot"
    } else {
        message
    };
    let commit = String::from_utf8_lossy(&git(
        Some(repo),
        repo,
        &args,
        Some(message.as_bytes()),
        &env,
    )?)
    .trim()
    .to_string();

    // Compare-and-swap: fails instead of clobbering if main moved underneath us.
    let old = parent.as_deref().unwrap_or(ZERO);
    git(
        Some(repo),
        repo,
        &[
            "update-ref",
            "-m",
            "doc forge snapshot",
            BRANCH,
            &commit,
            old,
        ],
        None,
        &[],
    )?;
    Ok(Some(commit))
}

/// All snapshots, oldest first, with their content.
fn history(repo: &Path) -> Result<Vec<Version>> {
    if head(repo)?.is_none() {
        return Ok(Vec::new());
    }
    let out = git(
        Some(repo),
        repo,
        &[
            "log",
            BRANCH,
            "--format=%x1e%H%x00%P%x00%ae%x00%at%x00%B%x00",
            "--raw",
            "--no-abbrev",
            "--root",
            "--no-renames",
        ],
        None,
        &[],
    )?;
    let text = String::from_utf8_lossy(&out);
    let mut metas = Vec::new();
    for record in text.split('\x1e').filter(|r| !r.trim().is_empty()) {
        let mut fields = record.splitn(6, '\0');
        let sha = fields.next().unwrap_or_default().trim().to_string();
        let parent = fields
            .next()
            .unwrap_or_default()
            .split_whitespace()
            .next()
            .map(str::to_string);
        let email = fields.next().unwrap_or_default().to_string();
        let at: i64 = fields
            .next()
            .unwrap_or_default()
            .trim()
            .parse()
            .unwrap_or(0);
        let message = fields.next().unwrap_or_default().trim().to_string();
        let raw = fields.next().unwrap_or_default();
        let blob = raw
            .lines()
            .filter(|l| l.starts_with(':'))
            .filter_map(|l| l.split_whitespace().nth(3))
            .find(|b| *b != ZERO)
            .map(str::to_string);
        let (label, body) = match message.split_once('\n') {
            Some((s, b)) => (s.trim().to_string(), b.trim().to_string()),
            None => (message.clone(), String::new()),
        };
        let author = if email == FORGE_EMAIL {
            Author::Agent
        } else {
            Author::You
        };
        metas.push((sha, parent, author, label, body, at * 1000, blob));
    }

    // Fetch every blob in one `cat-file --batch` round trip.
    let wanted: Vec<String> = metas.iter().filter_map(|m| m.6.clone()).collect();
    let input = wanted.iter().map(|b| format!("{b}\n")).collect::<String>();
    let batch = git(
        Some(repo),
        repo,
        &["cat-file", "--batch"],
        Some(input.as_bytes()),
        &[],
    )?;
    let mut blobs = HashMap::new();
    let mut i = 0;
    while i < batch.len() {
        let nl = match batch[i..].iter().position(|&c| c == b'\n') {
            Some(n) => i + n,
            None => break,
        };
        let header = String::from_utf8_lossy(&batch[i..nl]).to_string();
        let parts: Vec<&str> = header.split_whitespace().collect();
        i = nl + 1;
        if parts.len() < 3 || parts[1] == "missing" {
            continue;
        }
        let size: usize = parts[2].parse().unwrap_or(0);
        let end = (i + size).min(batch.len());
        blobs.insert(
            parts[0].to_string(),
            String::from_utf8_lossy(&batch[i..end]).to_string(),
        );
        i = end + 1;
    }

    let mut versions: Vec<Version> = metas
        .into_iter()
        .map(
            |(sha, parent, author, label, body, created_at, blob)| Version {
                content: blob
                    .and_then(|b| blobs.get(&b).cloned())
                    .unwrap_or_default(),
                sha,
                parent,
                author,
                label,
                body,
                created_at,
            },
        )
        .collect();
    versions.reverse();
    Ok(versions)
}

// ---- helpers -------------------------------------------------------------------

fn read_text(path: &Path) -> Result<String> {
    let bytes = fs::read(path)?;
    String::from_utf8(bytes)
        .map_err(|_| Error(format!("{} is not a UTF-8 text file", path.display())))
}

fn file_name(path: &Path) -> String {
    // Tree entry names can't contain tabs or newlines (mktree input format).
    path.file_name()
        .map(|n| n.to_string_lossy().replace(['\t', '\n', '\r'], "_"))
        .unwrap_or_else(|| "document".into())
}

fn names(path: &Path) -> (String, String) {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let folder = path
        .parent()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    (name, folder)
}

fn is_hex_sha(s: &str) -> bool {
    (7..=64).contains(&s.len()) && s.chars().all(|c| c.is_ascii_hexdigit())
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn new_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{:x}{:x}{:x}", nanos, std::process::id(), n)
}
