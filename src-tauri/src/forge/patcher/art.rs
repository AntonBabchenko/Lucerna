//! AutoRenamingTool (ART) — bytecode remapper invoked as a Java
//! subprocess.
//!
//! NeoForge's modern-era (1.20.1+) install pipeline runs ART to remap
//! the slim vanilla MC jar (jarsplitter output) from obfuscated names
//! to a merged SRG + Mojang-named mapping. NeoForge's subsequent
//! BinaryPatcher patches reference byte offsets in this output, so the
//! remapper output must be byte-identical to what Java ART produces.
//!
//! ART is NeoForge's fork/successor of Forge's FART
//! (ForgeAutoRenamingTool). See
//! `docs/superpowers/specs/2026-05-16-forge-loader-design.md` for the
//! shell-out rationale.
//!
//! Main-Class per jar manifest:
//!   - ART 1.0.13 (`AutoRenamingTool-1.0.13-all.jar`): `net.minecraftforge.fart.Main`
//!     (same package as Forge FART — 1.x line was a fork that kept the package name)
//!   - ART 2.0.3 (`AutoRenamingTool-2.0.3-all.jar`): `net.neoforged.art.Main`
//!     (2.x line repackaged under the `net.neoforged` namespace)
//!
//! At runtime we detect the version from the classpath path string rather
//! than re-reading the manifest: if the classpath contains a path segment
//! `net/neoforged/AutoRenamingTool/2.` we use `net.neoforged.art.Main`;
//! otherwise we fall back to `net.minecraftforge.fart.Main` (1.x compat).

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
}

pub fn parse_args(raw: &[String]) -> Result<Args> {
    let mut input = None;
    let mut output = None;
    let mut names = None;
    let mut ann_fix = false;
    let mut ids_fix = false;
    let mut src_fix = false;
    let mut record_fix = false;
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
            // ART flags shared with FART but not present in NeoForge's
            // audit args. Accept as no-ops so future ART versions don't break.
            "--strip-sigs" => { /* accepted, no-op for parser; passed through */ }
            "--reverse" => { /* accepted, no-op for parser; passed through */ }
            other => return Err(patcher_fail("art", &format!("unknown flag: {other}"))),
        }
    }
    Ok(Args {
        input: input.ok_or_else(|| patcher_fail("art", &"missing --input"))?,
        output: output.ok_or_else(|| patcher_fail("art", &"missing --output"))?,
        names: names.ok_or_else(|| patcher_fail("art", &"missing --names"))?,
        ann_fix,
        ids_fix,
        src_fix,
        record_fix,
    })
}

pub async fn run(args: Vec<String>, ctx: &ProcessorContext) -> Result<()> {
    // Validate args shape before spawning Java.
    let _ = parse_args(&args)?;
    let java_bin = locate_java_binary(ctx);
    let main_class = select_main_class(ctx);
    crate::process::run_java_processor(
        &java_bin,
        &ctx.classpath,
        main_class,
        &args,
        "art",
    )
    .await
}

/// Select the ART Main-Class by inspecting the classpath for the JAR path.
/// ART 1.0.x → `net.minecraftforge.fart.Main` (1.x kept Forge's package name).
/// ART 2.0.x+ → `net.neoforged.art.Main` (2.x repackaged under neoforged).
///
/// Detection heuristic: if any classpath entry contains `AutoRenamingTool/2.`
/// in its path string, use the 2.x main class; otherwise fall back to 1.x.
fn select_main_class(ctx: &ProcessorContext) -> &'static str {
    let is_v2 = ctx.classpath.iter().any(|p| {
        let s = p.display().to_string();
        // Match version 2.x: path segment like AutoRenamingTool/2.0.3/...
        s.contains("AutoRenamingTool/2.") || s.contains("AutoRenamingTool\\2.")
    });
    if is_v2 {
        "net.neoforged.art.Main"
    } else {
        "net.minecraftforge.fart.Main"
    }
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
        assert!(!p.ann_fix && !p.ids_fix && !p.src_fix && !p.record_fix);
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
        ];
        let p = parse_args(&raw).unwrap();
        assert!(p.ann_fix && p.ids_fix && p.src_fix && p.record_fix);
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

    #[test]
    fn select_main_class_v1_uses_fart_package() {
        let ctx = ProcessorContext {
            classpath: vec![PathBuf::from(
                "libraries/net/neoforged/AutoRenamingTool/1.0.13/AutoRenamingTool-1.0.13-all.jar",
            )],
            cache_dir: PathBuf::from("."),
            java_bin: None,
        };
        assert_eq!(select_main_class(&ctx), "net.minecraftforge.fart.Main");
    }

    #[test]
    fn select_main_class_v2_uses_neoforged_package() {
        let ctx = ProcessorContext {
            classpath: vec![PathBuf::from(
                "libraries/net/neoforged/AutoRenamingTool/2.0.3/AutoRenamingTool-2.0.3-all.jar",
            )],
            cache_dir: PathBuf::from("."),
            java_bin: None,
        };
        assert_eq!(select_main_class(&ctx), "net.neoforged.art.Main");
    }

    #[test]
    fn select_main_class_no_art_in_classpath_falls_back_to_v1() {
        let ctx = ProcessorContext {
            classpath: vec![],
            cache_dir: PathBuf::from("."),
            java_bin: None,
        };
        assert_eq!(select_main_class(&ctx), "net.minecraftforge.fart.Main");
    }
}
