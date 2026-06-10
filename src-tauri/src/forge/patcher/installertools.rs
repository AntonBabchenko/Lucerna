//! installertools — Forge's multi-tool. First CLI arg is `--task <NAME>`.
//! Sub-commands implemented in Phase 2: EXTRACT_FILES (this task),
//! MCP_DATA (Task 7), BUNDLER_EXTRACT (Task 8, Phase 3 stub).
//!
//! v0.4.1 NeoForge matrix fix: PROCESS_MINECRAFT_JAR (NeoForge ≥ 21.10)
//! replaced the old ART + binarypatcher two-step in the 21.x installer. It
//! is implemented inside the installertools fatjar itself (unlike the
//! older ART / binarypatcher which are separate JARs). We shell out to
//! Java rather than re-implementing the bytecode remapping in Rust, same
//! rationale as ART and binarypatcher.
//!
//! Main-Class: `net.neoforged.installertools.ConsoleTool` (same ConsoleTool
//! that routes all tasks). The classpath already contains the fatjar —
//! we reuse it directly.

use crate::error::Result;
use crate::forge::patcher::{patcher_fail, ProcessorContext};
use std::path::PathBuf;

pub async fn run(
    _classifier: Option<&str>,
    args: Vec<String>,
    ctx: &ProcessorContext,
) -> Result<()> {
    let task = read_flag(&args, "--task")
        .ok_or_else(|| patcher_fail("installertools", &"missing --task <NAME>"))?;
    match task.as_str() {
        "EXTRACT_FILES" => extract_files(&args, ctx).await,
        "MCP_DATA" => mcp_data(&args, ctx).await,
        "BUNDLER_EXTRACT" => bundler_extract(&args, ctx).await,
        "DOWNLOAD_MOJMAPS" => download_mojmaps(&args, ctx).await,
        "MERGE_MAPPING" => merge_mapping(&args, ctx).await,
        // NeoForge ≥ 21.10: PROCESS_MINECRAFT_JAR replaces the separate ART +
        // binarypatcher two-step. Shell out to Java (byte-fidelity required).
        "PROCESS_MINECRAFT_JAR" => process_minecraft_jar_via_java(&args, ctx).await,
        other => Err(crate::error::Error::ForgeUnsupportedProcessor {
            coord: format!("net.minecraftforge:installertools (task={other})"),
        }),
    }
}

pub(super) fn read_flag(args: &[String], name: &str) -> Option<String> {
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == name {
            return it.next().cloned();
        }
    }
    None
}

pub(super) fn read_all_flag(args: &[String], name: &str) -> Vec<String> {
    let mut out = vec![];
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == name {
            if let Some(v) = it.next() {
                out.push(v.clone());
            }
        }
    }
    out
}

async fn extract_files(args: &[String], _ctx: &ProcessorContext) -> Result<()> {
    use std::io::Read;
    use tokio::fs;
    let archive = read_flag(args, "--archive")
        .ok_or_else(|| patcher_fail("installertools::EXTRACT_FILES", &"missing --archive"))?;
    let output = read_flag(args, "--output")
        .ok_or_else(|| patcher_fail("installertools::EXTRACT_FILES", &"missing --output"))?;
    let entries = read_all_flag(args, "--from");
    if entries.is_empty() {
        return Err(patcher_fail(
            "installertools::EXTRACT_FILES",
            &"no --from entries",
        ));
    }
    let bytes = fs::read(&archive).await.map_err(|e| {
        patcher_fail(
            "installertools::EXTRACT_FILES",
            &format!("read {archive}: {e}"),
        )
    })?;
    let mut zip_archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| {
        patcher_fail(
            "installertools::EXTRACT_FILES",
            &format!("zip open {archive}: {e}"),
        )
    })?;
    fs::create_dir_all(&output).await.map_err(|e| {
        patcher_fail(
            "installertools::EXTRACT_FILES",
            &format!("mkdir {output}: {e}"),
        )
    })?;
    for name in entries {
        let (buf, basename) = {
            let mut entry = zip_archive.by_name(&name).map_err(|_| {
                patcher_fail(
                    "installertools::EXTRACT_FILES",
                    &format!("entry not found: {name}"),
                )
            })?;
            let mut buf = Vec::with_capacity(entry.size() as usize);
            entry.read_to_end(&mut buf).map_err(|e| {
                patcher_fail(
                    "installertools::EXTRACT_FILES",
                    &format!("read {name}: {e}"),
                )
            })?;
            let basename = std::path::Path::new(&name)
                .file_name()
                .ok_or_else(|| {
                    patcher_fail(
                        "installertools::EXTRACT_FILES",
                        &format!("no basename: {name}"),
                    )
                })?
                .to_string_lossy()
                .to_string();
            (buf, basename)
            // `entry` (ZipFile, not Send) dropped here before any await
        };
        let dest = PathBuf::from(&output).join(basename);
        fs::write(&dest, &buf).await.map_err(|e| {
            patcher_fail(
                "installertools::EXTRACT_FILES",
                &format!("write {}: {e}", dest.display()),
            )
        })?;
    }
    Ok(())
}

async fn mcp_data(args: &[String], _ctx: &ProcessorContext) -> Result<()> {
    use std::io::Read;
    use tokio::fs;
    let input = read_flag(args, "--input")
        .ok_or_else(|| patcher_fail("installertools::MCP_DATA", &"missing --input"))?;
    let output = read_flag(args, "--output")
        .ok_or_else(|| patcher_fail("installertools::MCP_DATA", &"missing --output"))?;
    let key = read_flag(args, "--key").unwrap_or_else(|| "mappings".into());

    let bytes = fs::read(&input)
        .await
        .map_err(|e| patcher_fail("installertools::MCP_DATA", &format!("read {input}: {e}")))?;
    // Synchronous zip block — ZipFile<'_> is not Send, don't hold across .await.
    let (_, file_bytes) = {
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
            .map_err(|e| patcher_fail("installertools::MCP_DATA", &format!("zip open: {e}")))?;

        // 1. Read config.json manifest.
        let manifest_text = {
            let mut e = archive.by_name("config.json").map_err(|_| {
                patcher_fail(
                    "installertools::MCP_DATA",
                    &format!("no config.json in {input}"),
                )
            })?;
            let mut s = String::with_capacity(e.size() as usize);
            e.read_to_string(&mut s).map_err(|err| {
                patcher_fail(
                    "installertools::MCP_DATA",
                    &format!("read config.json: {err}"),
                )
            })?;
            s
        };
        let manifest: serde_json::Value = serde_json::from_str(&manifest_text).map_err(|e| {
            patcher_fail(
                "installertools::MCP_DATA",
                &format!("parse config.json: {e}"),
            )
        })?;

        // 2. Look up data.<key> → relative path string.
        let rel = manifest
            .get("data")
            .and_then(|d| d.get(&key))
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                patcher_fail(
                    "installertools::MCP_DATA",
                    &format!("config.json missing data.{key} (or not a string)"),
                )
            })?
            .to_string();
        if rel.ends_with('/') {
            return Err(patcher_fail(
                "installertools::MCP_DATA",
                &format!("data.{key} points to a directory ({rel}) — Phase 2 doesn't support directory extraction"),
            ));
        }

        // 3. Extract that one file.
        let mut entry = archive.by_name(&rel).map_err(|_| {
            patcher_fail(
                "installertools::MCP_DATA",
                &format!("entry {rel} (from data.{key}) not in zip"),
            )
        })?;
        let mut file_bytes = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut file_bytes).map_err(|err| {
            patcher_fail("installertools::MCP_DATA", &format!("read {rel}: {err}"))
        })?;
        (rel, file_bytes)
    };

    // 4. Write to --output (file path). Create parent dir if needed.
    let out_path = std::path::PathBuf::from(&output);
    if let Some(parent) = out_path.parent() {
        fs::create_dir_all(parent).await.map_err(|e| {
            patcher_fail(
                "installertools::MCP_DATA",
                &format!("mkdir {}: {e}", parent.display()),
            )
        })?;
    }
    fs::write(&out_path, &file_bytes).await.map_err(|e| {
        patcher_fail(
            "installertools::MCP_DATA",
            &format!("write {}: {e}", out_path.display()),
        )
    })?;
    Ok(())
}

