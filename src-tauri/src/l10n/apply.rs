//! Building, placing and enabling the generated override resource pack.
//!
//! Split out of `commands::l10n_apply` because the rebuild has more than one
//! caller: the override editor's Apply button, and the AI pre-fill run, which
//! finishes by shipping the strings it just wrote. A domain module calling a
//! `#[tauri::command]` would be a layering inversion with no precedent in this
//! crate, so the command keeps only what is genuinely its own — the data-root
//! and running-instance guards — and delegates the work to [`rebuild_pack`].
//!
//! Nothing here changed in the move: the sweep, the ordering of sweep-then-
//! write, and the `false` return contract are exactly what the command did
//! inline.

/// Remove every Lucerna-generated resource pack — file and Add-ons registry
/// row — except `keep_filename`, best-effort.
///
/// Runs on EVERY `l10n_apply` call, not only when the target language
/// changes: it also sweeps up packs left behind by an older build of this
/// feature, or by a language the user has since abandoned. Only one Lucerna
/// pack is ever meant to exist — `options_txt::with_pack_enabled` already
/// enforces that for the `options.txt` ENTRY (it strips every prior
/// `PACK_PREFIX`-matching entry before appending the new one) — so leaving
/// the FILE and its Add-ons row behind after a language switch would be
/// inert but user-visible clutter: a resource pack the user never installed,
/// sitting in the Add-ons list with no explanation.
///
/// A removal failure here is untidiness, not a broken apply: logged via
/// `crate::diag!` and skipped, the same posture `ScanCache::update` takes on
/// a save failure. `keep_filename = None` sweeps every Lucerna pack — used on
/// the "nothing to ship" path, where no pack should survive at all.
async fn sweep_stale_lucerna_packs(inst_root: &std::path::Path, keep_filename: Option<&str>) {
    let installed = match crate::mods::assets::list(
        inst_root,
        crate::mods::platform::ContentKind::ResourcePack,
    )
    .await
    {
        Ok(items) => items,
        Err(e) => {
            crate::diag!("[l10n] apply: could not list resource packs to sweep: {e}");
            return;
        }
    };

    for asset in installed {
        if !asset
            .filename
            .starts_with(crate::l10n::options_txt::PACK_PREFIX)
        {
            continue; // not ours — never touched by this sweep
        }
        if keep_filename == Some(asset.filename.as_str()) {
            continue;
        }

        match crate::mods::install::safe_asset_remove_path(
            inst_root,
            crate::mods::platform::ContentKind::ResourcePack,
            &asset.filename,
        ) {
            Ok(path) => {
                if let Err(e) = tokio::fs::remove_file(&path).await {
                    if e.kind() != std::io::ErrorKind::NotFound {
                        crate::diag!(
                            "[l10n] apply: could not remove stale pack {}: {e}",
                            asset.filename
                        );
                    }
                }
            }
            Err(e) => {
                crate::diag!(
                    "[l10n] apply: refusing to remove stale pack {} ({e})",
                    asset.filename
                );
            }
        }

        if let Err(e) = crate::mods::assets::remove(
            inst_root,
            crate::mods::platform::ContentKind::ResourcePack,
            &asset.filename,
        )
        .await
        {
            crate::diag!(
                "[l10n] apply: could not remove stale pack's registry row {}: {e}",
                asset.filename
            );
        }
    }
}

/// Load every namespace with overrides for `lang` and build the pack bytes —
/// the single source of truth for the (stores, description, build) triple.
///
/// BOTH `rebuild_pack` and the cross-instance apply-targets command call this.
/// The targets command byte-compares an instance's installed pack against this
/// function's output, so a second assembly site would let the inputs drift —
/// most fragile of all, the free-form description string — and would flag every
/// applied instance as permanently outdated while the byte-stability test in
/// `pack.rs` stayed green. The description is therefore frozen and deliberately
/// unlocalized: it is part of the hashed bytes, so translating it would make a
/// pack applied under one UI language read 'outdated' under another.
pub(crate) fn assemble_current_pack(
    store_dir: &std::path::Path,
    lang: &str,
    fmt: crate::l10n::pack_format::PackFormat,
) -> Option<Vec<u8>> {
    let namespaces = crate::l10n::store::namespaces_with_overrides(store_dir, lang);
    let stores: Vec<_> = namespaces
        .iter()
        .map(|ns| crate::l10n::store::load(store_dir, lang, ns))
        .collect();
    let description = format!("Lucerna translations ({lang})");
    crate::l10n::pack::build(&stores, lang, fmt, &description)
}

