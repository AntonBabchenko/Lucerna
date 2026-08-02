//! Test fixtures shared by more than one of this module's `tests` modules.
//!
//! A [`RunContext`] is built from an `AppHandle` in production, so every test
//! that needs one has to assemble it by hand; keeping the single assembly here
//! is what stops `state` and `pipeline` from drifting into two different
//! notions of what a test run looks like.

use std::path::Path;

use crate::instances::schema::AiProvider;
use crate::l10n::prefill::glossary::Glossary;
use crate::l10n::scan::LangMap;
use crate::l10n::store;
use crate::network::consent::AiConsent;

use super::{RunContext, CACHE_DIR, CACHE_STEM};

/// A glossary standing in for vanilla's own corpus.
pub(super) fn vanilla(pairs: &[(&str, &str)]) -> Glossary {
    let mut en = LangMap::new();
    let mut tr = LangMap::new();
    for (i, (english, translated)) in pairs.iter().enumerate() {
        en.insert(format!("k{i}"), (*english).to_string());
        tr.insert(format!("k{i}"), (*translated).to_string());
    }
    Glossary::from_lang_maps(&en, &tr, "1.20.1")
}

/// A context over a temp directory. `Local` on port 1 so that any test
/// which accidentally reaches the provider fails fast and loudly instead
/// of hanging or spending money.
pub(super) fn test_ctx_with(dir: &Path, glossary: Glossary) -> RunContext {
    let store_dir = dir.join("l10n");
    RunContext {
        consent: AiConsent::for_test(),
        inst_root: dir.join("instance"),
        cache_path: store::store_path(&store_dir.join(CACHE_DIR), "ru_ru", CACHE_STEM),
        store_dir,
        lang: "ru_ru".to_string(),
        namespace: None,
        provider: AiProvider::Local,
        api_key: None,
        local_port: 1,
        model: "test-model".to_string(),
        glossary,
    }
}

pub(super) fn test_ctx(dir: &Path) -> RunContext {
    test_ctx_with(dir, Glossary::empty())
}
