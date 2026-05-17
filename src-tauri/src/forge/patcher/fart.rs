//! ForgeAutoRenamingTool (FART) — bytecode remapper invoked as a Java
//! subprocess.
//!
//! Forge's modern-era (1.17+) install pipeline runs FART to remap the
//! vanilla MC client jar from obfuscated names to a merged SRG +
//! Mojang-named mapping. Forge's subsequent BinaryPatcher patches
//! reference byte offsets in this output, so the remapper output must
//! be byte-identical to what Java FART 1.0.6 produces.
//!
//! Pure-Rust byte-fidelity is impractical (same reasoning as
//! SpecialSource — see
//! `docs/superpowers/specs/2026-05-16-forge-loader-design.md#addendum-2026-05-17-b--specialsource-shell-out`),
//! so we shell out to the canonical Java implementation. The processor
//! JAR + classpath dependencies are passed to `java -cp ... <main>`.
//!
//! Main-Class per FART 1.0.6 jar manifest: `net.minecraftforge.fart.Main`.

use crate::error::Result;
use crate::forge::patcher::{patcher_fail, ProcessorContext};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Args {
    pub input: String,
    pub output: String,
    pub names: String,
    pub ann_fix: bool,
    pub ids_fix: bool,
    pub src_fix: bool,
    pub record_fix: bool,
    pub strip_sigs: bool,
}

pub fn parse_args(raw: &[String]) -> Result<Args> {
    let mut input = None;
    let mut output = None;
    let mut names = None;
    let mut ann_fix = false;
    let mut ids_fix = false;
    let mut src_fix = false;
    let mut record_fix = false;
    let mut strip_sigs = false;
    let mut it = raw.iter();
    while let Some(flag) = it.next() {
        match flag.as_str() {
            "--input" => input = it.next().cloned(),
            "--output" => output = it.next().cloned(),
            "--names" => names = it.next().cloned(),
            "--ann-fix" => ann_fix = true,
            "--ids-fix" => ids_fix = true,
            "--src-fix" => src_fix = true,
            "--record-fix" => record_fix = true,
            "--strip-sigs" => strip_sigs = true,
            // FART 1.0.6+ extra flags surfaced by newer Forge (60.x / 61.x on
            // MC 1.21.x). Accept as no-ops here — `run` dispatches to Java
            // FART which interprets them natively.
            //   --reverse: invert mapping direction.
            "--reverse" => { /* accepted, no-op for parser; passed through */ }
            other => return Err(patcher_fail("fart", &format!("unknown flag: {other}"))),
        }
    }
    Ok(Args {
        input: input.ok_or_else(|| patcher_fail("fart", &"missing --input"))?,
        output: output.ok_or_else(|| patcher_fail("fart", &"missing --output"))?,
        names: names.ok_or_else(|| patcher_fail("fart", &"missing --names"))?,
        ann_fix,
        ids_fix,
        src_fix,
        record_fix,
        strip_sigs,
    })
}

pub async fn run(args: Vec<String>, ctx: &ProcessorContext) -> Result<()> {
    // Validate args shape before spawning Java — catches typos early
    // with a clean error instead of a Java stacktrace.
    let _ = parse_args(&args)?;

    // Locate java binary. JRE must already be installed
    // (modern::install calls ensure_jre upstream, same as transitional).
    let java_bin = locate_java_binary(ctx);

    // Build classpath string. ctx.classpath includes the processor jar
    // (added by the caller) plus its declared dependencies. Separator
    // is platform-specific.
    let sep = if cfg!(windows) { ";" } else { ":" };
    let cp: String = ctx
        .classpath
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(sep);

    let mut cmd = tokio::process::Command::new(&java_bin);
    cmd.arg("-cp").arg(&cp);
    cmd.arg("net.minecraftforge.fart.Main");
    cmd.args(&args);

    eprintln!(
        "fart: spawning {} with {} classpath entries",
        java_bin.display(),
        ctx.classpath.len()
    );

    let output = cmd.output().await.map_err(|e| {
        patcher_fail("fart", &format!("spawn java: {e}"))
    })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(patcher_fail(
            "fart",
            &format!(
                "java exit {}: stderr={} stdout={}",
                output.status,
                stderr.trim(),
                stdout.trim()
            ),
        ));
    }
    Ok(())
}

fn locate_java_binary(ctx: &ProcessorContext) -> PathBuf {
    ctx.java_bin.clone().unwrap_or_else(|| PathBuf::from("java"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_args_minimal() {
        let raw = vec![
            "--input".into(), "/i".into(),
            "--output".into(), "/o".into(),
            "--names".into(), "/m".into(),
        ];
        let p = parse_args(&raw).unwrap();
        assert_eq!((p.input.as_str(), p.output.as_str(), p.names.as_str()), ("/i", "/o", "/m"));
        assert!(!p.ann_fix && !p.ids_fix && !p.src_fix && !p.record_fix && !p.strip_sigs);
    }

    #[test]
    fn parse_args_all_bool_flags() {
        let raw = vec![
            "--input".into(), "/i".into(),
            "--output".into(), "/o".into(),
            "--names".into(), "/m".into(),
            "--ann-fix".into(),
            "--ids-fix".into(),
            "--src-fix".into(),
            "--record-fix".into(),
            "--strip-sigs".into(),
        ];
        let p = parse_args(&raw).unwrap();
        assert!(p.ann_fix && p.ids_fix && p.src_fix && p.record_fix && p.strip_sigs);
    }

    #[test]
    fn parse_args_missing_required_errors() {
        assert!(parse_args(&vec!["--input".into(), "/i".into()]).is_err());
    }

    #[test]
    fn parse_args_unknown_flag_errors() {
        let raw = vec![
            "--input".into(), "/i".into(),
            "--output".into(), "/o".into(),
            "--names".into(), "/m".into(),
            "--mystery".into(),
        ];
        assert!(parse_args(&raw).is_err());
    }
}