/// Rebuild and install the override pack for one instance: build it from
/// every namespace with overrides for `lang`, place it, register it in the
/// Add-ons list, and enable it in `options.txt`.
///
/// Returns whether `options.txt` activation happened. `false` covers two
/// distinct, non-error outcomes the caller must tell apart from a hard
/// failure — see `commands::l10n_apply`'s doc comment, which is the contract
/// the UI reads.
///
/// Callers own the guards: this does NOT check the data-root fallback state
/// or whether the instance is running. `l10n_apply` does both before calling
/// in, and any other caller must do the same.
///
/// Instance identity is resolved through the command layer's two shared
/// helpers rather than re-derived here: `read_active_mc_and_loader` answers
/// `InstanceNotFound` for an unknown id, and a second copy of that check
/// would be free to drift from the one every other instance-scoped command
/// uses.
pub async fn rebuild_pack(
    app: &tauri::AppHandle,
    instance_id: &str,
    lang: &str,
) -> Result<bool, crate::error::Error> {
    let inst_root = crate::commands::instance_root(app, instance_id)?;
    let store_dir =
        crate::paths::l10n_dir(app).map_err(|e| crate::error::Error::io("<l10n_dir>", e))?;
    let versions_dir = crate::paths::versions_dir(app)
        .map_err(|e| crate::error::Error::io("<versions_dir>", e))?;
    let (mc_version, _loader) = crate::commands::read_active_mc_and_loader(app, instance_id)?;

    let client_jar = crate::datapacks::compat::client_jar_path(&versions_dir, &mc_version);
    let fmt = crate::l10n::pack_format::from_client_jar_path(&client_jar);
    match crate::l10n::pack_format::apply_gate(fmt) {
        crate::l10n::pack_format::ApplyGate::UnknownFormat => {
            return Err(crate::error::Error::L10nFormatUnknown { mc_version });
        }
        crate::l10n::pack_format::ApplyGate::TooOld => {
            return Err(crate::error::Error::L10nFormatTooOld { mc_version });
        }
        crate::l10n::pack_format::ApplyGate::Ready => {}
    }

    let built = assemble_current_pack(&store_dir, lang, fmt);

    let mc_dir = inst_root.join(".minecraft");
    let filename = format!("{}{lang}.zip", crate::l10n::options_txt::PACK_PREFIX);

    let Some(bytes) = built else {
        // Nothing to ship: sweep away every Lucerna pack, including this
        // language's own if one exists — `keep_filename: None` — plus the
        // options.txt entry. Emptying every override must leave no Lucerna
        // pack behind at all.
        sweep_stale_lucerna_packs(&inst_root, None).await;
        crate::l10n::options_txt::update_atomically(
            &mc_dir,
            crate::launch::spawn::is_running(instance_id),
            |s| Some(crate::l10n::options_txt::with_pack_disabled(s)),
        )?;
        return Ok(false);
    };

    // Sweep BEFORE writing: by the time `install_asset_local` runs, no OTHER
    // Lucerna pack can be sitting in the Add-ons list. The sweep excludes
    // `filename` itself, so a re-apply of the SAME language never touches
    // the file/row this call is about to write — there is no ordering race
    // to get wrong here, only tidiness to get right before the new pack
    // lands rather than after.
    sweep_stale_lucerna_packs(&inst_root, Some(&filename)).await;

    // The sanctioned sink (temp-sibling + rename, so a pack that already
    // exists at this filename is never written through) plus the manual-
    // install registry convention (`source: None`) — see
    // `mods::asset_local::install_asset_local`'s own doc comment.
    crate::mods::asset_local::install_asset_local(
        &inst_root,
        crate::mods::platform::ContentKind::ResourcePack,
        &filename,
        &bytes,
    )
    .await?;

    // `ApplyGate::Ready` already proved `supports_apply(fmt)`, i.e. resource
    // format >= 4 — Minecraft 1.13+, which is exactly when `options.txt`
    // stores user pack ids under the `file/` prefix (see `l10n::options_txt`'s
    // module doc). Re-snapshot `is_running` rather than reusing the guard
    // the command applied: real time has passed doing the work above this
    // line.
    let activated = crate::l10n::options_txt::update_atomically(
        &mc_dir,
        crate::launch::spawn::is_running(instance_id),
        |s| {
            Some(crate::l10n::options_txt::with_pack_enabled(
                s, &filename, true,
            ))
        },
    )?;

    Ok(activated)
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------
    // sweep_stale_lucerna_packs (+ the sweep-then-write sequence
    // `rebuild_pack` performs, exercised directly since it needs no
    // AppHandle)
    // -----------------------------------------------------------------

    /// A real generated override pack for `lang`, via `l10n::pack::build` —
    /// exercises the exact bytes `rebuild_pack` would produce, not a
    /// hand-rolled stand-in.
    fn generated_pack(lang: &str) -> Vec<u8> {
        let mut s = crate::l10n::store::NamespaceStore::new("create", lang);
        s.set("item.create.wrench", "x", "Wrench", 1.0);
        crate::l10n::pack::build(
            &[s],
            lang,
            crate::l10n::pack_format::PackFormat {
                major: 34,
                minor: 0,
            },
            "Lucerna translations",
        )
        .expect("has an override, known format")
    }

    /// A minimal but valid hand-authored resource pack — `pack.mcmeta` plus
    /// an `assets/` tree, no `data/` tree — so `install_asset_local`'s
    /// `pack_meta::classify` accepts it. Stands in for a pack the USER
    /// installed, which the sweep must never touch.
    fn minimal_resource_pack_zip() -> Vec<u8> {
        use std::io::Write;
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut w = zip::ZipWriter::new(&mut buf);
            let opts: zip::write::FileOptions<()> = zip::write::FileOptions::default();
            w.start_file("pack.mcmeta", opts).unwrap();
            w.write_all(br#"{"pack":{"pack_format":15,"description":"x"}}"#)
                .unwrap();
            w.start_file("assets/minecraft/textures/x.png", opts)
                .unwrap();
            w.write_all(b"\x89PNG").unwrap();
            w.finish().unwrap();
        }
        buf.into_inner()
    }

    fn resourcepacks_dir(inst_root: &std::path::Path) -> std::path::PathBuf {
        inst_root.join(".minecraft").join("resourcepacks")
    }

    async fn installed_resource_packs(
        inst_root: &std::path::Path,
    ) -> Vec<crate::mods::platform::InstalledAsset> {
        crate::mods::assets::list(inst_root, crate::mods::platform::ContentKind::ResourcePack)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn two_applies_with_different_languages_leave_exactly_one_pack_the_second() {
        let td = tempfile::tempdir().unwrap();
        let inst_root = td.path();

        let ru = format!("{}ru_ru.zip", crate::l10n::options_txt::PACK_PREFIX);
        sweep_stale_lucerna_packs(inst_root, Some(&ru)).await;
        crate::mods::asset_local::install_asset_local(
            inst_root,
            crate::mods::platform::ContentKind::ResourcePack,
            &ru,
            &generated_pack("ru_ru"),
        )
        .await
        .unwrap();

        let de = format!("{}de_de.zip", crate::l10n::options_txt::PACK_PREFIX);
        sweep_stale_lucerna_packs(inst_root, Some(&de)).await;
        crate::mods::asset_local::install_asset_local(
            inst_root,
            crate::mods::platform::ContentKind::ResourcePack,
            &de,
            &generated_pack("de_de"),
        )
        .await
        .unwrap();

        let installed = installed_resource_packs(inst_root).await;
        assert_eq!(
            installed.len(),
            1,
            "must leave exactly one pack: {installed:?}"
        );
        assert_eq!(
            installed[0].filename, de,
            "the survivor must be the SECOND apply"
        );

        // Not just the registry row — the ru_ru FILE must be gone too.
        assert!(!resourcepacks_dir(inst_root).join(&ru).exists());
        assert!(resourcepacks_dir(inst_root).join(&de).exists());
    }

    #[tokio::test]
    async fn reapplying_the_same_language_twice_still_leaves_exactly_one_pack() {
        // Confirms the sweep-then-write ordering doesn't race itself on a
        // same-filename re-apply: `mods::assets::add` already dedups by
        // `(kind, filename)` and `place_bytes` already overwrites atomically,
        // and the sweep explicitly excludes `keep_filename`, so the sweep
        // never touches the very file this call is about to (re)write.
        let td = tempfile::tempdir().unwrap();
        let inst_root = td.path();
        let filename = format!("{}ru_ru.zip", crate::l10n::options_txt::PACK_PREFIX);

        for _ in 0..2 {
            sweep_stale_lucerna_packs(inst_root, Some(&filename)).await;
            crate::mods::asset_local::install_asset_local(
                inst_root,
                crate::mods::platform::ContentKind::ResourcePack,
                &filename,
                &generated_pack("ru_ru"),
            )
            .await
            .unwrap();
        }

        let installed = installed_resource_packs(inst_root).await;
        assert_eq!(installed.len(), 1);
        assert_eq!(installed[0].filename, filename);
        assert!(resourcepacks_dir(inst_root).join(&filename).exists());
    }

    #[tokio::test]
    async fn sweep_with_no_keep_filename_removes_every_lucerna_pack() {
        // Mirrors the "nothing to ship" path in `rebuild_pack`: emptying
        // every override must leave no Lucerna pack behind at all.
        let td = tempfile::tempdir().unwrap();
        let inst_root = td.path();
        let filename = format!("{}ru_ru.zip", crate::l10n::options_txt::PACK_PREFIX);
        crate::mods::asset_local::install_asset_local(
            inst_root,
            crate::mods::platform::ContentKind::ResourcePack,
            &filename,
            &generated_pack("ru_ru"),
        )
        .await
        .unwrap();

        sweep_stale_lucerna_packs(inst_root, None).await;

        assert!(installed_resource_packs(inst_root).await.is_empty());
        assert!(!resourcepacks_dir(inst_root).join(&filename).exists());
    }

    #[tokio::test]
    async fn sweep_never_touches_a_pack_the_user_installed_by_hand() {
        let td = tempfile::tempdir().unwrap();
        let inst_root = td.path();
        crate::mods::asset_local::install_asset_local(
            inst_root,
            crate::mods::platform::ContentKind::ResourcePack,
            "Faithful.zip",
            &minimal_resource_pack_zip(),
        )
        .await
        .unwrap();

        sweep_stale_lucerna_packs(inst_root, None).await;

        let installed = installed_resource_packs(inst_root).await;
        assert_eq!(installed.len(), 1);
        assert_eq!(installed[0].filename, "Faithful.zip");
        assert!(resourcepacks_dir(inst_root).join("Faithful.zip").exists());
    }

    #[tokio::test]
    async fn sweep_on_an_instance_with_no_resource_packs_at_all_is_a_harmless_no_op() {
        let td = tempfile::tempdir().unwrap();
        sweep_stale_lucerna_packs(td.path(), None).await;
        assert!(installed_resource_packs(td.path()).await.is_empty());
    }

    #[test]
    fn assemble_current_pack_is_the_single_source_for_pack_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = crate::l10n::store::NamespaceStore::new("create", "ru_ru");
        s.set("item.create.wrench", "Ключ", "Wrench", 1.0);
        crate::l10n::store::save(dir.path(), &s).unwrap();
        let fmt = crate::l10n::pack_format::PackFormat {
            major: 34,
            minor: 0,
        };

        let a = assemble_current_pack(dir.path(), "ru_ru", fmt).expect("one override exists");
        let b = assemble_current_pack(dir.path(), "ru_ru", fmt).expect("deterministic");
        assert_eq!(a, b, "the same store must yield identical bytes");

        // It must also equal `pack::build` fed the same stores with the FROZEN
        // description — the exact string `rebuild_pack` shipped with. If this
        // assertion ever fails the description drifted, and every applied pack
        // would report 'outdated' forever while the determinism check above
        // stayed green.
        let stores = vec![crate::l10n::store::load(dir.path(), "ru_ru", "create")];
        let direct =
            crate::l10n::pack::build(&stores, "ru_ru", fmt, "Lucerna translations (ru_ru)")
                .unwrap();
        assert_eq!(a, direct);
    }

    #[test]
    fn pack_zip_timestamps_are_the_fixed_1980_constant() {
        // The whole outdated-vs-current byte comparison rests on the `zip`
        // crate's `time` feature staying OFF (Cargo.toml declares
        // default-features = false). With it on, every entry embeds the build's
        // wall-clock time and every instance reads 'outdated' forever. DOS
        // timestamps have two-second granularity, so a build-twice-and-compare
        // check cannot catch that — pin the stored timestamp itself.
        let fmt = crate::l10n::pack_format::PackFormat {
            major: 34,
            minor: 0,
        };
        let mut s = crate::l10n::store::NamespaceStore::new("create", "ru_ru");
        s.set("k", "v", "V", 1.0);
        let bytes = crate::l10n::pack::build(&[s], "ru_ru", fmt, "d").unwrap();
        let mut ar = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        for i in 0..ar.len() {
            let entry = ar.by_index(i).unwrap();
            // `zip::DateTime` derives PartialEq but NOT Debug, so this is an
            // `assert!` on a comparison rather than `assert_eq!`.
            assert!(
                entry.last_modified() == Some(zip::DateTime::default()),
                "entry {} carries a non-1980 timestamp — the zip crate's `time` \
                 feature got enabled somewhere, so pack bytes now embed build time",
                entry.name()
            );
        }
    }
}
