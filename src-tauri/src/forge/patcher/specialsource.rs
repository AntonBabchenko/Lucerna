//! SpecialSource — bytecode remapper invoked as a Java subprocess.
//!
//! Forge's transitional-era (1.13-1.16) install pipeline runs
//! SpecialSource to remap the slim vanilla MC jar from obfuscated
//! names to SRG names. Forge's subsequent BinaryPatcher (GDiff)
//! patches reference byte offsets in this SRG output, so the
//! remapper output must be byte-identical to what Java SpecialSource
//! 1.8.5/1.11.0 produces.
//!
//! Pure-Rust byte-fidelity proved impractical, so we shell out to the
//! canonical Java implementation. The processor
//! JAR + classpath dependencies are passed to `java -cp ... <main>`.

use crate::error::Result;
use crate::forge::patcher::{patcher_fail, ProcessorContext};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Args {
    pub in_jar: String,
    pub out_jar: String,
    pub srg_in: String,
    pub kill_lvt: bool,
}

pub fn parse_args(raw: &[String]) -> Result<Args> {
    let mut in_jar = None;
    let mut out_jar = None;
    let mut srg_in = None;
    let mut kill_lvt = false;
    let mut live = false;
    let mut it = raw.iter();
    while let Some(flag) = it.next() {
        match flag.as_str() {
            "--in-jar" => in_jar = it.next().cloned(),
            "--out-jar" => out_jar = it.next().cloned(),
            "--srg-in" => srg_in = it.next().cloned(),
            "--kill-lvt" => kill_lvt = true,
            "--live" => live = true,
            "--quiet" => { /* swallow */ }
            other => {
                return Err(patcher_fail(
                    "specialsource",
                    &format!("unknown flag: {other}"),
                ))
            }
        }
    }
    if live {
        return Err(patcher_fail("specialsource", &"--live not supported"));
    }
    Ok(Args {
        in_jar: in_jar.ok_or_else(|| patcher_fail("specialsource", &"missing --in-jar"))?,
        out_jar: out_jar.ok_or_else(|| patcher_fail("specialsource", &"missing --out-jar"))?,
        srg_in: srg_in.ok_or_else(|| patcher_fail("specialsource", &"missing --srg-in"))?,
        kill_lvt,
    })
}

pub async fn run(args: Vec<String>, ctx: &ProcessorContext) -> Result<()> {
    // Validate args shape before spawning Java — a clean error beats a
    // Java stacktrace.
    let _ = parse_args(&args)?;
    let java_bin = locate_java_binary(ctx);
    crate::process::run_java_processor(
        &java_bin,
        &ctx.classpath,
        "net.md_5.specialsource.SpecialSource",
        &args,
        "specialsource",
    )
    .await
}

/// Return the java binary path to use for subprocess invocation.
/// Prefers `ctx.java_bin` (set by `transitional::install` after `ensure_jre`).
/// Falls back to `"java"` on PATH when not set — works on developer machines
/// and environments with a system JRE.
fn locate_java_binary(ctx: &ProcessorContext) -> PathBuf {
    ctx.java_bin
        .clone()
        .unwrap_or_else(|| PathBuf::from("java"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_args_minimal() {
        let raw = vec![
            "--in-jar".into(),
            "/i".into(),
            "--out-jar".into(),
            "/o".into(),
            "--srg-in".into(),
            "/m".into(),
        ];
        let p = parse_args(&raw).unwrap();
        assert_eq!(
            (p.in_jar.as_str(), p.out_jar.as_str(), p.srg_in.as_str()),
            ("/i", "/o", "/m")
        );
        assert!(!p.kill_lvt);
    }

    #[test]
    fn parse_args_kill_lvt_flag() {
        let raw = vec![
            "--in-jar".into(),
            "/i".into(),
            "--out-jar".into(),
            "/o".into(),
            "--srg-in".into(),
            "/m".into(),
            "--kill-lvt".into(),
        ];
        assert!(parse_args(&raw).unwrap().kill_lvt);
    }

    #[test]
    fn parse_args_rejects_live() {
        let raw = vec![
            "--in-jar".into(),
            "/i".into(),
            "--out-jar".into(),
            "/o".into(),
            "--srg-in".into(),
            "/m".into(),
            "--live".into(),
        ];
        assert!(parse_args(&raw).is_err());
    }

    #[test]
    fn parse_args_missing_required_errors() {
        assert!(parse_args(&vec!["--in-jar".into(), "/i".into()]).is_err());
    }
}