async fn bundler_extract(_args: &[String], _ctx: &ProcessorContext) -> Result<()> {
    // BUNDLER_EXTRACT unpacks Minecraft's bundled server jar (1.18+) into
    // its embedded libraries + plain server jar. Lucerna is client-only,
    // and the modern install_profile gates this processor behind
    // `sides: ["server"]` — the sides filter in modern::install /
    // transitional::install skips it before reaching here. If this error
    // fires, the sides filter is missing or wrong; investigate the caller,
    // do not implement here.
    Err(crate::error::Error::ForgeUnsupportedProcessor {
        coord: "net.minecraftforge:installertools (task=BUNDLER_EXTRACT) — \
                server-only, client install does not invoke this. \
                If reached, sides filter is missing in the caller."
            .into(),
    })
}

/// Shell out to the `installertools:4.0.6:fatjar` for `PROCESS_MINECRAFT_JAR`.
///
/// NeoForge ≥ 21.10 replaced the separate ART + binarypatcher two-step with a
/// single all-in-one task inside `installertools` itself. The JAR's Main-Class
/// is `net.neoforged.installertools.ConsoleTool`, which re-routes via `--task`.
/// The classpath is already resolved by `run_processor`'s dispatch path (the
/// fatjar is in `p.classpath`) so we reuse it directly.
///
/// Byte-fidelity is required (bytecode remapping + binary patching combined),
/// so we must shell out rather than reimplement.
async fn process_minecraft_jar_via_java(args: &[String], ctx: &ProcessorContext) -> Result<()> {
    let java_bin = ctx
        .java_bin
        .clone()
        .unwrap_or_else(|| PathBuf::from("java"));
    crate::process::run_java_processor(
        &java_bin,
        &ctx.classpath,
        "net.neoforged.installertools.ConsoleTool",
        args,
        "installertools::PROCESS_MINECRAFT_JAR",
    )
    .await
}

/// Extract a required, non-empty SHA-1 hex from a Mojang
/// `downloads.<side>_mappings` object. Vanilla Mojang artifacts have no
/// TOFU exception (Principle B.6 — "always SHA-1-verified, no exception"),
/// so a missing, empty, or non-string `sha1` is a hard error rather than the
/// old `unwrap_or("")`, which fed an empty expected hash to the downloader
/// and silently skipped verification.
fn require_mapping_sha1<'a>(dl: &'a serde_json::Value, key: &str) -> Result<&'a str> {
    dl.get("sha1")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            patcher_fail(
                "installertools::DOWNLOAD_MOJMAPS",
                &format!(
                    "downloads.{key}.sha1 missing or empty — refusing to install an unverified Mojang mapping (B.6)"
                ),
            )
        })
}

async fn download_mojmaps(args: &[String], _ctx: &ProcessorContext) -> Result<()> {
    use tokio::fs;
    let version = read_flag(args, "--version")
        .ok_or_else(|| patcher_fail("installertools::DOWNLOAD_MOJMAPS", &"missing --version"))?;
    let side = read_flag(args, "--side")
        .ok_or_else(|| patcher_fail("installertools::DOWNLOAD_MOJMAPS", &"missing --side"))?;
    let output = read_flag(args, "--output")
        .ok_or_else(|| patcher_fail("installertools::DOWNLOAD_MOJMAPS", &"missing --output"))?;
    let sanitize = args.iter().any(|a| a == "--sanitize");

    if side != "client" && side != "server" {
        return Err(patcher_fail(
            "installertools::DOWNLOAD_MOJMAPS",
            &format!("unknown side `{side}` (expected client|server)"),
        ));
    }

    // 1. Fetch Mojang version manifest, find the entry for `version`.
    let manifest_entries = crate::versions::manifest::list_manifest().await?;
    let entry = manifest_entries
        .iter()
        .find(|e| e.id == version)
        .ok_or_else(|| {
            patcher_fail(
                "installertools::DOWNLOAD_MOJMAPS",
                &format!("MC version `{version}` not present in Mojang manifest"),
            )
        })?;

    // 2. Fetch per-version JSON to get downloads.<side>_mappings.url.
    let version_json: serde_json::Value =
        crate::network::get_json(&entry.url, "forge-mojmaps").await?;
    let key = format!("{side}_mappings");
    let dl = version_json
        .get("downloads")
        .and_then(|d| d.get(&key))
        .ok_or_else(|| {
            patcher_fail(
                "installertools::DOWNLOAD_MOJMAPS",
                &format!("version JSON has no downloads.{key} (vanilla MC {version} may predate Mojang mapping publishing)"),
            )
        })?;
    let url = dl.get("url").and_then(|v| v.as_str()).ok_or_else(|| {
        patcher_fail(
            "installertools::DOWNLOAD_MOJMAPS",
            &format!("downloads.{key}.url missing or not a string"),
        )
    })?;
    let sha1 = require_mapping_sha1(dl, &key)?;

    // 3. Download.
    if let Some(parent) = std::path::Path::new(&output).parent() {
        fs::create_dir_all(parent).await.map_err(|e| {
            patcher_fail(
                "installertools::DOWNLOAD_MOJMAPS",
                &format!("mkdir {}: {e}", parent.display()),
            )
        })?;
    }
    crate::network::download::download_no_emit(
        url,
        std::path::Path::new(&output),
        sha1,
        "forge-mojmaps",
    )
    .await
    .map_err(|e| {
        patcher_fail(
            "installertools::DOWNLOAD_MOJMAPS",
            &format!("download {url}: {e:?}"),
        )
    })?;

    // 4. Optional sanitize pass: strip `package-info` mapping lines and
    //    standalone comments. Mojang mappings are in Proguard format
    //    ("a.b.C -> x.y.Z:" headers + indented members).
    if sanitize {
        let raw = fs::read_to_string(&output).await.map_err(|e| {
            patcher_fail(
                "installertools::DOWNLOAD_MOJMAPS",
                &format!("read {output} for sanitize: {e}"),
            )
        })?;
        let cleaned = sanitize_proguard_mappings(&raw);
        fs::write(&output, &cleaned).await.map_err(|e| {
            patcher_fail(
                "installertools::DOWNLOAD_MOJMAPS",
                &format!("write {output} after sanitize: {e}"),
            )
        })?;
    }

    Ok(())
}

