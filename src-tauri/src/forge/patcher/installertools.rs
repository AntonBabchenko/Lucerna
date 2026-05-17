//! installertools — Forge's multi-tool. First CLI arg is `--task <NAME>`.
//! Sub-commands implemented in Phase 2: EXTRACT_FILES (this task),
//! MCP_DATA (Task 7), BUNDLER_EXTRACT (Task 8, Phase 3 stub).

use crate::error::Result;
use crate::forge::patcher::{patcher_fail, ProcessorContext};
use std::path::PathBuf;

pub async fn run(_classifier: Option<&str>, args: Vec<String>, ctx: &ProcessorContext) -> Result<()> {
    let task = read_flag(&args, "--task").ok_or_else(|| {
        patcher_fail("installertools", &"missing --task <NAME>")
    })?;
    match task.as_str() {
        "EXTRACT_FILES" => extract_files(&args, ctx).await,
        "MCP_DATA" => mcp_data(&args, ctx).await,
        "BUNDLER_EXTRACT" => bundler_extract(&args, ctx).await,
        "DOWNLOAD_MOJMAPS" => download_mojmaps(&args, ctx).await,
        "MERGE_MAPPING" => merge_mapping(&args, ctx).await,
        other => Err(crate::error::Error::ForgeUnsupportedProcessor {
            coord: format!("net.minecraftforge:installertools (task={other})"),
        }),
    }
}

pub(super) fn read_flag(args: &[String], name: &str) -> Option<String> {
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == name { return it.next().cloned(); }
    }
    None
}

pub(super) fn read_all_flag(args: &[String], name: &str) -> Vec<String> {
    let mut out = vec![];
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == name {
            if let Some(v) = it.next() { out.push(v.clone()); }
        }
    }
    out
}

async fn extract_files(args: &[String], _ctx: &ProcessorContext) -> Result<()> {
    use std::io::Read;
    use tokio::fs;
    let archive = read_flag(args, "--archive").ok_or_else(|| {
        patcher_fail("installertools::EXTRACT_FILES", &"missing --archive")
    })?;
    let output = read_flag(args, "--output").ok_or_else(|| {
        patcher_fail("installertools::EXTRACT_FILES", &"missing --output")
    })?;
    let entries = read_all_flag(args, "--from");
    if entries.is_empty() {
        return Err(patcher_fail("installertools::EXTRACT_FILES", &"no --from entries"));
    }
    let bytes = fs::read(&archive).await.map_err(|e| {
        patcher_fail("installertools::EXTRACT_FILES", &format!("read {archive}: {e}"))
    })?;
    let mut zip_archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| {
        patcher_fail("installertools::EXTRACT_FILES", &format!("zip open {archive}: {e}"))
    })?;
    fs::create_dir_all(&output).await.map_err(|e| {
        patcher_fail("installertools::EXTRACT_FILES", &format!("mkdir {output}: {e}"))
    })?;
    for name in entries {
        let (buf, basename) = {
            let mut entry = zip_archive.by_name(&name).map_err(|_| {
                patcher_fail("installertools::EXTRACT_FILES", &format!("entry not found: {name}"))
            })?;
            let mut buf = Vec::with_capacity(entry.size() as usize);
            entry.read_to_end(&mut buf).map_err(|e| {
                patcher_fail("installertools::EXTRACT_FILES", &format!("read {name}: {e}"))
            })?;
            let basename = std::path::Path::new(&name).file_name().ok_or_else(|| {
                patcher_fail("installertools::EXTRACT_FILES", &format!("no basename: {name}"))
            })?.to_string_lossy().to_string();
            (buf, basename)
            // `entry` (ZipFile, not Send) dropped here before any await
        };
        let dest = PathBuf::from(&output).join(basename);
        fs::write(&dest, &buf).await.map_err(|e| {
            patcher_fail("installertools::EXTRACT_FILES", &format!("write {}: {e}", dest.display()))
        })?;
    }
    Ok(())
}

