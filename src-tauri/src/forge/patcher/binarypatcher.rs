//! BinaryPatcher — applies Forge's per-class GDiff v4 patches to an
//! SRG-deobfuscated vanilla jar, overlaying Forge's modifications.
//!
//! Pipeline:
//!   1. Decompress `--apply <patches.lzma>` via xz2 → patch container.
//!   2. Patch container is itself a JAR with one `.binpatch` entry per
//!      patched class (path: `<class>.binpatch`).
//!   3. Each .binpatch is a header (version, source/target class names,
//!      optional Adler-32 checksum) + GDiff v4 payload.
//!   4. For each patched class: read original from `--clean` jar,
//!      apply patch via `apply_gdiff`, write to output jar.
//!   5. Unpatched entries pass through unchanged.
//!
//! Note: the plan originally specified BSDiff; reverse-engineering the
//! real format against the 1.16.5-36.2.42 installer fixture and Forge's
//! `BinaryPatcher/Patcher.java` source confirmed GDiff v4 instead.

use crate::error::Result;
use crate::forge::patcher::{patcher_fail, ProcessorContext};

#[derive(Debug, Clone)]
pub struct Args {
    pub clean: String,
    pub output: String,
    pub apply: String,
}

pub fn parse_args(raw: &[String]) -> Result<Args> {
    let mut clean = None;
    let mut output = None;
    let mut apply = None;
    let mut it = raw.iter();
    while let Some(flag) = it.next() {
        match flag.as_str() {
            "--clean" => clean = it.next().cloned(),
            "--output" => output = it.next().cloned(),
            "--apply" => apply = it.next().cloned(),
            // binarypatcher 1.2.0+ bool flags. The presence of either of
            // these triggers the Java shell-out route in `run` (1.0.12's
            // pure-Rust impl can't reproduce 1.2.0+ semantics). For 1.0.12
            // they're absent, and the Rust path runs as before.
            //   --unpatched: include unpatched classes from --clean.
            //   --data:      include non-class data entries.
            //   --store:     binarypatcher 1.3.0+ (Forge 26.1.x / MC 26.1.x);
            //                changes how patches are persisted.
            "--unpatched" => { /* accepted, no-op for our parser; passed through to Java */ }
            "--data" => { /* accepted, no-op for our parser; passed through to Java */ }
            "--store" => { /* accepted, no-op for our parser; passed through to Java */ }
            // binarypatcher 1.3.0+ value-bearing flag.
            //   --marker <path>: writes a sentinel file after patching;
            //                    used by Forge bootstrap to skip already-
            //                    patched jars on relaunch.
            "--marker" => { let _ = it.next(); /* swallow value; passed through to Java */ }
            other => return Err(patcher_fail("binarypatcher", &format!("unknown flag: {other}"))),
        }
    }
    Ok(Args {
        clean: clean.ok_or_else(|| patcher_fail("binarypatcher", &"missing --clean"))?,
        output: output.ok_or_else(|| patcher_fail("binarypatcher", &"missing --output"))?,
        apply: apply.ok_or_else(|| patcher_fail("binarypatcher", &"missing --apply"))?,
    })
}

/// Decompress raw LZMA1 bytes (no .xz framing) — the format Forge
/// uses for `data/client.lzma`. Output is a ZIP/JAR with .binpatch
/// entries inside.
pub fn decompress_lzma(input: &[u8]) -> Result<Vec<u8>> {
    use std::io::Read;
    let stream = xz2::stream::Stream::new_lzma_decoder(u64::MAX).map_err(|e| {
        patcher_fail("binarypatcher", &format!("lzma stream init: {e}"))
    })?;
    let mut reader = xz2::read::XzDecoder::new_stream(input, stream);
    let mut out = Vec::with_capacity(input.len() * 4);
    reader.read_to_end(&mut out).map_err(|e| {
        patcher_fail("binarypatcher", &format!("lzma decompress: {e}"))
    })?;
    Ok(out)
}

// ─── BinPatch format ─────────────────────────────────────────────────────────
//
// Each entry inside the decompressed patch container (a ZIP whose entries are
// `<class>.binpatch`) follows the format written by Forge's BinaryPatcher
// (net.minecraftforge.binarypatcher) and applied by `com.nothome.delta.GDiffPatcher`:
//
//   u8            version       must be 1
//   java-utf8     obf           obfuscated / source class name (/-separated)
//   java-utf8     srg           SRG-remapped class name
//   u8            exists        0 = class not in vanilla; 1 = class exists
//   if exists:
//     u32 BE      checksum      Adler-32 of original class bytes
//   u32 BE        payload_len   byte length of the GDiff patch payload
//   [payload_len] payload       GDiff v4 patch payload
//                               (magic 0xd1ffd1ff 0x04 + commands)
//                               empty payload + exists=1 → class deleted
//
// "java-utf8" is Java DataInputStream.readUTF(): 2-byte BE unsigned short
// giving the byte-count, followed by that many modified-UTF-8 bytes.