/// Strip Proguard-mapping noise: pure-comment lines (`# ...`) and any
/// class-header entry whose obfuscated name ends with `.package-info`
/// (along with its indented member block). Mirrors Forge installertools'
/// `--sanitize` flag — the merge step downstream chokes on package-info
/// classes that lack member bodies.
pub(crate) fn sanitize_proguard_mappings(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut skip_block = false;
    for line in input.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') {
            // Drop comments entirely; safe because Proguard format permits them only
            // as standalone lines, not on the same line as data.
            continue;
        }
        // Class-header lines start in column 0 ("foo.bar.Baz -> x.y.Z:").
        let is_class_header = !line.is_empty() && !line.starts_with(' ') && !line.starts_with('\t');
        if is_class_header {
            skip_block = false;
            if let Some(arrow) = line.find(" -> ") {
                let obf = &line[..arrow];
                if obf.ends_with(".package-info") || obf.ends_with("$package-info") {
                    skip_block = true;
                    continue;
                }
            }
        }
        if skip_block {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

async fn merge_mapping(args: &[String], _ctx: &ProcessorContext) -> Result<()> {
    use tokio::fs;
    // installertools 2.x used `--left/--right/--reverse-right`;
    // installertools 3.0.13+ (NeoForge 21.8+) renamed these to
    // `--merge/--base/--reverse-base` with identical semantics
    // (merge=left=overlay, base=right=foundation, reverse-base
    // flips the base side's direction). Older arg names also lost
    // the `--classes/--fields/--methods` flags — 3.x always merges
    // all kinds. Accept both naming conventions.
    let left = read_flag(args, "--left")
        .or_else(|| read_flag(args, "--merge"))
        .ok_or_else(|| patcher_fail("installertools::MERGE_MAPPING", &"missing --left/--merge"))?;
    let right = read_flag(args, "--right")
        .or_else(|| read_flag(args, "--base"))
        .ok_or_else(|| patcher_fail("installertools::MERGE_MAPPING", &"missing --right/--base"))?;
    let output = read_flag(args, "--output")
        .ok_or_else(|| patcher_fail("installertools::MERGE_MAPPING", &"missing --output"))?;
    // 3.x always merges classes+fields+methods (no toggles); 2.x had explicit flags.
    let is_3x_naming = args
        .iter()
        .any(|a| a == "--merge" || a == "--base" || a == "--reverse-base");
    let merge_classes = is_3x_naming || args.iter().any(|a| a == "--classes");
    let merge_fields = is_3x_naming || args.iter().any(|a| a == "--fields");
    let merge_methods = is_3x_naming || args.iter().any(|a| a == "--methods");
    let reverse_right = args
        .iter()
        .any(|a| a == "--reverse-right" || a == "--reverse-base");

    let left_text = fs::read_to_string(&left).await.map_err(|e| {
        patcher_fail(
            "installertools::MERGE_MAPPING",
            &format!("read {left}: {e}"),
        )
    })?;
    let right_text = fs::read_to_string(&right).await.map_err(|e| {
        patcher_fail(
            "installertools::MERGE_MAPPING",
            &format!("read {right}: {e}"),
        )
    })?;

    // Parse right (Proguard) mapping.
    // class_map: obf_class → mojang_class (when reverse_right=true)
    // field_map: (obf_class, obf_field) → mojang_field_name
    // method_map: (obf_class, obf_method_name, obf_descriptor) → mojang_method_name
    //   Descriptors in Proguard use source (Mojang) class names; we translate
    //   them back to obfuscated descriptors so keys match the TSRG descriptor.
    let (class_map, field_map, method_map) = parse_proguard_full(&right_text, reverse_right);

    // Detect TSRG version of left input. v2 opens with `tsrg2 <name1> ...`;
    // v1 has no header.
    let header_line: Option<&str> = left_text
        .lines()
        .find(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'));
    let is_v2 = header_line.is_some_and(|l| l.split_whitespace().next() == Some("tsrg2"));

    // Output format: Java installertools MERGE_MAPPING always writes `tsrg2 left right`
    // as the header (regardless of input column names) and strips the trailing `id`
    // column from every line — producing exactly 2 name slots (obf + merged-name).
    // This is what downstream ART/srgutils expects.
    let mut out = String::with_capacity(left_text.len());
    if is_v2 {
        out.push_str("tsrg2 left right\n");
    }

    // Current obf class (used as key into field/method maps).
    let mut current_obf_class = String::new();

    for line in left_text.lines() {
        if line.is_empty() || line.trim_start().starts_with('#') {
            continue;
        }

        // Detect indentation level.
        let tab_depth = line.chars().take_while(|c| *c == '\t').count();

        if tab_depth >= 2 {
            // Nested parameter line (TSRG v2) or `static` keyword.
            // Java tool strips the trailing id column from param lines.
            // Format: \t\tstatic   → emit as-is (1 token)
            //         \t\tidx obf param_name [id] → emit first 3 tokens only
            let content = &line[2..]; // strip 2 leading tabs
            let tokens: Vec<&str> = content.split_whitespace().collect();
            out.push('\t');
            out.push('\t');
            if tokens.is_empty() {
                out.push('\n');
                continue;
            }
            if tokens[0] == "static" {
                out.push_str("static\n");
            } else {
                // idx obf param_name [id] → only first 3 tokens
                for (i, tok) in tokens.iter().take(3).enumerate() {
                    if i > 0 {
                        out.push(' ');
                    }
                    out.push_str(tok);
                }
                out.push('\n');
            }
            continue;
        }

        if tab_depth == 1 {
            // Member line.
            let trimmed = &line[1..]; // strip leading tab
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.is_empty() {
                continue;
            }

            // TSRG v2 field:  obf_name  srg_name  [id]
            // TSRG v2 method: obf_name  descriptor  srg_name  [id]
            // Distinguish field vs method by checking parts[1] for '(' prefix.
            let is_method = parts.get(1).map(|p| p.starts_with('(')).unwrap_or(false);

            if is_method && parts.len() >= 3 {
                // parts: [obf_name, descriptor, srg_name, id?]
                let obf_name = parts[0];
                let descriptor = parts[1];
                let srg_name = parts[2];
                let named = if merge_methods {
                    // Key includes the obfuscated descriptor for disambiguation
                    // when a class has multiple overloads with the same obf name.
                    let key = (
                        current_obf_class.clone(),
                        obf_name.to_string(),
                        descriptor.to_string(),
                    );
                    method_map.get(&key).map(String::as_str).unwrap_or(srg_name)
                } else {
                    srg_name
                };
                // Always emit exactly: \tobf descriptor named  (id stripped)
                out.push('\t');
                out.push_str(obf_name);
                out.push(' ');
                out.push_str(descriptor);
                out.push(' ');
                out.push_str(named);
                out.push('\n');
            } else if !is_method && parts.len() >= 2 {
                // parts: [obf_name, srg_name, id?]
                let obf_name = parts[0];
                let srg_name = parts[1];
                let named = if merge_fields {
                    let key = (current_obf_class.clone(), obf_name.to_string());
                    field_map.get(&key).map(String::as_str).unwrap_or(srg_name)
                } else {
                    srg_name
                };
                // Always emit exactly: \tobf named  (id stripped)
                out.push('\t');
                out.push_str(obf_name);
                out.push(' ');
                out.push_str(named);
                out.push('\n');
            } else {
                // 1-token member line (unusual but pass through first token only).
                out.push('\t');
                out.push_str(parts[0]);
                out.push('\n');
            }
            continue;
        }

        // tab_depth == 0: class-header line or tsrg2 header.
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }
        if parts[0] == "tsrg2" {
            // v2 header — already written above; skip the input header line.
            continue;
        }
        if parts.len() < 2 {
            // Malformed class line; skip.
            continue;
        }
        let obf = parts[0];
        let srg = parts[1];

        // Track current obf class for member lookups below.
        current_obf_class = obf.to_string();

        let named = if merge_classes {
            class_map.get(obf).map(String::as_str).unwrap_or(srg)
        } else {
            srg
        };
        // Always emit exactly: obf named  (id stripped, only 2 columns)
        out.push_str(obf);
        out.push(' ');
        out.push_str(named);
        out.push('\n');
    }

    if let Some(parent) = std::path::Path::new(&output).parent() {
        fs::create_dir_all(parent).await.map_err(|e| {
            patcher_fail(
                "installertools::MERGE_MAPPING",
                &format!("mkdir {}: {e}", parent.display()),
            )
        })?;
    }
    fs::write(&output, &out).await.map_err(|e| {
        patcher_fail(
            "installertools::MERGE_MAPPING",
            &format!("write {output}: {e}"),
        )
    })?;
    Ok(())
}

/// Walk a TSRG (v1 or v2) text body; collect (obf, srg) pairs from class-header
/// lines (lines without leading tab/space). Ignores members and comments.
///
/// TSRG v2 (1.17+ mcp_config) opens with a `tsrg2 obf srg [id...]` declaration
/// line listing the mapping-name columns. Class lines below have the same
/// 2+-column shape, so we treat the first column as obf and the second as srg
/// uniformly — but we MUST skip the v2 header itself, otherwise it leaks into
/// the merged output and downstream srgutils interprets our output as
/// malformed v2 (only 2 cols where 3+ are declared) and fails the install.
pub(crate) fn parse_tsrg_class_headers(body: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for line in body.lines() {
        if line.is_empty()
            || line.starts_with('\t')
            || line.starts_with(' ')
            || line.starts_with('#')
        {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }
        // Skip the TSRG v2 format-declaration line — it would otherwise
        // be emitted as a class pair and confuse downstream srgutils.
        if parts[0] == "tsrg2" {
            continue;
        }
        if parts.len() >= 2 {
            out.push((parts[0].to_string(), parts[1].to_string()));
        }
    }
    out
}

/// Walk Proguard mappings text; emit a map of obf-class → named-class
/// (when `reverse` is true) or named-class → obf-class (when false).
/// Normalises `.` to `/` for output values to match TSRG class-name format.
pub(crate) fn parse_proguard_class_headers(
    body: &str,
    reverse: bool,
) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    for line in body.lines() {
        // Class header is a left-aligned line ending with " -> X:" where
        // member lines start with whitespace.
        if line.starts_with(' ')
            || line.starts_with('\t')
            || line.starts_with('#')
            || line.is_empty()
        {
            continue;
        }
        let Some(arrow) = line.find(" -> ") else {
            continue;
        };
        let named = line[..arrow].replace('.', "/");
        let after = &line[arrow + 4..];
        let obf = after.trim_end_matches(':').replace('.', "/");
        if reverse {
            out.insert(obf, named);
        } else {
            out.insert(named, obf);
        }
    }
    out
}