async fn mcp_data(args: &[String], _ctx: &ProcessorContext) -> Result<()> {
    use std::io::Read;
    use tokio::fs;
    let input = read_flag(args, "--input").ok_or_else(|| {
        patcher_fail("installertools::MCP_DATA", &"missing --input")
    })?;
    let output = read_flag(args, "--output").ok_or_else(|| {
        patcher_fail("installertools::MCP_DATA", &"missing --output")
    })?;
    let key = read_flag(args, "--key").unwrap_or_else(|| "mappings".into());

    let bytes = fs::read(&input).await.map_err(|e| {
        patcher_fail("installertools::MCP_DATA", &format!("read {input}: {e}"))
    })?;
    // Synchronous zip block — ZipFile<'_> is not Send, don't hold across .await.
    let (_, file_bytes) = {
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| {
            patcher_fail("installertools::MCP_DATA", &format!("zip open: {e}"))
        })?;

        // 1. Read config.json manifest.
        let manifest_text = {
            let mut e = archive.by_name("config.json").map_err(|_| {
                patcher_fail("installertools::MCP_DATA", &format!("no config.json in {input}"))
            })?;
            let mut s = String::with_capacity(e.size() as usize);
            e.read_to_string(&mut s).map_err(|err| {
                patcher_fail("installertools::MCP_DATA", &format!("read config.json: {err}"))
            })?;
            s
        };
        let manifest: serde_json::Value = serde_json::from_str(&manifest_text).map_err(|e| {
            patcher_fail("installertools::MCP_DATA", &format!("parse config.json: {e}"))
        })?;

        // 2. Look up data.<key> → relative path string.
        let rel = manifest
            .get("data")
            .and_then(|d| d.get(&key))
            .and_then(|v| v.as_str())
            .ok_or_else(|| patcher_fail(
                "installertools::MCP_DATA",
                &format!("config.json missing data.{key} (or not a string)"),
            ))?
            .to_string();
        if rel.ends_with('/') {
            return Err(patcher_fail(
                "installertools::MCP_DATA",
                &format!("data.{key} points to a directory ({rel}) — Phase 2 doesn't support directory extraction"),
            ));
        }

        // 3. Extract that one file.
        let mut entry = archive.by_name(&rel).map_err(|_| {
            patcher_fail("installertools::MCP_DATA", &format!("entry {rel} (from data.{key}) not in zip"))
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
            patcher_fail("installertools::MCP_DATA", &format!("mkdir {}: {e}", parent.display()))
        })?;
    }
    fs::write(&out_path, &file_bytes).await.map_err(|e| {
        patcher_fail("installertools::MCP_DATA", &format!("write {}: {e}", out_path.display()))
    })?;
    Ok(())
}