/// A parsed `.binpatch` entry extracted from the patch container ZIP.
#[derive(Debug, Clone)]
pub struct BinPatch {
    /// Obfuscated (or source) class name, /-separated (e.g. `net/minecraft/…`).
    pub source_class: String,
    /// SRG-remapped class name.
    pub target_class: String,
    /// Adler-32 checksum of the original class, `None` if the class is absent
    /// from the vanilla jar (exists byte == 0).
    pub source_checksum: Option<u32>,
    /// Raw GDiff v4 payload bytes.  Empty slice means "delete this class".
    pub patch_payload: Vec<u8>,
}

/// Read a Java `DataInputStream.readUTF()` string from `data` at `pos`.
/// Returns `(string, new_pos)`.
fn read_java_utf(data: &[u8], pos: usize) -> crate::error::Result<(String, usize)> {
    if pos + 2 > data.len() {
        return Err(patcher_fail("binarypatcher", &"unexpected EOF reading java-utf length"));
    }
    let len = u16::from_be_bytes([data[pos], data[pos + 1]]) as usize;
    let end = pos + 2 + len;
    if end > data.len() {
        return Err(patcher_fail("binarypatcher", &"unexpected EOF reading java-utf string"));
    }
    let s = std::str::from_utf8(&data[pos + 2..end])
        .map_err(|e| patcher_fail("binarypatcher", &format!("java-utf8 decode: {e}")))?
        .to_owned();
    Ok((s, end))
}

/// Parse a raw `.binpatch` entry (the bytes of one entry inside the container ZIP).
pub fn parse_binpatch(input: &[u8]) -> crate::error::Result<BinPatch> {
    if input.is_empty() {
        return Err(patcher_fail("binarypatcher", &"empty binpatch input"));
    }
    let mut pos = 0usize;

    // version byte must be 1
    let version = input[pos];
    pos += 1;
    if version != 1 {
        return Err(patcher_fail(
            "binarypatcher",
            &format!("unsupported binpatch version: {version}"),
        ));
    }

    // obf name + srg name
    let (source_class, p) = read_java_utf(input, pos)?;
    pos = p;
    let (target_class, p) = read_java_utf(input, pos)?;
    pos = p;

    // exists byte
    if pos >= input.len() {
        return Err(patcher_fail("binarypatcher", &"unexpected EOF at exists byte"));
    }
    let exists = input[pos] != 0;
    pos += 1;

    // optional checksum
    let source_checksum = if exists {
        if pos + 4 > input.len() {
            return Err(patcher_fail("binarypatcher", &"unexpected EOF reading checksum"));
        }
        let cksum = u32::from_be_bytes([input[pos], input[pos+1], input[pos+2], input[pos+3]]);
        pos += 4;
        Some(cksum)
    } else {
        None
    };

    // payload length + payload bytes
    if pos + 4 > input.len() {
        return Err(patcher_fail("binarypatcher", &"unexpected EOF reading payload length"));
    }
    let payload_len = u32::from_be_bytes([input[pos], input[pos+1], input[pos+2], input[pos+3]]) as usize;
    pos += 4;
    if pos + payload_len > input.len() {
        return Err(patcher_fail(
            "binarypatcher",
            &format!("payload length {payload_len} exceeds available bytes {}", input.len() - pos),
        ));
    }
    let patch_payload = input[pos..pos + payload_len].to_vec();

    Ok(BinPatch { source_class, target_class, source_checksum, patch_payload })
}

// ─── GDiff v4 applier ─────────────────────────────────────────────────────────
//
// GDiff v4 format (com.nothome.delta.GDiffWriter constants):
//
//   Magic: 0xd1 0xff 0xd1 0xff 0x04  (5 bytes)
//   Commands:
//     0          EOF — stop processing
//     1..=246    DATA(N) — copy next N bytes from patch stream as literals
//     247        DATA_USHORT — u16 BE length, then that many literal bytes
//     248        DATA_INT   — u32 BE length, then that many literal bytes
//     249        COPY_USHORT_UBYTE  — u16 BE offset, u8 length; copy from source
//     250        COPY_USHORT_USHORT — u16 BE offset, u16 BE length
//     251        COPY_USHORT_INT    — u16 BE offset, u32 BE length
//     252        COPY_INT_UBYTE     — u32 BE offset, u8 length
//     253        COPY_INT_USHORT    — u32 BE offset, u16 BE length
//     254        COPY_INT_INT       — u32 BE offset, u32 BE length
//     255        COPY_LONG_INT      — u64 BE offset, u32 BE length

const GDIFF_DATA_MAX: u8 = 246;
const GDIFF_DATA_USHORT: u8 = 247;
const GDIFF_DATA_INT: u8 = 248;
const GDIFF_COPY_USHORT_UBYTE: u8 = 249;
const GDIFF_COPY_USHORT_USHORT: u8 = 250;
const GDIFF_COPY_USHORT_INT: u8 = 251;
const GDIFF_COPY_INT_UBYTE: u8 = 252;
const GDIFF_COPY_INT_USHORT: u8 = 253;
const GDIFF_COPY_INT_INT: u8 = 254;
const GDIFF_COPY_LONG_INT: u8 = 255;

