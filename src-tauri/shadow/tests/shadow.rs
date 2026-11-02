use forge_shadow::{Author, Store};
use std::fs;
use std::path::Path;
use std::process::Command;

fn sh(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn listing(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .collect();
    v.sort();
    v
}

#[test]
fn open_save_sync_restore() {
    let data = tempfile::tempdir().unwrap();
    let docs = tempfile::tempdir().unwrap();
    let file = docs.path().join("plan.md");
    fs::write(&file, "# Plan\n\nfirst draft\n").unwrap();
    let store = Store::new(data.path()).unwrap();

    let doc = store.open(&file).unwrap();
    assert_eq!(doc.name, "plan.md");
    assert_eq!(doc.history.len(), 1);
    assert_eq!(doc.history[0].label, "Started tracking in Doc Forge");
    assert_eq!(doc.history[0].content, "# Plan\n\nfirst draft\n");

    // Re-opening unchanged content records nothing.
    assert_eq!(store.open(&file).unwrap().history.len(), 1);

    // Agent save writes the file and records an agent snapshot.
    let h = store
        .save(
            &doc.id,
            "# Plan\n\nsharper draft\n",
            "Accepted: tighter intro",
            Author::Agent,
        )
        .unwrap();
    assert_eq!(h.len(), 2);
    assert_eq!(h[1].author, Author::Agent);
    assert_eq!(h[1].label, "Accepted: tighter intro");
    assert_eq!(h[1].parent.as_deref(), Some(h[0].sha.as_str()));
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "# Plan\n\nsharper draft\n"
    );

    // Saving identical content is a no-op.
    assert_eq!(
        store
            .save(
                &doc.id,
                "# Plan\n\nsharper draft\n",
                "Manual edits",
                Author::You
            )
            .unwrap()
            .len(),
        2
    );

    // Multi-line messages keep a subject and body (trailers).
    let h = store
        .save(
            &doc.id,
            "# Plan\n\nthird\n",
            "Manual edits\n\nForge-Suggestion: abc",
            Author::You,
        )
        .unwrap();
    assert_eq!(h[2].label, "Manual edits");
    assert_eq!(h[2].body, "Forge-Suggestion: abc");

    // Edits made outside Doc Forge are picked up.
    fs::write(&file, "# Plan\n\nedited in vim\n").unwrap();
    let s = store.sync(&doc.id).unwrap();
    assert!(s.changed);
    assert_eq!(s.history.last().unwrap().label, "Edited outside Doc Forge");
    assert!(!store.sync(&doc.id).unwrap().changed);

    // Restore writes an old version back and records it as new history.
    let first = s.history[0].sha.clone();
    let r = store.restore(&doc.id, &first, "Restored v1").unwrap();
    assert_eq!(r.content, "# Plan\n\nfirst draft\n");
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "# Plan\n\nfirst draft\n"
    );
    assert_eq!(r.history.len(), 5);
    assert_eq!(r.history.last().unwrap().label, "Restored v1");
    assert!(store.restore(&doc.id, "not-a-sha; rm -rf /", "x").is_err());

    // History survives a fresh Store (app restart).
    let reopened = Store::new(data.path()).unwrap();
    assert_eq!(reopened.history(&doc.id).unwrap().len(), 5);
    assert_eq!(reopened.recent()[0].name, "plan.md");
}