/// Parse a Proguard mapping into class, field, and method lookup tables.
///
/// Returns `(class_map, field_map, method_map)` where:
/// - `class_map`: obf_class_name → mojang_class_name  (when `reverse=true`)
/// - `field_map`: (obf_class, obf_field_name) → mojang_field_name
/// - `method_map`: (obf_class, obf_method_name, obf_descriptor) → mojang_method_name
///
/// Proguard format (reverse_right=true means mojang→obf, reversed to obf→mojang):
/// ```text
/// com.example.Foo -> a:
///     int myField -> b
///     int:12:void myMethod(int) -> c
/// ```
/// After reversal: class `a` → `com/example/Foo`, field `(a, b)` → `myField`,
/// method `(a, c, "(I)V")` → `myMethod`.
///
/// Note: method_map keys include the **obfuscated** JVM descriptor. Proguard
/// descriptors are written using Mojang/source class names; we translate them
/// back to obfuscated names via the class map so that they match TSRG descriptors.
/// This is required for correct disambiguation of overloaded methods.
fn parse_proguard_full(
    body: &str,
    reverse: bool,
) -> (
    std::collections::HashMap<String, String>,
    std::collections::HashMap<(String, String), String>,
    std::collections::HashMap<(String, String, String), String>,
) {
    use std::collections::HashMap;
    let mut class_map: HashMap<String, String> = HashMap::new();
    // named_to_obf: mojang-class-name → obf-class-name, used to convert
    // Proguard source descriptors to obfuscated JVM descriptors.
    let mut named_to_obf: HashMap<String, String> = HashMap::new();
    let mut field_map: HashMap<(String, String), String> = HashMap::new();
    let mut method_map: HashMap<(String, String, String), String> = HashMap::new();

    // ── Pass 1: build class maps ─────────────────────────────────────────────
    for line in body.lines() {
        if line.starts_with('#')
            || line.is_empty()
            || line.starts_with(' ')
            || line.starts_with('\t')
        {
            continue;
        }
        let Some(arrow_pos) = line.find(" -> ") else {
            continue;
        };
        let named = line[..arrow_pos].replace('.', "/");
        let after = &line[arrow_pos + 4..];
        let obf = after.trim_end_matches(':').replace('.', "/");
        if reverse {
            class_map.insert(obf.clone(), named.clone());
        } else {
            class_map.insert(named.clone(), obf.clone());
        }
        // Always build named_to_obf regardless of `reverse` — we need it to
        // translate Proguard source-type descriptors to obfuscated descriptors.
        named_to_obf.insert(named, obf);
    }

    // ── Pass 2: build field and method maps ──────────────────────────────────
    let mut current_obf_class = String::new();
    for line in body.lines() {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        if line.starts_with(' ') || line.starts_with('\t') {
            // Member line.
            if current_obf_class.is_empty() {
                continue;
            }
            let trimmed = line.trim();
            let Some(arrow_pos) = trimmed.find(" -> ") else {
                continue;
            };
            let left_part = &trimmed[..arrow_pos];
            let right_part = &trimmed[arrow_pos + 4..];
            let obf_member = right_part.trim();
            let left_stripped = strip_line_number_prefix(left_part);

            if let Some(paren_pos) = left_stripped.find('(') {
                // Method: "return_type mojang_name(args,...)"
                let before_paren = &left_stripped[..paren_pos];
                let args_end = left_stripped.rfind(')').unwrap_or(left_stripped.len());
                let args_str = &left_stripped[paren_pos + 1..args_end];
                let parts: Vec<&str> = before_paren.split_whitespace().collect();
                let return_type = parts.first().copied().unwrap_or("void");
                let mojang_name = parts.last().copied().unwrap_or("");
                if !mojang_name.is_empty() && !obf_member.is_empty() {
                    // Compute the obfuscated JVM descriptor by translating
                    // Proguard source class names → obfuscated names.
                    let obf_descriptor = build_obf_descriptor(return_type, args_str, &named_to_obf);
                    if reverse {
                        method_map.insert(
                            (
                                current_obf_class.clone(),
                                obf_member.to_string(),
                                obf_descriptor,
                            ),
                            mojang_name.to_string(),
                        );
                    } else {
                        method_map.insert(
                            (
                                current_obf_class.clone(),
                                mojang_name.to_string(),
                                obf_descriptor,
                            ),
                            obf_member.to_string(),
                        );
                    }
                }
            } else {
                // Field: "type mojang_name"
                let mojang_name = left_stripped.split_whitespace().last().unwrap_or("");
                if !mojang_name.is_empty() && !obf_member.is_empty() {
                    if reverse {
                        field_map.insert(
                            (current_obf_class.clone(), obf_member.to_string()),
                            mojang_name.to_string(),
                        );
                    } else {
                        field_map.insert(
                            (current_obf_class.clone(), mojang_name.to_string()),
                            obf_member.to_string(),
                        );
                    }
                }
            }
        } else {
            // Class header line.
            let Some(arrow_pos) = line.find(" -> ") else {
                continue;
            };
            let after = &line[arrow_pos + 4..];
            let obf = after.trim_end_matches(':').replace('.', "/");
            current_obf_class = obf;
        }
    }

    (class_map, field_map, method_map)
}