/// Apply a GDiff v4 patch payload to `original`, producing patched bytes.
///
/// `patch` must begin with the 5-byte GDiff magic (`d1 ff d1 ff 04`).
/// An empty `patch` slice is valid and returns an empty Vec (class deleted).
pub fn apply_gdiff(original: &[u8], patch: &[u8]) -> crate::error::Result<Vec<u8>> {
    if patch.is_empty() {
        // Empty payload = class deleted; produce no bytes.
        return Ok(Vec::new());
    }
    if patch.len() < 5 || &patch[..4] != &[0xd1, 0xff, 0xd1, 0xff] || patch[4] != 4 {
        return Err(patcher_fail(
            "binarypatcher",
            &format!("invalid GDiff magic/version: {:?}", &patch[..patch.len().min(5)]),
        ));
    }

    let mut out = Vec::with_capacity(original.len());
    let mut pos = 5usize; // skip 5-byte magic+version

    macro_rules! need {
        ($n:expr) => {{
            if pos + $n > patch.len() {
                return Err(patcher_fail("binarypatcher", &"unexpected EOF in GDiff patch"));
            }
        }};
    }
    macro_rules! read_u8 {
        () => {{
            need!(1);
            let v = patch[pos];
            pos += 1;
            v
        }};
    }
    macro_rules! read_u16_be {
        () => {{
            need!(2);
            let v = u16::from_be_bytes([patch[pos], patch[pos + 1]]) as usize;
            pos += 2;
            v
        }};
    }
    macro_rules! read_u32_be {
        () => {{
            need!(4);
            let v = u32::from_be_bytes([patch[pos], patch[pos+1], patch[pos+2], patch[pos+3]]) as usize;
            pos += 4;
            v
        }};
    }
    macro_rules! read_u64_be {
        () => {{
            need!(8);
            let v = u64::from_be_bytes([
                patch[pos], patch[pos+1], patch[pos+2], patch[pos+3],
                patch[pos+4], patch[pos+5], patch[pos+6], patch[pos+7],
            ]) as usize;
            pos += 8;
            v
        }};
    }

    loop {
        need!(1);
        let cmd = patch[pos];
        pos += 1;
        match cmd {
            0 => break, // EOF
            1..=GDIFF_DATA_MAX => {
                // DATA(N): copy N literal bytes from patch stream
                let n = cmd as usize;
                need!(n);
                out.extend_from_slice(&patch[pos..pos + n]);
                pos += n;
            }
            GDIFF_DATA_USHORT => {
                let n = read_u16_be!();
                need!(n);
                out.extend_from_slice(&patch[pos..pos + n]);
                pos += n;
            }
            GDIFF_DATA_INT => {
                let n = read_u32_be!();
                need!(n);
                out.extend_from_slice(&patch[pos..pos + n]);
                pos += n;
            }
            GDIFF_COPY_USHORT_UBYTE => {
                let offset = read_u16_be!();
                let length = read_u8!() as usize;
                copy_from_source(original, offset, length, &mut out)?;
            }
            GDIFF_COPY_USHORT_USHORT => {
                let offset = read_u16_be!();
                let length = read_u16_be!();
                copy_from_source(original, offset, length, &mut out)?;
            }
            GDIFF_COPY_USHORT_INT => {
                let offset = read_u16_be!();
                let length = read_u32_be!();
                copy_from_source(original, offset, length, &mut out)?;
            }
            GDIFF_COPY_INT_UBYTE => {
                let offset = read_u32_be!();
                let length = read_u8!() as usize;
                copy_from_source(original, offset, length, &mut out)?;
            }
            GDIFF_COPY_INT_USHORT => {
                let offset = read_u32_be!();
                let length = read_u16_be!();
                copy_from_source(original, offset, length, &mut out)?;
            }
            GDIFF_COPY_INT_INT => {
                let offset = read_u32_be!();
                let length = read_u32_be!();
                copy_from_source(original, offset, length, &mut out)?;
            }
            GDIFF_COPY_LONG_INT => {
                let offset = read_u64_be!();
                let length = read_u32_be!();
                copy_from_source(original, offset, length, &mut out)?;
            }
        }
    }
    Ok(out)
}

/// Copy `length` bytes from `source` at `offset` into `out`.
#[inline]
fn copy_from_source(source: &[u8], offset: usize, length: usize, out: &mut Vec<u8>) -> crate::error::Result<()> {
    let end = offset.checked_add(length).ok_or_else(|| patcher_fail("binarypatcher", &"COPY offset+length overflow"))?;
    if end > source.len() {
        return Err(patcher_fail(
            "binarypatcher",
            &format!("COPY out of bounds: offset={offset} len={length} src_len={}", source.len()),
        ));
    }
    out.extend_from_slice(&source[offset..end]);
    Ok(())
}

pub async fn run(args: Vec<String>, ctx: &ProcessorContext) -> Result<()> {
    // Forge default — flag-based routing for back-compat with 1.0.12 Rust path.
    run_impl(args, ctx, false).await
}

