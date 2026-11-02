//! Live tests against real providers. They skip (pass) when no key is available, so
//! CI without secrets stays green. Run locally with:
//!   set -a; . ../.env.local; set +a; cargo test -p forge-ai --test live -- --nocapture

use forge_ai::{AiService, KeySource, MemorySecrets, Role, RoleChoice, Status};

fn fireworks_key() -> Option<String> {
    std::env::var("FIREWORKS_API_KEY")
        .ok()
        .filter(|k| !k.trim().is_empty())
}

fn service(dir: &tempfile::TempDir) -> AiService {
    AiService::new(
        dir.path().join("ai.json"),
        Box::new(MemorySecrets::default()),
    )
}

#[tokio::test]
async fn fireworks_end_to_end() {
    let Some(key) = fireworks_key() else {
        eprintln!("FIREWORKS_API_KEY not set; skipping live Fireworks test");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let ai = service(&dir);

    // A wrong key is rejected and not stored.
    let bad = ai
        .save_key("fireworks", "fw_definitely_not_a_key")
        .await
        .unwrap();
    assert_eq!(bad.status, Status::Unauthorized, "{}", bad.message);
    assert_eq!(bad.message, "Fireworks rejected this key.");

    // The real key is verified, stored, and models are discovered with capabilities.
    let check = ai.save_key("fireworks", &key).await.unwrap();
    assert_eq!(check.status, Status::Connected, "{}", check.message);
    assert!(!check.models.is_empty());
    assert!(
        check.models.iter().all(|m| !m.id.contains("embedding")),
        "embedders must be filtered"
    );
    let tool_models: Vec<_> = check
        .models
        .iter()
        .filter(|m| m.tools == Some(true) && !m.router)
        .collect();
    assert!(!tool_models.is_empty(), "expected tool-capable models");
    eprintln!(
        "fireworks: {} chat models in {} ms, {} tool-capable, e.g. {}",
        check.models.len(),
        check.latency_ms,
        tool_models.len(),
        tool_models
            .iter()
            .take(4)
            .map(|m| m.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );

    let view = ai.view();
    let fw = view.providers.iter().find(|p| p.id == "fireworks").unwrap();
    assert_eq!(fw.key_source, KeySource::Stored);
    assert_eq!(
        fw.key_hint.as_deref(),
        Some(forge_ai::key_hint(&key).as_str())
    );
    assert!(fw
        .check
        .as_ref()
        .is_some_and(|c| c.status == Status::Connected));

    // Assign the editor role and do a real round trip through it.
    let model = tool_models
        .iter()
        .find(|m| m.name.contains("flash"))
        .unwrap_or(&tool_models[0])
        .id
        .clone();
    ai.set_role(
        Role::Editor,
        Some(RoleChoice {
            provider: "fireworks".into(),
            model: model.clone(),
        }),
    )
    .unwrap();
    assert_eq!(
        ai.role(Role::Quick).unwrap().model,
        model,
        "quick falls back to editor"
    );
    let choice = ai.role(Role::Editor).unwrap();
    let t = ai.test(&choice.provider, &choice.model).await.unwrap();
    eprintln!(
        "test via {}: {:?} in {} ms, usage {:?}, reasoned={}",
        choice.model, t.reply, t.latency_ms, t.usage, t.reasoned
    );
    assert_eq!(t.status, Status::Connected, "{}", t.message);
    assert!(
        t.reply.to_lowercase().contains("connection ok"),
        "reply was {:?}",
        t.reply
    );

    // Settings persist across restarts (key stays out of the file).
    let saved = std::fs::read_to_string(dir.path().join("ai.json")).unwrap();
    assert!(
        !saved.contains(&key),
        "the key must never be written to the config file"
    );
    let again = AiService::new(
        dir.path().join("ai.json"),
        Box::new(MemorySecrets::default()),
    );
    assert_eq!(again.role(Role::Editor).unwrap().model, model);
}

#[tokio::test]
async fn offline_states_are_clear() {
    let dir = tempfile::tempdir().unwrap();
    let ai = service(&dir);
    // Anthropic with no key (and none in the environment of this test) reports it plainly.
    if std::env::var("ANTHROPIC_API_KEY").is_err() {
        let c = ai.check("anthropic").await.unwrap();
        assert_eq!(c.status, Status::Unauthorized);
        assert_eq!(c.message, "No key yet.");
    }
    // Ollama is either running locally or clearly unreachable, never an error dump.
    let c = ai.check("ollama").await.unwrap();
    assert!(
        matches!(c.status, Status::Connected | Status::Unreachable),
        "{:?}: {}",
        c.status,
        c.message
    );
    if c.status == Status::Unreachable {
        assert_eq!(c.message, "Ollama could not be reached.");
    }
    // Custom providers validate their address and can be removed.
    assert!(ai.add_custom("Gateway", "ftp://nope").is_err());
    let id = ai.add_custom("Gateway", "http://127.0.0.1:9/v1").unwrap();
    let c = ai.check(&id).await.unwrap();
    assert_eq!(c.status, Status::Unreachable);
    ai.set_role(
        Role::Quick,
        Some(RoleChoice {
            provider: id.clone(),
            model: "m".into(),
        }),
    )
    .unwrap();
    ai.remove_provider(&id).unwrap();
    assert!(
        ai.view().roles.quick.is_none(),
        "removing a provider clears roles that used it"
    );
    assert!(
        ai.remove_provider("fireworks").is_err(),
        "built-ins can't be removed"
    );
}

/// The Quick model names a thread from its opening exchange.
#[tokio::test]
async fn titles_a_thread() {
    let Ok(key) = std::env::var("FIREWORKS_API_KEY") else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let ai = forge_ai::AiService::new(
        dir.path().join("ai.json"),
        Box::new(forge_ai::MemorySecrets::default()),
    );
    ai.save_key("fireworks", &key).await.unwrap();
    let model = std::env::var("FORGE_QUICK_MODEL")
        .unwrap_or_else(|_| "accounts/fireworks/models/deepseek-v4p1-flash".into());
    ai.set_role(
        forge_ai::Role::Editor,
        Some(forge_ai::RoleChoice {
            provider: "fireworks".into(),
            model,
        }),
    )
    .unwrap();
    let started = std::time::Instant::now();
    let t = ai
        .title(
            "oh wait df is usually used for disk usage command uh?",
            "On most systems df is the disk-free command (du measures disk usage). In Doc Forge, df notes.md opens the file in the app, while df -h still runs the system command.",
        )
        .await
        .unwrap();
    eprintln!("title: {t:?} in {} ms", started.elapsed().as_millis());
    let words = t.split_whitespace().count();
    assert!((1..=8).contains(&words), "{t}");
    assert!(!t.ends_with('.') && !t.contains('"'));
}