async fn bundler_extract(_args: &[String], _ctx: &ProcessorContext) -> Result<()> {
    // BUNDLER_EXTRACT unpacks Minecraft's bundled server jar (1.18+) into
    // its embedded libraries + plain server jar. FTlauncher is client-only,
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

async fn download_mojmaps(args: &[String], _ctx: &ProcessorContext) -> Result<()> {
    use tokio::fs;
    let version = read_flag(args, "--version").ok_or_else(|| {
        patcher_fail("installertools::DOWNLOAD_MOJMAPS", &"missing --version")
    })?;
    let side = read_flag(args, "--side").ok_or_else(|| {
        patcher_fail("installertools::DOWNLOAD_MOJMAPS", &"missing --side")
    })?;
    let output = read_flag(args, "--output").ok_or_else(|| {
        patcher_fail("installertools::DOWNLOAD_MOJMAPS", &"missing --output")
    })?;
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
    let url = dl
        .get("url")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            patcher_fail(
                "installertools::DOWNLOAD_MOJMAPS",
                &format!("downloads.{key}.url missing or not a string"),
            )
        })?;
    let sha1 = dl
        .get("sha1")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    // 3. Download.
    if let Some(parent) = std::path::Path::new(&output).parent() {
        fs::create_dir_all(parent).await.map_err(|e| {
            patcher_fail(
                "installertools::DOWNLOAD_MOJMAPS",
                &format!("mkdir {}: {e}", parent.display()),
            )
        })?;
    }
    crate::network::download::download_no_emit(url, std::path::Path::new(&output), sha1, "forge-mojmaps")
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
    use std::collections::HashMap;
    use tokio::fs;
    let left = read_flag(args, "--left").ok_or_else(|| {
        patcher_fail("installertools::MERGE_MAPPING", &"missing --left")
    })?;
    let right = read_flag(args, "--right").ok_or_else(|| {
        patcher_fail("installertools::MERGE_MAPPING", &"missing --right")
    })?;
    let output = read_flag(args, "--output").ok_or_else(|| {
        patcher_fail("installertools::MERGE_MAPPING", &"missing --output")
    })?;
    let classes_only = args.iter().any(|a| a == "--classes");
    let reverse_right = args.iter().any(|a| a == "--reverse-right");

    if !classes_only {
        return Err(patcher_fail(
            "installertools::MERGE_MAPPING",
            &"member-level merge not supported (Phase 3 covers --classes only — 1.20.4 only invokes the class-only mode)",
        ));
    }

    let left_text = fs::read_to_string(&left).await.map_err(|e| {
        patcher_fail("installertools::MERGE_MAPPING", &format!("read {left}: {e}"))
    })?;
    let right_text = fs::read_to_string(&right).await.map_err(|e| {
        patcher_fail("installertools::MERGE_MAPPING", &format!("read {right}: {e}"))
    })?;

    // Parse right (Proguard) as obf→named class map.
    let right_map: HashMap<String, String> =
        parse_proguard_class_headers(&right_text, reverse_right);

    // Detect TSRG version of left input. v2 opens with `tsrg2 <name1> ...`;
    // v1 has no header. We preserve the input format so that downstream
    // srgutils (FART's mapping loader) chooses its v2 code path when the
    // input was v2 — emitting v1 confuses it (and produced wrong FART output
    // empirically during e2e).
    let header_line: Option<&str> = left_text
        .lines()
        .find(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'));
    let is_v2 = header_line.is_some_and(|l| l.split_whitespace().next() == Some("tsrg2"));

    // Walk left line-by-line. `--classes` semantic per srgutils 0.5.x:
    //   - tsrg2 header: preserved verbatim (declares column count).
    //   - Class-header lines: replace the 2nd column (srg name) with right's
    //     named lookup; keep all other columns (e.g. v2's id column).
    //   - Member lines (start with tab): pass through UNCHANGED — `--classes`
    //     means right contributes only class-level info; member mappings
    //     carry over from left as-is. Required so FART rewrites field/method
    //     names internally — without them, the binarypatcher patches reference
    //     a different (correctly-renamed) byte layout that we'd never produce.
    //   - Empty/comment lines: dropped.
    let mut out = String::with_capacity(left_text.len());
    for line in left_text.lines() {
        if line.is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        if line.starts_with('\t') || line.starts_with(' ') {
            // Member line (TSRG v1 or v2) — verbatim.
            out.push_str(line);
            out.push('\n');
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }
        if parts[0] == "tsrg2" {
            // v2 header — preserve so srgutils uses the v2 code path.
            out.push_str(line);
            out.push('\n');
            continue;
        }
        if parts.len() < 2 {
            // Malformed class line; skip.
            continue;
        }
        let obf = parts[0];
        let srg = parts[1];
        let named = right_map.get(obf).map(String::as_str).unwrap_or(srg);
        out.push_str(obf);
        out.push(' ');
        out.push_str(named);
        // For TSRG v2, preserve any trailing columns (e.g. the `id` slot).
        if is_v2 {
            for extra in &parts[2..] {
                out.push(' ');
                out.push_str(extra);
            }
        }
        out.push('\n');
    }

    if let Some(parent) = std::path::Path::new(&output).parent() {
        fs::create_dir_all(parent).await.map_err(|e| {
            patcher_fail("installertools::MERGE_MAPPING", &format!("mkdir {}: {e}", parent.display()))
        })?;
    }
    fs::write(&output, &out).await.map_err(|e| {
        patcher_fail("installertools::MERGE_MAPPING", &format!("write {output}: {e}"))
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
        if line.is_empty() || line.starts_with('\t') || line.starts_with(' ') || line.starts_with('#') {
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
pub(crate) fn parse_proguard_class_headers(body: &str, reverse: bool) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    for line in body.lines() {
        // Class header is a left-aligned line ending with " -> X:" where
        // member lines start with whitespace.
        if line.starts_with(' ') || line.starts_with('\t') || line.starts_with('#') || line.is_empty() {
            continue;
        }
        let Some(arrow) = line.find(" -> ") else { continue };
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn extract_files_unknown_task_errors() {
        let ctx = ProcessorContext { classpath: vec![], cache_dir: PathBuf::from("."), java_bin: None };
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

        let ctx = ProcessorContext { classpath: vec![], cache_dir: dir.path().to_path_buf(), java_bin: None };
        let args = vec![
            "--task".into(), "EXTRACT_FILES".into(),
            "--archive".into(), zip_path.display().to_string(),
            "--from".into(), "data/hello.txt".into(),
            "--output".into(), out_dir.display().to_string(),
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
        w.write_all(br#"{"data":{"mappings":"config/joined.tsrg"}}"#).unwrap();
        w.start_file::<_, ()>("config/joined.tsrg", opts).unwrap();
        w.write_all(b"net/x/Y net/x/Y\n").unwrap();
        w.start_file::<_, ()>("other/skip.txt", opts).unwrap();
        w.write_all(b"ignored").unwrap();
        w.finish().unwrap();

        let ctx = ProcessorContext { classpath: vec![], cache_dir: dir.path().to_path_buf(), java_bin: None };
        let args = vec![
            "--task".into(), "MCP_DATA".into(),
            "--input".into(), zip_path.display().to_string(),
            "--output".into(), out_path.display().to_string(),
            "--key".into(), "mappings".into(),
        ];
        run(None, args, &ctx).await.expect("mcp_data");
        let got = std::fs::read_to_string(&out_path).unwrap();
        assert!(got.contains("net/x/Y"), "output should be the extracted joined.tsrg");
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
        w.write_all(br#"{"data":{"mappings":"../escape.txt"}}"#).unwrap();
        w.finish().unwrap();

        let ctx = ProcessorContext { classpath: vec![], cache_dir: dir.path().to_path_buf(), java_bin: None };
        let args = vec![
            "--task".into(), "MCP_DATA".into(),
            "--input".into(), zip_path.display().to_string(),
            "--output".into(), out_path.display().to_string(),
            "--key".into(), "mappings".into(),
        ];
        let err = run(None, args, &ctx).await.unwrap_err();
        match err {
            crate::error::Error::ForgePatcherFailed { processor, .. } => {
                assert_eq!(processor, "installertools::MCP_DATA");
            }
            other => panic!("expected ForgePatcherFailed, got {other:?}"),
        }
        assert!(!dir.path().join("escape.txt").exists(), "escape file must not exist");
    }

    #[tokio::test]
    async fn bundler_extract_is_server_only_stub() {
        let ctx = ProcessorContext { classpath: vec![], cache_dir: PathBuf::from("."), java_bin: None };
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
        let ctx = ProcessorContext { classpath: vec![], cache_dir: dir.path().to_path_buf(), java_bin: None };
        let args = vec![
            "--task".into(), "DOWNLOAD_MOJMAPS".into(),
            "--version".into(), "1.20.4".into(),
            "--side".into(), "bogus".into(),
            "--output".into(), dir.path().join("out").display().to_string(),
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
    fn parse_tsrg_class_headers_picks_only_headers() {
        let body = "\
net/minecraft/A net/minecraft/srg/A\n\
\tfoo named_foo\n\
net/minecraft/B net/minecraft/srg/B\n\
# comment\n\
";
        let headers = parse_tsrg_class_headers(body);
        assert_eq!(headers, vec![
            ("net/minecraft/A".to_string(), "net/minecraft/srg/A".to_string()),
            ("net/minecraft/B".to_string(), "net/minecraft/srg/B".to_string()),
        ]);
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
        assert_eq!(headers, vec![
            ("aab$a".to_string(), "net/minecraft/foo/Bar".to_string()),
            ("zzc".to_string(), "net/minecraft/baz/Qux".to_string()),
        ]);
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
    async fn merge_mapping_v2_input_preserves_header_and_columns() {
        // 1.20.4 mcp_config shape: tsrg2 header with `obf srg id` slots.
        // Verify our merged output keeps the v2 header so srgutils picks
        // its v2 code path on the FART side.
        let dir = tempfile::tempdir().unwrap();
        let left = dir.path().join("left.tsrg");
        let right = dir.path().join("right.txt");
        let output = dir.path().join("merged.tsrg");
        std::fs::write(&left, "\
tsrg2 obf srg id\n\
a net/minecraft/srg/A 12345\n\
\tfoo srg_foo 67\n\
\tbar ()V srg_bar 89\n\
\t\t0 paramName 99\n\
b net/minecraft/srg/B 6789\n\
").unwrap();
        std::fs::write(&right, "com.example.Named -> a:\n    int n -> q\nother.Skipped -> z:\n").unwrap();

        let ctx = ProcessorContext { classpath: vec![], cache_dir: dir.path().to_path_buf(), java_bin: None };
        let args = vec![
            "--task".into(), "MERGE_MAPPING".into(),
            "--left".into(), left.display().to_string(),
            "--right".into(), right.display().to_string(),
            "--output".into(), output.display().to_string(),
            "--classes".into(),
            "--reverse-right".into(),
        ];
        run(None, args, &ctx).await.expect("merge");

        let merged = std::fs::read_to_string(&output).unwrap();
        // tsrg2 header is preserved.
        assert!(merged.starts_with("tsrg2 obf srg id\n"), "tsrg2 header missing: {merged}");
        // Class `a` present in right's reversed map → use named, id preserved.
        assert!(merged.contains("a com/example/Named 12345\n"), "got: {merged}");
        // Class `b` absent from right → SRG fallback, id preserved.
        assert!(merged.contains("b net/minecraft/srg/B 6789\n"), "got: {merged}");
        // Member lines preserved verbatim (with id column intact for v2).
        assert!(merged.contains("\tfoo srg_foo 67\n"), "field line lost: {merged}");
        assert!(merged.contains("\tbar ()V srg_bar 89\n"), "method line lost: {merged}");
        // Param line preserved (`\t\t...`) — FART may read it.
        assert!(merged.contains("\t\t0 paramName 99\n"), "param line lost: {merged}");
    }

    #[tokio::test]
    async fn merge_mapping_tsrg_v1_input_passes_members_through() {
        // Phase 2 1.16.5 mcp_config is TSRG v1 (no header, 2 name slots). Verify
        // we don't accidentally add a tsrg2 header or strip columns.
        let dir = tempfile::tempdir().unwrap();
        let left = dir.path().join("left.tsrg");
        let right = dir.path().join("right.txt");
        let output = dir.path().join("merged.tsrg");
        std::fs::write(&left, "\
a net/minecraft/srg/A\n\
\tfoo srg_foo\n\
\tbar ()V srg_bar\n\
").unwrap();
        std::fs::write(&right, "").unwrap();

        let ctx = ProcessorContext { classpath: vec![], cache_dir: dir.path().to_path_buf(), java_bin: None };
        let args = vec![
            "--task".into(), "MERGE_MAPPING".into(),
            "--left".into(), left.display().to_string(),
            "--right".into(), right.display().to_string(),
            "--output".into(), output.display().to_string(),
            "--classes".into(),
            "--reverse-right".into(),
        ];
        run(None, args, &ctx).await.expect("merge");

        let merged = std::fs::read_to_string(&output).unwrap();
        // Class line: srg fallback (right is empty), v1 shape.
        assert!(merged.contains("a net/minecraft/srg/A\n"), "got: {merged}");
        // Field + method lines verbatim.
        assert!(merged.contains("\tfoo srg_foo\n"), "field line lost: {merged}");
        assert!(merged.contains("\tbar ()V srg_bar\n"), "method line lost: {merged}");
        // No tsrg2 header was synthesised.
        assert!(!merged.contains("tsrg2"), "stale tsrg2 header leaked: {merged}");
    }

    #[tokio::test]
    async fn merge_mapping_member_merge_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ProcessorContext { classpath: vec![], cache_dir: dir.path().to_path_buf(), java_bin: None };
        let args = vec![
            "--task".into(), "MERGE_MAPPING".into(),
            "--left".into(), "/nonexistent/left".into(),
            "--right".into(), "/nonexistent/right".into(),
            "--output".into(), dir.path().join("o.tsrg").display().to_string(),
            // No --classes — member merge requested, not supported in Phase 3.
            "--reverse-right".into(),
        ];
        let err = run(None, args, &ctx).await.unwrap_err();
        match err {
            crate::error::Error::ForgePatcherFailed { processor, details } => {
                assert_eq!(processor, "installertools::MERGE_MAPPING");
                assert!(details.contains("--classes only"));
            }
            other => panic!("got {other:?}"),
        }
    }
}