#[test]
fn never_touches_the_users_repository() {
    let data = tempfile::tempdir().unwrap();
    let proj = tempfile::tempdir().unwrap();
    let dir = proj.path();
    sh(dir, &["init", "-q"]);
    fs::write(dir.join("notes.md"), "hello\n").unwrap();
    fs::write(dir.join(".gitignore"), "notes.md\n").unwrap();
    fs::write(dir.join(".gitattributes"), "*.md filter=boom\n").unwrap();
    sh(
        dir,
        &["add", "-f", "notes.md", ".gitignore", ".gitattributes"],
    );
    sh(
        dir,
        &[
            "-c",
            "user.name=u",
            "-c",
            "user.email=u@x",
            "commit",
            "-qm",
            "user commit",
        ],
    );
    // A filter and hooks that would leave evidence if git ever ran them.
    sh(
        dir,
        &["config", "filter.boom.clean", "touch FILTER_RAN; cat"],
    );
    let hooks = dir.join(".git/hooks");
    fs::create_dir_all(&hooks).unwrap();
    for h in ["pre-commit", "post-commit", "reference-transaction"] {
        fs::write(hooks.join(h), "#!/bin/sh\ntouch \"$GIT_DIR/../HOOK_RAN\"\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(hooks.join(h), fs::Permissions::from_mode(0o755)).unwrap();
        }
    }
    let log_before = sh(dir, &["log", "--format=%H"]);
    let refs_before = sh(dir, &["for-each-ref"]);
    let files_before = listing(dir);

    // Hostile environment: point GIT_DIR at the user's repo.
    std::env::set_var("GIT_DIR", dir.join(".git"));
    let store = Store::new(data.path()).unwrap();
    let doc = store.open(&dir.join("notes.md")).unwrap();
    store
        .save(&doc.id, "hello, edited\n", "Manual edits", Author::You)
        .unwrap();
    store
        .save(
            &doc.id,
            "hello, edited by forge\n",
            "Accepted",
            Author::Agent,
        )
        .unwrap();
    std::env::remove_var("GIT_DIR");

    assert_eq!(store.history(&doc.id).unwrap().len(), 3);
    assert_eq!(
        sh(dir, &["log", "--format=%H"]),
        log_before,
        "user's history changed"
    );
    assert_eq!(
        sh(dir, &["for-each-ref"]),
        refs_before,
        "user's refs changed"
    );
    assert_eq!(
        listing(dir),
        files_before,
        "files were added to the user's folder"
    );
    assert!(
        !dir.join("HOOK_RAN").exists() && !dir.join(".git/HOOK_RAN").exists(),
        "a user hook ran"
    );
    assert!(!dir.join("FILTER_RAN").exists(), "a user filter ran");
    // The user's repo just sees an ordinary modified file.
    assert_eq!(sh(dir, &["status", "--porcelain"]).trim(), "M notes.md");
    // And none of our commits exist in it.
    let ours = store.history(&doc.id).unwrap().last().unwrap().sha.clone();
    let probe = Command::new("git")
        .args(["cat-file", "-e", &ours])
        .current_dir(dir)
        .status()
        .unwrap();
    assert!(!probe.success());
    // Our repo lives in our data folder.
    assert!(store.repo_path(&doc.id).starts_with(data.path()));
}

#[test]
fn moved_files_keep_their_history() {
    let data = tempfile::tempdir().unwrap();
    let docs = tempfile::tempdir().unwrap();
    let a = docs.path().join("draft.md");
    fs::write(&a, "v1\n").unwrap();
    let store = Store::new(data.path()).unwrap();
    let doc = store.open(&a).unwrap();
    store
        .save(&doc.id, "v2\n", "Manual edits", Author::You)
        .unwrap();

    let moved = docs.path().join("sub");
    fs::create_dir(&moved).unwrap();
    let b = moved.join("renamed.md");
    fs::rename(&a, &b).unwrap();

    let again = store.open(&b).unwrap();
    assert_eq!(again.id, doc.id);
    assert_eq!(again.history.len(), 2);
    assert_eq!(again.name, "renamed.md");
    assert_eq!(again.folder, "sub");

    // A different file with the same name elsewhere gets its own history.
    let other = docs.path().join("draft.md");
    fs::write(&other, "unrelated\n").unwrap();
    assert_ne!(store.open(&other).unwrap().id, doc.id);
}

#[test]
fn rejects_binary_files() {
    let data = tempfile::tempdir().unwrap();
    let docs = tempfile::tempdir().unwrap();
    let f = docs.path().join("image.bin");
    fs::write(&f, [0xff, 0xfe, 0x00, 0x81]).unwrap();
    let store = Store::new(data.path()).unwrap();
    assert!(store.open(&f).is_err());
}
