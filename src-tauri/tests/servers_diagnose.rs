//! Integration tests for the server diagnosis pipeline.
//!
//! Tests operate on the public API (no AppHandle) — I/O and Tauri wiring
//! are covered by the command tests; these verify the pure classification
//! and token-matching pipeline end-to-end.

use lucerna_lib::logs::diagnose::server::{
    classify_client_only_mods, diagnose_server_log, dist_crash_tokens,
};
use lucerna_lib::mods::local::ModEnvironment;

#[test]
fn etf_crash_end_to_end_classification() {
    let log = "Caused by: java.lang.RuntimeException: Attempted to load class net/minecraft/client/gui/screens/Screen for invalid dist DEDICATED_SERVER\n\tat TRANSFORMER/minecraft@1.20.1/net.minecraft.resources.ResourceLocation.handler$zpl000$etf$illegalPathOverride(ResourceLocation.java:525)\n";
    let diag = diagnose_server_log(log).expect("diagnosis");
    assert_eq!(diag.pattern_id, "server-client-only-mod-crash");
    let tokens = dist_crash_tokens(log);
    let mods = vec![(
        "entity_texture_features_1.20.1-forge-7.1.jar".to_string(),
        ModEnvironment::Unknown,
    )];
    let found = classify_client_only_mods(&mods, &tokens);
    assert_eq!(found.len(), 1);
    assert!(found[0].filename.contains("entity_texture_features"));
}

#[test]
fn log_signature_content_based_stable() {
    use lucerna_lib::logs::diagnose::log_signature;
    let content = "some server log content\n";
    assert_eq!(log_signature(content), log_signature(content));
    assert_ne!(log_signature(content), log_signature("other content\n"));
    // Not empty
    assert!(!log_signature(content).is_empty());
}