/// Build an obfuscated JVM method descriptor from Proguard source-type strings.
///
/// Proguard uses Java source type names (e.g. `int`, `net.minecraft.WorldVersion`).
/// TSRG descriptors use obfuscated JVM type names (e.g. `I`, `Lad;`).
/// We convert each source type via `named_to_obf` (mojang_class → obf_class).
///
/// Examples:
/// - `return_type="void", args="net.minecraft.WorldVersion"` → `"(Lad;)V"`
/// - `return_type="org.joml.Quaternionf", args="float"` → `"(F)Lorg/joml/Quaternionf;"`
///   (org.joml.Quaternionf is not in named_to_obf, so falls back to its JVM form)
fn build_obf_descriptor(
    return_type: &str,
    args_str: &str,
    named_to_obf: &std::collections::HashMap<String, String>,
) -> String {
    let args: Vec<&str> = if args_str.trim().is_empty() {
        vec![]
    } else {
        args_str.split(',').collect()
    };
    let mut desc = String::with_capacity(args.len() * 4 + 4);
    desc.push('(');
    for arg in &args {
        source_type_to_jvm_token(arg.trim(), named_to_obf, &mut desc);
    }
    desc.push(')');
    source_type_to_jvm_token(return_type.trim(), named_to_obf, &mut desc);
    desc
}

/// Append the JVM type descriptor token for a single Java source type.
/// `type_str` may be a primitive, a qualified class name, or either with `[]` suffixes.
fn source_type_to_jvm_token(
    type_str: &str,
    named_to_obf: &std::collections::HashMap<String, String>,
    out: &mut String,
) {
    let (base, dims) = {
        let mut s = type_str;
        let mut d = 0usize;
        while s.ends_with("[]") {
            s = &s[..s.len() - 2];
            d += 1;
        }
        (s, d)
    };
    for _ in 0..dims {
        out.push('[');
    }
    match base {
        "boolean" => out.push('Z'),
        "byte" => out.push('B'),
        "char" => out.push('C'),
        "short" => out.push('S'),
        "int" => out.push('I'),
        "long" => out.push('J'),
        "float" => out.push('F'),
        "double" => out.push('D'),
        "void" => out.push('V'),
        _ => {
            let jvm_class = base.replace('.', "/");
            let obf_class = named_to_obf
                .get(&jvm_class)
                .map(String::as_str)
                .unwrap_or(&jvm_class);
            out.push('L');
            out.push_str(obf_class);
            out.push(';');
        }
    }
}

