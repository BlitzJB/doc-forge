//! Connect a provider for the installed app exactly as Settings → Models does:
//! verify the key, store it in Doc Forge's credentials file, and choose the Editor and Quick models.
//!   set -a; . ../../.env.local; set +a
//!   cargo run -p forge-ai --example connect -- fireworks <editor-model> [quick-model]
//! The key comes from the provider's environment variable (e.g. FIREWORKS_API_KEY).

use forge_ai::{AiService, FileSecrets, Role, RoleChoice, Status};

const APP_ID: &str = "app.docforge.desktop";

#[tokio::main]
async fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let provider = args
        .first()
        .ok_or("usage: connect <provider> [editor-model] [quick-model]")?;
    let home = std::env::var("HOME").map_err(|e| e.to_string())?;
    // Tauri's app_config_dir on macOS.
    let config = format!("{home}/Library/Application Support/{APP_ID}/ai.json");
    let creds = format!("{home}/Library/Application Support/{APP_ID}/credentials.json");
    let ai = AiService::new(config, Box::new(FileSecrets::new(creds)));

    let view = ai.view();
    let p = view
        .providers
        .iter()
        .find(|p| p.id == *provider)
        .ok_or("unknown provider")?;
    let var = p.info.env_var.ok_or("this provider has no key variable")?;
    let key = std::env::var(var).map_err(|_| format!("{var} is not set"))?;

    let check = ai.save_key(provider, &key).await?;
    println!(
        "{provider}: {:?}, {} ms, {} models",
        check.status,
        check.latency_ms,
        check.models.len()
    );
    if check.status != Status::Connected {
        return Err(check.message);
    }
    for m in check.models.iter().filter(|m| m.tools == Some(true)) {
        println!("  tools  {}", m.id);
    }
    if let Some(editor) = args.get(1) {
        ai.set_role(
            Role::Editor,
            Some(RoleChoice {
                provider: provider.clone(),
                model: editor.clone(),
            }),
        )?;
        let quick = args.get(2).unwrap_or(editor);
        ai.set_role(
            Role::Quick,
            Some(RoleChoice {
                provider: provider.clone(),
                model: quick.clone(),
            }),
        )?;
        for (role, model) in [("editor", editor), ("quick", quick)] {
            let t = ai.test(provider, model).await?;
            println!(
                "{role} {model}: {:?} in {} ms, reply {:?}",
                t.status, t.latency_ms, t.reply
            );
        }
    }
    Ok(())
}
