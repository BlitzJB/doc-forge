mod ai_commands;
mod cli;
mod launch;
mod threads;

use forge_ai::{AiService, FileSecrets};
use forge_shadow::{Author, OpenedDoc, RecentDoc, Store, Synced, Version};
use std::path::{Path, PathBuf};
use tauri::{Manager, State};

type Res<T> = Result<T, String>;

// Shadow-repo work shells out to git, so it runs off the main thread.
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> forge_shadow::Result<T> + Send + 'static,
) -> Res<T> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn doc_open(store: State<'_, std::sync::Arc<Store>>, path: PathBuf) -> Res<OpenedDoc> {
    let s = store.inner().clone();
    blocking(move || s.open(&path)).await
}

#[tauri::command]
async fn doc_save(
    store: State<'_, std::sync::Arc<Store>>,
    id: String,
    content: String,
    message: String,
    author: Author,
) -> Res<Vec<Version>> {
    let s = store.inner().clone();
    blocking(move || s.save(&id, &content, &message, author)).await
}

#[tauri::command]
async fn doc_sync(store: State<'_, std::sync::Arc<Store>>, id: String) -> Res<Synced> {
    let s = store.inner().clone();
    blocking(move || s.sync(&id)).await
}

#[tauri::command]
async fn doc_restore(
    store: State<'_, std::sync::Arc<Store>>,
    id: String,
    sha: String,
    message: String,
) -> Res<Synced> {
    let s = store.inner().clone();
    blocking(move || s.restore(&id, &sha, &message)).await
}

/// What's at a path sent from outside (the `df` command): "file", "missing" (can be
/// created: its folder exists), "no-folder", or "not-a-file".
#[tauri::command]
fn doc_probe(path: PathBuf) -> &'static str {
    if path.is_file() {
        "file"
    } else if path.exists() {
        "not-a-file"
    } else if path.parent().is_some_and(|d| d.is_dir()) {
        "missing"
    } else {
        "no-folder"
    }
}

/// Create an empty document the author agreed to create. Never overwrites.
#[tauri::command]
async fn doc_create(store: State<'_, std::sync::Arc<Store>>, path: PathBuf) -> Res<OpenedDoc> {
    if !path.is_absolute() {
        return Err("Doc Forge needs a full path to create a file.".into());
    }
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::AlreadyExists => {
                "That file exists now; open it instead.".to_string()
            }
            std::io::ErrorKind::NotFound => "Its folder doesn't exist.".to_string(),
            std::io::ErrorKind::PermissionDenied => {
                "Doc Forge isn't allowed to write in that folder.".to_string()
            }
            _ => e.to_string(),
        })?;
    let store = store.inner().clone();
    blocking(move || store.open(&path)).await
}

#[tauri::command]
fn doc_recent(store: State<'_, std::sync::Arc<Store>>) -> Vec<RecentDoc> {
    store.recent()
}

// ---- The df terminal command ------------------------------------------------------

fn df_script(app: &tauri::AppHandle) -> PathBuf {
    cli::script(app.path().resource_dir().ok())
}

#[tauri::command]
fn cli_status(app: tauri::AppHandle) -> cli::CliStatus {
    cli::status(&df_script(&app))
}

#[tauri::command]
async fn cli_install(app: tauri::AppHandle) -> Res<String> {
    let script = df_script(&app);
    tauri::async_runtime::spawn_blocking(move || cli::install(&script))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn cli_uninstall() -> Res<()> {
    tauri::async_runtime::spawn_blocking(cli::uninstall)
        .await
        .map_err(|e| e.to_string())?
}

// ---- Editor threads -----------------------------------------------------------------

type ThreadStore<'a> = State<'a, threads::Threads>;

#[tauri::command]
async fn thread_list(t: ThreadStore<'_>, doc: String) -> Res<Vec<threads::ThreadMeta>> {
    t.list(&doc)
}

#[tauri::command]
async fn thread_load(t: ThreadStore<'_>, doc: String, id: String) -> Res<serde_json::Value> {
    t.load(&doc, &id)
}

#[tauri::command]
async fn thread_save(t: ThreadStore<'_>, doc: String, thread: serde_json::Value) -> Res<()> {
    t.save(&doc, &thread)
}

#[tauri::command]
async fn thread_delete(t: ThreadStore<'_>, doc: String, id: String) -> Res<()> {
    t.delete(&doc, &id)
}

/// A short title for a thread from its opening exchange (the Quick model).
#[tauri::command]
async fn thread_title(
    ai: State<'_, std::sync::Arc<AiService>>,
    prompt: String,
    reply: String,
) -> Res<String> {
    ai.title(&prompt, &reply).await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    ai_commands::load_dev_env();
    tauri::Builder::default()
        // First, so a second launch (`df notes.md` while Doc Forge is open) hands its
        // files to this window and exits before anything else starts.
        .plugin(tauri_plugin_single_instance::init(|app, args, cwd| {
            launch::open_paths(app, launch::paths_from_args(&args, Path::new(&cwd)));
        }))
        // Managed up front: macOS can deliver files to open before setup runs.
        .manage(launch::Pending::default())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // Shadow repositories live in the app's private data folder, never next to user files.
            let root = app.path().app_data_dir()?.join("history");
            let store = Store::new(root).map_err(|e| e.to_string())?;
            app.manage(std::sync::Arc::new(store));
            app.manage(threads::Threads::new(
                app.path().app_data_dir()?.join("threads"),
            ));
            // Provider settings in ai.json; keys and sign-in tokens in credentials.json (mode 600),
            // both in the app's config folder. No system keychain, so no permission prompts.
            let ai_path = app.path().app_config_dir()?.join("ai.json");
            let ai = AiService::new(
                ai_path,
                Box::new(FileSecrets::new(
                    app.path().app_config_dir()?.join("credentials.json"),
                )),
            );
            app.manage(std::sync::Arc::new(ai));
            app.manage(std::sync::Arc::new(ai_commands::Runs::default()));
            // Files named on this launch's command line.
            let args: Vec<String> = std::env::args().collect();
            let cwd = std::env::current_dir().unwrap_or_default();
            launch::open_paths(app.handle(), launch::paths_from_args(&args, &cwd));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            doc_open,
            doc_save,
            doc_sync,
            doc_restore,
            doc_recent,
            doc_probe,
            doc_create,
            ai_commands::ai_state,
            ai_commands::ai_check,
            ai_commands::ai_save_key,
            ai_commands::ai_forget_key,
            ai_commands::ai_set_base_url,
            ai_commands::ai_add_custom,
            ai_commands::ai_remove_provider,
            ai_commands::ai_set_role,
            ai_commands::ai_test,
            ai_commands::ai_save_search_key,
            ai_commands::ai_forget_search_key,
            ai_commands::ai_search_sign_in,
            ai_commands::ai_cancel_sign_in,
            ai_commands::ai_search_sign_out,
            ai_commands::agent_run,
            ai_commands::agent_cancel,
            launch::take_opens,
            thread_list,
            thread_load,
            thread_save,
            thread_delete,
            thread_title,
            cli_status,
            cli_install,
            cli_uninstall
        ])
        .build(tauri::generate_context!())
        .expect("error while building Doc Forge")
        .run(|app, event| {
            // macOS delivers Finder and `open -a` requests as an event, not as arguments.
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Opened { urls } = event {
                let paths = urls.iter().filter_map(|u| u.to_file_path().ok()).collect();
                launch::open_paths(app, paths);
            }
            #[cfg(not(target_os = "macos"))]
            let _ = (app, event);
        });
}