/// Strip the leading "start:end:" or "start:" line-number prefix from a
/// Proguard method descriptor. E.g. "12:34:void" → "void", "void" → "void".
fn strip_line_number_prefix(s: &str) -> &str {
    // Count leading numeric:numeric: segments.
    let mut rest = s;
    loop {
        // Does rest start with digits followed by ':'?
        let colon = rest.find(':');
        match colon {
            Some(i) if rest[..i].chars().all(|c| c.is_ascii_digit()) => {
                rest = &rest[i + 1..];
            }
            _ => break,
        }
    }
    rest
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn extract_files_unknown_task_errors() {
        let ctx = ProcessorContext {
            classpath: vec![],
            cache_dir: PathBuf::from("."),
            java_bin: None,
        };
        let args = vec!["--task".into(), "BOGUS".into()];
        assert!(run(None, args, &ctx).await.is_err());
    }

    #[tokio::test]
    async fn extract_files_happy_path() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("in.zip");
        let out_dir = dir.path().join("out");
        let f = std::fs::File::create(&zip_path).unwrap();
        let mut w = zip::ZipWriter::new(f);
        let opts: zip::write::FileOptions<()> = zip::write::FileOptions::default();
        w.start_file::<_, ()>("data/hello.txt", opts).unwrap();
        w.write_all(b"hi").unwrap();
        w.finish().unwrap();

        let ctx = ProcessorContext {
            classpath: vec![],
            cache_dir: dir.path().to_path_buf(),
            java_bin: None,
        };
        let args = vec![
            "--task".into(),
            "EXTRACT_FILES".into(),
            "--archive".into(),
            zip_path.display().to_string(),
            "--from".into(),
            "data/hello.txt".into(),
            "--output".into(),
            out_dir.display().to_string(),
        ];
        run(None, args, &ctx).await.expect("extract");
        let got = std::fs::read_to_string(out_dir.join("hello.txt")).unwrap();
        assert_eq!(got, "hi");
    }

    #[tokio::test]
    async fn mcp_data_extracts_via_manifest_lookup() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("mcp_config.zip");
        let out_path = dir.path().join("out").join("mappings.txt");
        let f = std::fs::File::create(&zip_path).unwrap();
        let mut w = zip::ZipWriter::new(f);
        let opts: zip::write::FileOptions<()> = zip::write::FileOptions::default();
        w.start_file::<_, ()>("config.json", opts).unwrap();
        w.write_all(br#"{"data":{"mappings":"config/joined.tsrg"}}"#)
            .unwrap();
        w.start_file::<_, ()>("config/joined.tsrg", opts).unwrap();
        w.write_all(b"net/x/Y net/x/Y\n").unwrap();
        w.start_file::<_, ()>("other/skip.txt", opts).unwrap();
        w.write_all(b"ignored").unwrap();
        w.finish().unwrap();

        let ctx = ProcessorContext {
            classpath: vec![],
            cache_dir: dir.path().to_path_buf(),
            java_bin: None,
        };
        let args = vec![
            "--task".into(),
            "MCP_DATA".into(),
            "--input".into(),
            zip_path.display().to_string(),
            "--output".into(),
            out_path.display().to_string(),
            "--key".into(),
            "mappings".into(),
        ];
        run(None, args, &ctx).await.expect("mcp_data");
        let got = std::fs::read_to_string(&out_path).unwrap();
        assert!(
            got.contains("net/x/Y"),
            "output should be the extracted joined.tsrg"
        );
    }

    #[tokio::test]
    async fn mcp_data_rejects_zip_slip_via_manifest() {
        // Adversarial config.json points data.mappings at "../escape.txt".
        // archive.by_name("../escape.txt") will fail with NotFound because
        // there is no such entry in the zip — the error path triggers cleanly.
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("evil.zip");
        let out_path = dir.path().join("out").join("mappings.txt");
        let f = std::fs::File::create(&zip_path).unwrap();
        let mut w = zip::ZipWriter::new(f);
        let opts: zip::write::FileOptions<()> = zip::write::FileOptions::default();
        w.start_file::<_, ()>("config.json", opts).unwrap();
        w.write_all(br#"{"data":{"mappings":"../escape.txt"}}"#)
            .unwrap();
        w.finish().unwrap();

        let ctx = ProcessorContext {
            classpath: vec![],
            cache_dir: dir.path().to_path_buf(),
            java_bin: None,
        };
        let args = vec![
            "--task".into(),
            "MCP_DATA".into(),
            "--input".into(),
            zip_path.display().to_string(),
            "--output".into(),
            out_path.display().to_string(),
            "--key".into(),
            "mappings".into(),
        ];
        let err = run(None, args, &ctx).await.unwrap_err();
        match err {
            crate::error::Error::ForgePatcherFailed { processor, .. } => {
                assert_eq!(processor, "installertools::MCP_DATA");
            }
            other => panic!("expected ForgePatcherFailed, got {other:?}"),
        }
        assert!(
            !dir.path().join("escape.txt").exists(),
            "escape file must not exist"
        );
    }

    #[tokio::test]
    async fn bundler_extract_is_server_only_stub() {
        let ctx = ProcessorContext {
            classpath: vec![],
            cache_dir: PathBuf::from("."),
            java_bin: None,
        };
        let args = vec!["--task".into(), "BUNDLER_EXTRACT".into()];
        let err = run(None, args, &ctx).await.unwrap_err();
        match err {
            crate::error::Error::ForgeUnsupportedProcessor { coord } => {
                assert!(coord.contains("BUNDLER_EXTRACT"));
                assert!(coord.contains("server-only") || coord.contains("client install"));
            }
            other => panic!("got {other:?}"),
        }
    }

    #[tokio::test]
    async fn process_minecraft_jar_does_not_return_unsupported_processor() {
        // PROCESS_MINECRAFT_JAR (NeoForge ≥ 21.10) must NOT return
        // ForgeUnsupportedProcessor — it should attempt the Java shell-out.
        // With an empty classpath and no real installertools fatjar, it will
        // fail with ForgePatcherFailed (spawn failed or non-zero exit), not
        // ForgeUnsupportedProcessor. That proves dispatch reached the Java
        // shell-out path.
        let ctx = ProcessorContext {
            classpath: vec![],
            cache_dir: PathBuf::from("."),
            java_bin: Some(PathBuf::from("java")),
        };
        let args = vec![
            "--task".into(),
            "PROCESS_MINECRAFT_JAR".into(),
            "--input".into(),
            "/nonexistent/mc.jar".into(),
            "--output".into(),
            "/nonexistent/patched.jar".into(),
        ];
        let err = run(None, args, &ctx).await.unwrap_err();
        assert!(
            !matches!(err, crate::error::Error::ForgeUnsupportedProcessor { .. }),
            "PROCESS_MINECRAFT_JAR must not return ForgeUnsupportedProcessor; got: {err:?}"
        );
    }

    #[test]
    fn sanitize_drops_package_info_blocks() {
        let raw = "\
# this is a comment\n\
com.example.Foo -> a:\n\
    void bar() -> a\n\
com.example.package-info -> b:\n\
com.example.Baz -> c:\n\
    int n -> b\n\
";
        let out = sanitize_proguard_mappings(raw);
        assert!(out.contains("Foo -> a"));
        assert!(out.contains("Baz -> c"));
        assert!(!out.contains("package-info"));
        assert!(!out.contains("# this is a comment"));
    }

    #[test]
    fn sanitize_keeps_member_lines_after_kept_class() {
        let raw = "com.example.Foo -> a:\n    int n -> b\n    void m() -> c\n";
        let out = sanitize_proguard_mappings(raw);
        assert!(out.contains("int n -> b"));
        assert!(out.contains("void m() -> c"));
    }

    #[tokio::test]
    async fn download_mojmaps_rejects_unknown_side() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ProcessorContext {
            classpath: vec![],
            cache_dir: dir.path().to_path_buf(),
            java_bin: None,
        };
        let args = vec![
            "--task".into(),
            "DOWNLOAD_MOJMAPS".into(),
            "--version".into(),
            "1.20.4".into(),
            "--side".into(),
            "bogus".into(),
            "--output".into(),
            dir.path().join("out").display().to_string(),
        ];
        let err = run(None, args, &ctx).await.unwrap_err();
        match err {
            crate::error::Error::ForgePatcherFailed { processor, details } => {
                assert_eq!(processor, "installertools::DOWNLOAD_MOJMAPS");
                assert!(details.contains("bogus"));
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn require_mapping_sha1_accepts_present_hex() {
        let dl = serde_json::json!({
            "url": "https://example/mappings.txt",
            "sha1": "da39a3ee5e6b4b0d3255bfef95601890afd80709"
        });
        assert_eq!(
            require_mapping_sha1(&dl, "client_mappings").unwrap(),
            "da39a3ee5e6b4b0d3255bfef95601890afd80709"
        );
    }

    #[test]
    fn require_mapping_sha1_rejects_missing_empty_or_nonstring() {
        // B.6: a Mojang artifact with no sha1 must be a hard error, never a
        // silent verification skip.
        for dl in [
            serde_json::json!({ "url": "u" }),               // missing
            serde_json::json!({ "url": "u", "sha1": "" }),   // empty
            serde_json::json!({ "url": "u", "sha1": 123 }),  // non-string
            serde_json::json!({ "url": "u", "sha1": null }), // null
        ] {
            let err = require_mapping_sha1(&dl, "client_mappings").unwrap_err();
            match err {
                crate::error::Error::ForgePatcherFailed { processor, details } => {
                    assert_eq!(processor, "installertools::DOWNLOAD_MOJMAPS");
                    assert!(details.contains("sha1 missing"), "details: {details}");
                }
                other => panic!("got {other:?}"),
            }
        }
    }

    #[test]
    fn parse_tsrg_class_headers_picks_only_headers() {
        let body = "\
net/minecraft/A net/minecraft/srg/A\n\
\tfoo named_foo\n\
net/minecraft/B net/minecraft/srg/B\n\
# comment\n\
";
        let headers = parse_tsrg_class_headers(body);
        assert_eq!(
            headers,
            vec![
                (
                    "net/minecraft/A".to_string(),
                    "net/minecraft/srg/A".to_string()
                ),
                (
                    "net/minecraft/B".to_string(),
                    "net/minecraft/srg/B".to_string()
                ),
            ]
        );
    }

    #[test]
    fn parse_tsrg_class_headers_skips_tsrg2_header_line() {
        // 1.20.4 mcp_config ships TSRG v2 — its `tsrg2 obf srg id` declaration
        // line must not be emitted as a class pair (would corrupt MERGE_MAPPING
        // output and break downstream FART/srgutils parsing).
        let body = "\
tsrg2 obf srg id\n\
aab$a net/minecraft/foo/Bar 12345\n\
\tfoo named_foo bar_id\n\
zzc net/minecraft/baz/Qux 67890\n\
";
        let headers = parse_tsrg_class_headers(body);
        assert_eq!(
            headers,
            vec![
                ("aab$a".to_string(), "net/minecraft/foo/Bar".to_string()),
                ("zzc".to_string(), "net/minecraft/baz/Qux".to_string()),
            ]
        );
        // Most importantly: the literal "tsrg2" token must not appear as a class.
        assert!(headers.iter().all(|(o, _)| o != "tsrg2"));
    }

    #[test]
    fn parse_proguard_class_headers_reversed() {
        let body = "\
com.example.Foo -> a:\n\
    int n -> b\n\
com.example.Bar -> c:\n\
";
        let map = parse_proguard_class_headers(body, true);
        assert_eq!(map.get("a"), Some(&"com/example/Foo".to_string()));
        assert_eq!(map.get("c"), Some(&"com/example/Bar".to_string()));
    }

    #[tokio::test]
    async fn merge_mapping_v2_input_produces_left_right_header_and_strips_id() {
        // Java installertools MERGE_MAPPING always writes `tsrg2 left right` header
        // and strips the trailing `id` column from every line — producing exactly
        // 2 name slots (obf + merged-name). We must match that format exactly so
        // that downstream ART reads the merged mapping correctly.
        let dir = tempfile::tempdir().unwrap();
        let left = dir.path().join("left.tsrg");
        let right = dir.path().join("right.txt");
        let output = dir.path().join("merged.tsrg");
        std::fs::write(
            &left,
            "\
tsrg2 obf srg id\n\
a net/minecraft/srg/A 12345\n\
\tfoo srg_foo 67\n\
\tbar ()V srg_bar 89\n\
\t\t0 o paramName 99\n\
b net/minecraft/srg/B 6789\n\
",
        )
        .unwrap();
        std::fs::write(
            &right,
            "com.example.Named -> a:\n    int n -> q\nother.Skipped -> z:\n",
        )
        .unwrap();

        let ctx = ProcessorContext {
            classpath: vec![],
            cache_dir: dir.path().to_path_buf(),
            java_bin: None,
        };
        let args = vec![
            "--task".into(),
            "MERGE_MAPPING".into(),
            "--left".into(),
            left.display().to_string(),
            "--right".into(),
            right.display().to_string(),
            "--output".into(),
            output.display().to_string(),
            "--classes".into(),
            "--reverse-right".into(),
        ];
        run(None, args, &ctx).await.expect("merge");

        let merged = std::fs::read_to_string(&output).unwrap();
        // tsrg2 header is `tsrg2 left right` (not the original column names).
        assert!(
            merged.starts_with("tsrg2 left right\n"),
            "tsrg2 header wrong: {merged}"
        );
        // Class `a` present in right's reversed map → use named; id stripped.
        assert!(merged.contains("a com/example/Named\n"), "got: {merged}");
        // Class `b` absent from right → SRG fallback; id stripped.
        assert!(merged.contains("b net/minecraft/srg/B\n"), "got: {merged}");
        // Member lines: id stripped.
        assert!(
            merged.contains("\tfoo srg_foo\n"),
            "field line wrong: {merged}"
        );
        assert!(
            merged.contains("\tbar ()V srg_bar\n"),
            "method line wrong: {merged}"
        );
        // Param line: first 3 tokens kept, id stripped.
        assert!(
            merged.contains("\t\t0 o paramName\n"),
            "param line wrong: {merged}"
        );
    }

    #[tokio::test]
    async fn merge_mapping_tsrg_v1_input_passes_members_through() {
        // Phase 2 1.16.5 mcp_config is TSRG v1 (no header, 2 name slots). Verify
        // we don't accidentally add a tsrg2 header or corrupt v1 member lines.
        let dir = tempfile::tempdir().unwrap();
        let left = dir.path().join("left.tsrg");
        let right = dir.path().join("right.txt");
        let output = dir.path().join("merged.tsrg");
        std::fs::write(
            &left,
            "\
a net/minecraft/srg/A\n\
\tfoo srg_foo\n\
\tbar ()V srg_bar\n\
",
        )
        .unwrap();
        std::fs::write(&right, "").unwrap();

        let ctx = ProcessorContext {
            classpath: vec![],
            cache_dir: dir.path().to_path_buf(),
            java_bin: None,
        };
        let args = vec![
            "--task".into(),
            "MERGE_MAPPING".into(),
            "--left".into(),
            left.display().to_string(),
            "--right".into(),
            right.display().to_string(),
            "--output".into(),
            output.display().to_string(),
            "--classes".into(),
            "--reverse-right".into(),
        ];
        run(None, args, &ctx).await.expect("merge");

        let merged = std::fs::read_to_string(&output).unwrap();
        // Class line: srg fallback (right is empty), v1 shape (no id to strip).
        assert!(merged.contains("a net/minecraft/srg/A\n"), "got: {merged}");
        // Field + method lines pass through.
        assert!(
            merged.contains("\tfoo srg_foo\n"),
            "field line lost: {merged}"
        );
        assert!(
            merged.contains("\tbar ()V srg_bar\n"),
            "method line lost: {merged}"
        );
        // No tsrg2 header was synthesised for v1 input.
        assert!(
            !merged.contains("tsrg2"),
            "stale tsrg2 header leaked: {merged}"
        );
    }

    #[tokio::test]
    async fn merge_mapping_fields_methods_accepted() {
        // Regression guard: --fields --methods are supported (v0.4.1 NeoForge).
        // With no --classes, class names in left pass through unchanged.
        // id column is stripped; header becomes `tsrg2 left right`.
        let dir = tempfile::tempdir().unwrap();
        let ctx = ProcessorContext {
            classpath: vec![],
            cache_dir: dir.path().to_path_buf(),
            java_bin: None,
        };
        let left = dir.path().join("left.tsrg");
        let right = dir.path().join("right.txt");
        let output = dir.path().join("merged.tsrg");
        std::fs::write(
            &left,
            "\
tsrg2 obf srg id\n\
a net/minecraft/srg/A 12345\n\
\tb srg_b 67\n\
\tc ()V srg_c 89\n\
",
        )
        .unwrap();
        std::fs::write(&right, "com.example.Named -> a:\n    int mojField -> b\n    void mojMethod() -> c\nother.Skipped -> z:\n").unwrap();
        let args = vec![
            "--task".into(),
            "MERGE_MAPPING".into(),
            "--left".into(),
            left.display().to_string(),
            "--right".into(),
            right.display().to_string(),
            "--output".into(),
            output.display().to_string(),
            "--fields".into(),
            "--methods".into(),
            "--reverse-right".into(),
        ];
        run(None, args, &ctx)
            .await
            .expect("merge with fields+methods should succeed");
        let merged = std::fs::read_to_string(&output).unwrap();
        // Header is `tsrg2 left right`.
        assert!(merged.starts_with("tsrg2 left right\n"), "header: {merged}");
        // Without --classes, class names are left unchanged (srg name, id stripped).
        assert!(
            merged.contains("a net/minecraft/srg/A\n"),
            "class line: {merged}"
        );
        // With --fields, field name is replaced with Mojang name; id stripped.
        assert!(merged.contains("\tb mojField\n"), "field merge: {merged}");
        // With --methods, method name is replaced with Mojang name; id stripped.
        assert!(
            merged.contains("\tc ()V mojMethod\n"),
            "method merge: {merged}"
        );
    }

    #[tokio::test]
    async fn merge_mapping_accepts_installertools_3x_arg_naming() {
        // NeoForge 21.8+ ships installertools:3.0.13 which renamed:
        //   --left          → --merge
        //   --right         → --base
        //   --reverse-right → --reverse-base
        //   --classes/--fields/--methods dropped (3.x always merges all kinds)
        // Verify the parser accepts the new shape and produces an
        // implicit-all-merge output (classes + fields + methods all
        // resolved from the reversed base).
        let dir = tempfile::tempdir().unwrap();
        let ctx = ProcessorContext {
            classpath: vec![],
            cache_dir: dir.path().to_path_buf(),
            java_bin: None,
        };
        let left = dir.path().join("left.tsrg");
        let right = dir.path().join("right.txt");
        let output = dir.path().join("merged.tsrg");
        // Same inputs as `merge_mapping_fields_methods_accepted` — only
        // arg-name flavor differs.
        std::fs::write(
            &left,
            "\
tsrg2 obf srg id\n\
a net/minecraft/srg/A 12345\n\
\tb srg_b 67\n\
\tc ()V srg_c 89\n\
",
        )
        .unwrap();
        std::fs::write(&right, "com.example.Named -> a:\n    int mojField -> b\n    void mojMethod() -> c\nother.Skipped -> z:\n").unwrap();
        let args = vec![
            "--task".into(),
            "MERGE_MAPPING".into(),
            "--merge".into(),
            left.display().to_string(),
            "--base".into(),
            right.display().to_string(),
            "--output".into(),
            output.display().to_string(),
            "--reverse-base".into(),
        ];
        run(None, args, &ctx)
            .await
            .expect("merge with 3.x arg shape");

        let merged = std::fs::read_to_string(&output).unwrap();
        // Canonical tsrg2 header + id column stripped.
        assert!(merged.starts_with("tsrg2 left right\n"), "header: {merged}");
        // 3.x merges classes (implicit), so SRG name is replaced with Mojang form.
        assert!(
            merged.contains("a com/example/Named\n"),
            "class merge (3.x implicit): {merged}"
        );
        // 3.x merges fields + methods implicitly too.
        assert!(
            merged.contains("\tb mojField\n"),
            "field merge (3.x implicit): {merged}"
        );
        assert!(
            merged.contains("\tc ()V mojMethod\n"),
            "method merge (3.x implicit): {merged}"
        );
    }

    #[tokio::test]
    async fn merge_mapping_3x_missing_required_args_errors_cleanly() {
        // 3.x arg names with one missing should produce a clean error
        // mentioning both naming conventions so the diagnostic is
        // useful regardless of which version the caller used.
        let dir = tempfile::tempdir().unwrap();
        let ctx = ProcessorContext {
            classpath: vec![],
            cache_dir: dir.path().to_path_buf(),
            java_bin: None,
        };
        let only_merge = vec![
            "--task".into(),
            "MERGE_MAPPING".into(),
            "--merge".into(),
            "/tmp/left.tsrg".into(),
            "--output".into(),
            "/tmp/out.tsrg".into(),
        ];
        let err = run(None, only_merge, &ctx).await.unwrap_err();
        let s = format!("{err:?}");
        assert!(
            s.contains("--right") && s.contains("--base"),
            "error should mention both naming conventions: {s}"
        );
    }
}
