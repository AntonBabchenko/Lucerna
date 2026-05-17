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
    Err(crate::error::Error::ForgeUnsupportedProcessor {
        coord: "net.minecraftforge:installertools (task=BUNDLER_EXTRACT) — Phase 3".into(),
    })
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
    async fn bundler_extract_returns_phase3_stub() {
        let ctx = ProcessorContext { classpath: vec![], cache_dir: PathBuf::from("."), java_bin: None };
        let args = vec!["--task".into(), "BUNDLER_EXTRACT".into()];
        let err = run(None, args, &ctx).await.unwrap_err();
        match err {
            crate::error::Error::ForgeUnsupportedProcessor { coord } => {
                assert!(coord.contains("BUNDLER_EXTRACT") && coord.contains("Phase 3"));
            }
            other => panic!("got {other:?}"),
        }
    }
}