/// NeoForge's binarypatcher 2.1.2 has no `--data`/`--unpatched` flags but
/// still requires the Java shell-out path (pure-Rust 1.0.12 semantics produce
/// wrong output for 2.x format). Coord-based dispatch from `run_processor`
/// calls this entry with `is_neoforge=true` (ADDENDUM B).
pub async fn run_neoforge(args: Vec<String>, ctx: &ProcessorContext) -> Result<()> {
    run_impl(args, ctx, true).await
}

async fn run_impl(args: Vec<String>, ctx: &ProcessorContext, is_neoforge: bool) -> Result<()> {
    use std::io::{Read, Write};

    // Coord-based force-Java route for NeoForge (ADDENDUM B). Skip Rust path
    // entirely — NeoForge binarypatcher 2.x JAR is incompatible with the
    // pure-Rust 1.0.12 GDiff path.
    if is_neoforge {
        return run_via_java(&args, ctx, true).await;
    }

    let parsed = parse_args(&args)?;

    // Modern-era 1.20.4+ install profiles invoke binarypatcher 1.2.0
    // with `--data --unpatched` flags. The 1.2.0 binpatch wire format and
    // GDiff payload semantics differ subtly from 1.0.12 (the version Phase 2
    // covers): observed during e2e — patches reference offsets past the
    // FART output end, indicating the 1.2.0 reference implementation does
    // extra processing on `--clean` before patching (or its binpatch entries
    // carry additional header bytes we don't parse). Re-implementing 1.2.0's
    // exact behavior pure-Rust would re-create the same byte-fidelity quagmire
    // the SpecialSource → FART pivot already solved. Per
    // `docs/superpowers/specs/2026-05-16-forge-loader-design.md#addendum-2026-05-17-b--specialsource-shell-out`,
    // bytecode-rewriting processors that produce inputs to / outputs from
    // byte-offset-encoded patches shell out to the canonical Java tool.
    //
    // Routing key: presence of either 1.2.0 flag. Phase 2's transitional
    // pipeline (binarypatcher 1.0.12) emits neither and stays on the pure-Rust
    // path that has been e2e-validated against 1.16.5.
    let is_v1_2_0 = args.iter().any(|a| a == "--data" || a == "--unpatched");
    if is_v1_2_0 {
        return run_via_java(&args, ctx, false).await;
    }

    // 1. Load + decompress patches.
    let lzma_bytes = tokio::fs::read(&parsed.apply).await.map_err(|e| {
        patcher_fail("binarypatcher", &format!("read {}: {e}", parsed.apply))
    })?;
    let patch_container_bytes = decompress_lzma(&lzma_bytes)?;
    let mut patch_zip = zip::ZipArchive::new(std::io::Cursor::new(patch_container_bytes)).map_err(|e| {
        patcher_fail("binarypatcher", &format!("patch container zip: {e}"))
    })?;

    // Build a map: source_class_name → BinPatch.
    let mut patches: std::collections::HashMap<String, BinPatch> = std::collections::HashMap::new();
    for i in 0..patch_zip.len() {
        let mut entry = patch_zip.by_index(i).map_err(|e| {
            patcher_fail("binarypatcher", &format!("patch entry {i}: {e}"))
        })?;
        let name = entry.name().to_string();
        if !name.ends_with(".binpatch") { continue; }
        let mut buf = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut buf).map_err(|e| {
            patcher_fail("binarypatcher", &format!("read patch {name}: {e}"))
        })?;
        let p = parse_binpatch(&buf)?;
        patches.insert(p.source_class.clone(), p);
    }

    // 2. Open clean jar + create output jar.
    let clean_bytes = tokio::fs::read(&parsed.clean).await.map_err(|e| {
        patcher_fail("binarypatcher", &format!("read {}: {e}", parsed.clean))
    })?;
    let mut clean_zip = zip::ZipArchive::new(std::io::Cursor::new(clean_bytes)).map_err(|e| {
        patcher_fail("binarypatcher", &format!("clean zip: {e}"))
    })?;
    if let Some(parent) = std::path::Path::new(&parsed.output).parent() {
        tokio::fs::create_dir_all(parent).await.ok();
    }
    let out_file = std::fs::File::create(&parsed.output).map_err(|e| {
        patcher_fail("binarypatcher", &format!("create {}: {e}", parsed.output))
    })?;
    let mut out_zip = zip::ZipWriter::new(out_file);
    let opts: zip::write::FileOptions<()> = zip::write::FileOptions::default();

    // 3. Apply each patch + pass through non-patched entries.
    let mut consumed: std::collections::HashSet<String> = std::collections::HashSet::new();
    for i in 0..clean_zip.len() {
        // Tighten ZipFile lifetime into a sync block so no .await is held
        // while a ZipFile borrow is alive (satisfies Send constraint).
        let (class_key, name, buf) = {
            let mut entry = clean_zip.by_index(i).map_err(|e| {
                patcher_fail("binarypatcher", &format!("clean idx {i}: {e}"))
            })?;
            if entry.is_dir() { continue; }
            let name = entry.name().to_string();
            let mut buf = Vec::with_capacity(entry.size() as usize);
            entry.read_to_end(&mut buf).map_err(|e| {
                patcher_fail("binarypatcher", &format!("clean read {name}: {e}"))
            })?;
            // Match by class FQN (strip `.class` suffix, use `/`-separated form).
            let class_key = name.strip_suffix(".class").unwrap_or(&name).to_string();
            (class_key, name, buf)
        };
        let (out_name, out_bytes) = match patches.get(&class_key) {
            Some(p) => {
                consumed.insert(class_key.clone());
                // Empty payload = "delete this class" — skip writing to output.
                if p.patch_payload.is_empty() {
                    continue;
                }
                eprintln!("binarypatcher: applying patch to {} (clean: {} bytes, patch: {} bytes)",
                    class_key, buf.len(), p.patch_payload.len());
                let patched = apply_gdiff(&buf, &p.patch_payload).map_err(|e| {
                    patcher_fail("binarypatcher", &format!("apply patch for {class_key}: {e}"))
                })?;
                let new_name = format!("{}.class", p.target_class);
                (new_name, patched)
            }
            None => (name.clone(), buf),
        };
        out_zip.start_file::<_, ()>(out_name.clone(), opts).map_err(|e| {
            patcher_fail("binarypatcher", &format!("zip start {out_name}: {e}"))
        })?;
        out_zip.write_all(&out_bytes).map_err(|e| {
            patcher_fail("binarypatcher", &format!("zip write {out_name}: {e}"))
        })?;
    }

    // 4. Emit patches that didn't match any clean entry — these are NEW classes
    // Forge adds on top of vanilla (e.g., TagRegistry$OptionalNamedTag).
    // The patch is a GDiff against an empty source, so apply_gdiff(b"", payload)
    // yields the new class bytes.
    for (source_class, p) in &patches {
        if consumed.contains(source_class) { continue; }
        if p.patch_payload.is_empty() { continue; } // empty + unmatched = no-op
        eprintln!("binarypatcher: emitting NEW class {} from patch (payload: {} bytes)",
            p.target_class, p.patch_payload.len());
        let new_bytes = apply_gdiff(&[], &p.patch_payload).map_err(|e| {
            patcher_fail("binarypatcher", &format!("apply patch for NEW class {}: {e}", p.target_class))
        })?;
        let new_name = format!("{}.class", p.target_class);
        out_zip.start_file::<_, ()>(new_name.clone(), opts).map_err(|e| {
            patcher_fail("binarypatcher", &format!("zip start {new_name}: {e}"))
        })?;
        out_zip.write_all(&new_bytes).map_err(|e| {
            patcher_fail("binarypatcher", &format!("zip write {new_name}: {e}"))
        })?;
    }

    out_zip.finish().map_err(|e| patcher_fail("binarypatcher", &format!("zip finish: {e}")))?;
    Ok(())
}

/// Java-subprocess shell-out for binarypatcher 1.2.0+ (Forge) and 2.1.2
/// (NeoForge). Spawns `java -cp <ctx.classpath joined> <main-class> <args>`.
///
/// Main-Class:
///   - Forge `net.minecraftforge:binarypatcher:1.2.0+` → `net.minecraftforge.binarypatcher.ConsoleTool`
///   - NeoForge `net.neoforged.installertools:binarypatcher:2.1.2` → `net.neoforged.binarypatcher.ConsoleTool`
async fn run_via_java(args: &[String], ctx: &ProcessorContext, is_neoforge: bool) -> Result<()> {
    use std::path::PathBuf;
    let java_bin = ctx.java_bin.clone().unwrap_or_else(|| PathBuf::from("java"));
    // Select the main class by flavor (ADDENDUM B).
    let main_class = if is_neoforge {
        "net.neoforged.binarypatcher.ConsoleTool"
    } else {
        "net.minecraftforge.binarypatcher.ConsoleTool"
    };
    crate::process::run_java_processor(
        &java_bin,
        &ctx.classpath,
        main_class,
        args,
        "binarypatcher",
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_args_three_required() {
        let raw = vec![
            "--clean".into(), "/i".into(),
            "--output".into(), "/o".into(),
            "--apply".into(), "/p".into(),
        ];
        let p = parse_args(&raw).unwrap();
        assert_eq!((p.clean.as_str(), p.output.as_str(), p.apply.as_str()), ("/i", "/o", "/p"));
    }

    #[test]
    fn parse_args_missing_required_errors() {
        assert!(parse_args(&vec!["--clean".into(), "/i".into()]).is_err());
    }

    #[test]
    fn parse_args_unknown_flag_errors() {
        assert!(parse_args(&vec!["--mysterious".into()]).is_err());
    }

    #[test]
    fn parse_args_accepts_v1_2_0_flags() {
        let raw = vec![
            "--clean".into(), "/c".into(),
            "--output".into(), "/o".into(),
            "--apply".into(), "/p".into(),
            "--data".into(),
            "--unpatched".into(),
        ];
        let parsed = parse_args(&raw).expect("1.2.0 flags must parse");
        assert_eq!(parsed.clean, "/c");
        assert_eq!(parsed.output, "/o");
        assert_eq!(parsed.apply, "/p");
    }

    #[test]
    fn parse_args_accepts_v1_3_0_flags() {
        // Forge 26.1.x / MC 26.1.x ships binarypatcher 1.3.0 with two new
        // flags: `--store` (bool) and `--marker <path>` (value-bearing).
        // Our parser must accept both so the Java shell-out route can
        // forward them; the routing key `--data || --unpatched` still
        // triggers the Java path.
        let raw = vec![
            "--clean".into(), "/c".into(),
            "--output".into(), "/o".into(),
            "--apply".into(), "/p".into(),
            "--data".into(),
            "--unpatched".into(),
            "--store".into(),
            "--marker".into(), ".forge_patched_minecraft".into(),
        ];
        let parsed = parse_args(&raw).expect("1.3.0 flags must parse");
        assert_eq!(parsed.clean, "/c");
        assert_eq!(parsed.output, "/o");
        assert_eq!(parsed.apply, "/p");
    }

    // ── GDiff + parse_binpatch tests ──────────────────────────────────────────

    /// Build a minimal GDiff v4 payload that copies all bytes from source
    /// and appends one literal byte.
    fn make_gdiff_payload(source: &[u8], extra: u8) -> Vec<u8> {
        let mut p = vec![0xd1u8, 0xff, 0xd1, 0xff, 0x04]; // magic
        // COPY_USHORT_USHORT: offset=0, length=source.len()
        assert!(source.len() <= 65535, "test helper: source too large");
        p.push(GDIFF_COPY_USHORT_USHORT);
        p.extend_from_slice(&(0u16).to_be_bytes());
        p.extend_from_slice(&(source.len() as u16).to_be_bytes());
        // DATA(1): one literal byte
        p.push(1);
        p.push(extra);
        p.push(0); // EOF
        p
    }

    #[test]
    fn apply_gdiff_round_trip() {
        let original = b"hello world, this is original data!";
        let payload = make_gdiff_payload(original, b'!');
        let result = apply_gdiff(original, &payload).expect("apply_gdiff");
        let mut expected = original.to_vec();
        expected.push(b'!');
        assert_eq!(result, expected);
    }

    #[test]
    fn apply_gdiff_empty_payload_means_deleted() {
        let result = apply_gdiff(b"anything", &[]).expect("empty");
        assert!(result.is_empty());
    }

    #[test]
    fn apply_gdiff_bad_magic_errors() {
        let bad = b"\x00\x00\x00\x00\x04some data";
        assert!(apply_gdiff(b"orig", bad).is_err());
    }

    /// Construct a synthetic .binpatch entry and verify parse_binpatch round-trips.
    fn make_binpatch(obf: &str, srg: &str, exists: bool, checksum: u32, payload: &[u8]) -> Vec<u8> {
        let mut buf = vec![1u8]; // version
        // java-utf8: 2-byte BE length + bytes
        let write_java_utf = |b: &mut Vec<u8>, s: &str| {
            let bytes = s.as_bytes();
            b.extend_from_slice(&(bytes.len() as u16).to_be_bytes());
            b.extend_from_slice(bytes);
        };
        write_java_utf(&mut buf, obf);
        write_java_utf(&mut buf, srg);
        buf.push(if exists { 1 } else { 0 });
        if exists {
            buf.extend_from_slice(&checksum.to_be_bytes());
        }
        buf.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        buf.extend_from_slice(payload);
        buf
    }

    #[test]
    fn parse_binpatch_round_trip() {
        let dummy_payload = b"\xd1\xff\xd1\xff\x04\x05hello\x00"; // gdiff magic + DATA(5) + EOF
        let raw = make_binpatch("net/minecraft/Foo", "net/minecraft/Foo", true, 0xdeadbeef, dummy_payload);
        let patch = parse_binpatch(&raw).expect("parse");
        assert_eq!(patch.source_class, "net/minecraft/Foo");
        assert_eq!(patch.target_class, "net/minecraft/Foo");
        assert_eq!(patch.source_checksum, Some(0xdeadbeef));
        assert_eq!(patch.patch_payload, dummy_payload);
    }

    #[test]
    fn parse_binpatch_no_exists() {
        let raw = make_binpatch("a/B", "a/B", false, 0, b"");
        let patch = parse_binpatch(&raw).expect("parse");
        assert_eq!(patch.source_checksum, None);
        assert!(patch.patch_payload.is_empty());
    }

    #[test]
    fn parse_binpatch_bad_version_errors() {
        let mut raw = make_binpatch("x/Y", "x/Y", false, 0, b"");
        raw[0] = 2; // wrong version
        assert!(parse_binpatch(&raw).is_err());
    }

    #[test]
    fn lzma_round_trip() {
        use std::io::Write;
        // Use xz2 itself to encode, then decode. Goal: prove our reader path works.
        let original = b"some bytes to compress".repeat(10);
        let stream = xz2::stream::Stream::new_lzma_encoder(&xz2::stream::LzmaOptions::new_preset(6).unwrap()).unwrap();
        let mut enc = xz2::write::XzEncoder::new_stream(Vec::new(), stream);
        enc.write_all(&original).unwrap();
        let compressed = enc.finish().unwrap();
        let decompressed = decompress_lzma(&compressed).expect("decompress");
        assert_eq!(decompressed, original);
    }

    // ── Option B: synthesised empty-patches pipeline test ─────────────────────
    //
    // Builds a minimal clean.jar (2 non-class entries) + a patches.lzma
    // containing ZERO .binpatch entries, runs `run`, and asserts the output
    // jar contains the same entries (pass-through path).  Validates the
    // LZMA → ZIP → iterate → ZipWriter pipeline without needing valid GDiff
    // patch construction.

    /// Build a minimal in-memory ZIP with the given (name, bytes) entries.
    fn make_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        use std::io::Write;
        let buf = Vec::new();
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(buf));
        let opts: zip::write::FileOptions<()> = zip::write::FileOptions::default();
        for (name, data) in entries {
            w.start_file::<_, ()>(*name, opts).unwrap();
            w.write_all(data).unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    /// LZMA1-compress `input` bytes using xz2 (same codec as `decompress_lzma`).
    fn lzma_compress(input: &[u8]) -> Vec<u8> {
        use std::io::Write;
        let stream = xz2::stream::Stream::new_lzma_encoder(
            &xz2::stream::LzmaOptions::new_preset(1).unwrap(),
        )
        .unwrap();
        let mut enc = xz2::write::XzEncoder::new_stream(Vec::new(), stream);
        enc.write_all(input).unwrap();
        enc.finish().unwrap()
    }

    #[tokio::test]
    async fn run_passthrough_with_empty_patch_container() {
        use std::io::Read as _;

        // Two non-.class entries in the clean jar.
        let clean_entries: &[(&str, &[u8])] = &[
            ("META-INF/MANIFEST.MF", b"Manifest-Version: 1.0\n"),
            ("some/resource.txt", b"hello from clean jar"),
        ];
        let clean_zip_bytes = make_zip(clean_entries);

        // Empty patch container (a valid ZIP with no entries).
        let empty_patch_zip = make_zip(&[]);
        let patches_lzma = lzma_compress(&empty_patch_zip);

        // Write fixtures to a temp dir.
        let tmp = tempfile::tempdir().expect("tempdir");
        let clean_path = tmp.path().join("clean.jar");
        let patches_path = tmp.path().join("patches.lzma");
        let output_path = tmp.path().join("output.jar");

        std::fs::write(&clean_path, &clean_zip_bytes).unwrap();
        std::fs::write(&patches_path, &patches_lzma).unwrap();

        let args = vec![
            "--clean".to_string(),
            clean_path.to_string_lossy().to_string(),
            "--output".to_string(),
            output_path.to_string_lossy().to_string(),
            "--apply".to_string(),
            patches_path.to_string_lossy().to_string(),
        ];

        let ctx = ProcessorContext {
            classpath: vec![],
            cache_dir: tmp.path().to_path_buf(),
            java_bin: None,
        };
        run(args, &ctx).await.expect("run should succeed");

        // Read back and verify output contains the same entries.
        let out_bytes = std::fs::read(&output_path).expect("output jar");
        let mut out_zip = zip::ZipArchive::new(std::io::Cursor::new(out_bytes)).expect("open output zip");

        let expected_names: std::collections::HashSet<&str> =
            clean_entries.iter().map(|(n, _)| *n).collect();
        let actual_names: std::collections::HashSet<String> =
            (0..out_zip.len()).map(|i| out_zip.by_index(i).unwrap().name().to_string()).collect();

        assert_eq!(
            actual_names,
            expected_names.iter().map(|s| s.to_string()).collect(),
            "output jar should contain all clean entries (pass-through)",
        );

        // Verify entry contents were not corrupted.
        for (name, expected_data) in clean_entries {
            let mut entry = out_zip.by_name(name).expect("entry present");
            let mut actual = Vec::new();
            entry.read_to_end(&mut actual).unwrap();
            assert_eq!(actual, *expected_data, "entry {name} content mismatch");
        }
    }

    // ── Bug 1: empty-payload patch means "delete this class" ─────────────────

    #[tokio::test]
    async fn run_skips_empty_payload_patches() {
        // Clean jar: one class entry that has a matching empty-payload patch.
        let class_bytes = b"\xca\xfe\xba\xbe\x00\x00\x00\x3c"; // fake class header
        let clean_entries: &[(&str, &[u8])] = &[
            ("pkg/ToDelete.class", class_bytes),
        ];
        let clean_zip_bytes = make_zip(clean_entries);

        // Build a binpatch with empty payload for pkg/ToDelete.
        let empty_patch_raw = make_binpatch("pkg/ToDelete", "pkg/ToDelete", true, 0xdeadc0de, b"");
        // Pack it into a patch-container ZIP.
        let patch_container = make_zip(&[("pkg/ToDelete.binpatch", &empty_patch_raw)]);
        let patches_lzma = lzma_compress(&patch_container);

        let tmp = tempfile::tempdir().expect("tempdir");
        let clean_path = tmp.path().join("clean.jar");
        let patches_path = tmp.path().join("patches.lzma");
        let output_path = tmp.path().join("output.jar");

        std::fs::write(&clean_path, &clean_zip_bytes).unwrap();
        std::fs::write(&patches_path, &patches_lzma).unwrap();

        let args = vec![
            "--clean".to_string(), clean_path.to_string_lossy().to_string(),
            "--output".to_string(), output_path.to_string_lossy().to_string(),
            "--apply".to_string(), patches_path.to_string_lossy().to_string(),
        ];
        let ctx = ProcessorContext {
            classpath: vec![],
            cache_dir: tmp.path().to_path_buf(),
            java_bin: None,
        };
        run(args, &ctx).await.expect("run should succeed");

        let out_bytes = std::fs::read(&output_path).expect("output jar");
        let mut out_zip = zip::ZipArchive::new(std::io::Cursor::new(out_bytes)).expect("open output zip");

        // The deleted class must NOT be present.
        assert!(
            out_zip.by_name("pkg/ToDelete.class").is_err(),
            "empty-payload patch: class should be absent from output jar"
        );
        // Output jar should be empty (no other entries).
        assert_eq!(out_zip.len(), 0, "output jar should have no entries");
    }

    // ── Bug 2: unmatched patches emit new classes ─────────────────────────────

    /// Build a minimal GDiff payload that emits `data` literal bytes from an
    /// empty source.  Uses DATA(N) command (tag = N, for N <= 246).
    fn make_gdiff_from_empty(data: &[u8]) -> Vec<u8> {
        assert!(data.len() <= 246, "test helper: use DATA_USHORT for larger payloads");
        let mut p = vec![0xd1u8, 0xff, 0xd1, 0xff, 0x04]; // magic
        p.push(data.len() as u8); // DATA(N) tag
        p.extend_from_slice(data);
        p.push(0); // EOF
        p
    }

    #[tokio::test]
    async fn run_emits_new_classes_from_unmatched_patches() {
        use std::io::Read as _;

        // Clean jar: one existing class.
        let existing_bytes = b"\xca\xfe\xba\xbe\x00\x00\x00\x3c"; // fake class
        let clean_entries: &[(&str, &[u8])] = &[
            ("pkg/Existing.class", existing_bytes),
        ];
        let clean_zip_bytes = make_zip(clean_entries);

        // New-class patch: source does not exist in clean jar, payload is a GDiff
        // against empty source that emits "hello" bytes.
        let new_class_payload = make_gdiff_from_empty(b"hello");
        // exists=false (source absent from vanilla), no checksum field.
        let new_patch_raw = make_binpatch("pkg/NewClass", "pkg/NewClass", false, 0, &new_class_payload);
        let patch_container = make_zip(&[("pkg/NewClass.binpatch", &new_patch_raw)]);
        let patches_lzma = lzma_compress(&patch_container);

        let tmp = tempfile::tempdir().expect("tempdir");
        let clean_path = tmp.path().join("clean.jar");
        let patches_path = tmp.path().join("patches.lzma");
        let output_path = tmp.path().join("output.jar");

        std::fs::write(&clean_path, &clean_zip_bytes).unwrap();
        std::fs::write(&patches_path, &patches_lzma).unwrap();

        let args = vec![
            "--clean".to_string(), clean_path.to_string_lossy().to_string(),
            "--output".to_string(), output_path.to_string_lossy().to_string(),
            "--apply".to_string(), patches_path.to_string_lossy().to_string(),
        ];
        let ctx = ProcessorContext {
            classpath: vec![],
            cache_dir: tmp.path().to_path_buf(),
            java_bin: None,
        };
        run(args, &ctx).await.expect("run should succeed");

        let out_bytes = std::fs::read(&output_path).expect("output jar");
        let mut out_zip = zip::ZipArchive::new(std::io::Cursor::new(out_bytes)).expect("open output zip");

        // Existing class must still be present and unmodified.
        {
            let mut entry = out_zip.by_name("pkg/Existing.class").expect("existing class present");
            let mut actual = Vec::new();
            entry.read_to_end(&mut actual).unwrap();
            assert_eq!(actual, existing_bytes, "existing class content must be unchanged");
        }

        // New class must be present with the expected content.
        {
            let mut entry = out_zip.by_name("pkg/NewClass.class").expect("new class must be present");
            let mut actual = Vec::new();
            entry.read_to_end(&mut actual).unwrap();
            assert_eq!(actual, b"hello", "new class content must match patch output");
        }

        assert_eq!(out_zip.len(), 2, "output jar should contain exactly 2 entries");
    }
}
