//! `DOWNLOAD_MOJMAPS`: the one Forge/NeoForge install processor Lucerna does
//! not run on Java, because it downloads and every download goes through
//! `network::`. Without `--sanitize` the Mojang mappings are written verbatim,
//! as the Java task's `Files.copy` does. With `--sanitize` they are converted
//! to TSRG exactly as srgutils 0.5.10 does (`IMappingFile.load(in).write(out,
//! TSRG, false)`), byte for byte. Forge declares the result's SHA-1
//! (`MOJMAPS_SHA`) and the processor stage refuses any other bytes (spec
//! 2026-10-16 §5.2).

use crate::error::{Error, Result};
use crate::forge::patcher::patcher_fail;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::path::PathBuf;

const TASK: &str = "installertools::DOWNLOAD_MOJMAPS";

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct MojmapsArgs {
    pub version: String,
    pub side: String,
    pub output: PathBuf,
    pub sanitize: bool,
}

/// The Java task's options are `--version --side --output --sanitize --format
/// --skipIfExists`. The last two would change the bytes or skip the work; they
/// are refused, as is anything unknown, rather than silently ignored.
pub(crate) fn parse_args(coord: &str, args: &[String]) -> Result<MojmapsArgs> {
    let (mut version, mut side, mut output, mut sanitize) = (None, None, None, false);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--task" => {
                // DOWNLOAD_MOJMAPS: the dispatcher routed on it.
                it.next();
            }
            "--version" => version = it.next().cloned(),
            "--side" => side = it.next().cloned(),
            "--output" => output = it.next().map(PathBuf::from),
            "--sanitize" => sanitize = true,
            other => {
                return Err(Error::ForgeUnsupportedProcessor {
                    coord: format!("{coord} DOWNLOAD_MOJMAPS {other}"),
                })
            }
        }
    }
    let version = version.ok_or_else(|| patcher_fail(TASK, &"missing --version"))?;
    let side = side.ok_or_else(|| patcher_fail(TASK, &"missing --side"))?;
    if side != "client" && side != "server" {
        return Err(patcher_fail(
            TASK,
            &format!("unknown side `{side}` (expected client|server)"),
        ));
    }
    let output = output.ok_or_else(|| patcher_fail(TASK, &"missing --output"))?;
    Ok(MojmapsArgs {
        version,
        side,
        output,
        sanitize,
    })
}

/// Run the task: fetch `downloads.<side>_mappings` for `version` from Mojang's
/// manifest (sha1-verified), then write it verbatim or converted.
pub async fn run(coord: &str, args: &[String]) -> Result<()> {
    let a = parse_args(coord, args)?;
    let (url, sha1) = mappings_download(&a.version, &a.side).await?;
    if let Some(parent) = a.output.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| Error::io(parent.display().to_string(), e))?;
    }
    if !a.sanitize {
        return crate::network::download::download_no_emit(&url, &a.output, &sha1, "forge-mojmaps")
            .await;
    }
    let raw = a.output.with_extension("proguard.part");
    crate::network::download::download_no_emit(&url, &raw, &sha1, "forge-mojmaps").await?;
    let text = tokio::fs::read_to_string(&raw)
        .await
        .map_err(|e| Error::io(raw.display().to_string(), e))?;
    let tsrg = proguard_to_tsrg(&text).map_err(|why| patcher_fail(TASK, &why))?;
    let tmp = a.output.with_extension("tsrg.part");
    tokio::fs::write(&tmp, tsrg.as_bytes())
        .await
        .map_err(|e| Error::io(tmp.display().to_string(), e))?;
    tokio::fs::rename(&tmp, &a.output)
        .await
        .map_err(|e| Error::io(a.output.display().to_string(), e))?;
    if let Err(e) = tokio::fs::remove_file(&raw).await {
        // A leftover download in the libraries tree: harmless and overwritten
        // next time, but said, never silent.
        crate::diag!("mojmaps: could not remove {}: {e}", raw.display());
    }
    Ok(())
}

/// `downloads.<side>_mappings` of `version` in Mojang's manifest: its URL and SHA-1.
async fn mappings_download(version: &str, side: &str) -> Result<(String, String)> {
    let manifest_entries = crate::versions::manifest::list_manifest().await?;
    let entry = manifest_entries
        .iter()
        .find(|e| e.id == version)
        .ok_or_else(|| {
            patcher_fail(
                TASK,
                &format!("MC version `{version}` not present in Mojang manifest"),
            )
        })?;
    let version_json: serde_json::Value =
        crate::network::get_json(&entry.url, "forge-mojmaps").await?;
    let key = format!("{side}_mappings");
    let dl = version_json
        .get("downloads")
        .and_then(|d| d.get(&key))
        .ok_or_else(|| {
            patcher_fail(
                TASK,
                &format!("version JSON has no downloads.{key} (vanilla MC {version} may predate Mojang mapping publishing)"),
            )
        })?;
    let url = dl.get("url").and_then(|v| v.as_str()).ok_or_else(|| {
        patcher_fail(
            TASK,
            &format!("downloads.{key}.url missing or not a string"),
        )
    })?;
    let sha1 = require_mapping_sha1(dl, &key)?;
    Ok((url.to_string(), sha1.to_string()))
}

fn require_mapping_sha1<'a>(dl: &'a serde_json::Value, key: &str) -> Result<&'a str> {
    dl.get("sha1")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            patcher_fail(
                TASK,
                &format!(
                    "downloads.{key}.sha1 missing or empty — refusing to install an unverified Mojang mapping (B.6)"
                ),
            )
        })
}

// ---- srgutils 0.5.10: ProGuard -> TSRG -------------------------------------
//
// A transliteration of `InternalUtils.loadNamed`/`filter`/`stripComment`/
// `loadProguard`/`toDesc` and `MappingFile.write(…, TSRG, false)`. Each helper
// names the Java behaviour it reproduces. Validated against the MOJMAPS_SHA
// Forge publishes for 1.20.6, 1.21.1 and 1.21.11.

/// Java's `String.compareTo`: lexicographic over UTF-16 code units.
fn java_cmp(a: &str, b: &str) -> Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

/// Java's `String.hashCode()`.
pub(crate) fn java_hash(s: &str) -> i32 {
    s.encode_utf16()
        .fold(0i32, |h, c| h.wrapping_mul(31).wrapping_add(c as i32))
}

/// `java.util.HashMap.hash`: the high half folded into the low half.
fn spread(h: i32) -> u32 {
    let h = h as u32;
    h ^ (h >> 16)
}

/// `BufferedReader.lines()`: lines end at `\n`, `\r` or `\r\n`; a final line
/// end does not start an empty line.
fn java_lines(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let (mut start, mut i) = (0usize, 0usize);
    while i < bytes.len() {
        match bytes[i] {
            b'\n' => {
                out.push(&text[start..i]);
                i += 1;
                start = i;
            }
            b'\r' => {
                out.push(&text[start..i]);
                i += if bytes.get(i + 1) == Some(&b'\n') {
                    2
                } else {
                    1
                };
                start = i;
            }
            _ => i += 1,
        }
    }
    if start < bytes.len() {
        out.push(&text[start..]);
    }
    out
}

/// Java's `String.split(sep)` for a one-character separator: trailing empty
/// strings are removed; an empty input yields one empty string.
fn java_split(s: &str, sep: char) -> Vec<&str> {
    if s.is_empty() {
        return vec![""];
    }
    let mut v: Vec<&str> = s.split(sep).collect();
    while v.last() == Some(&"") {
        v.pop();
    }
    v
}

/// Java's `String.trim()`: strips characters <= U+0020 at both ends.
fn java_trim(s: &str) -> &str {
    s.trim_matches(|c: char| c <= ' ')
}

/// srgutils' `InternalUtils.stripComment`, quirk included: an inline `#` also
/// drops the character before it (`substring(0, idx - 1)`), and trailing
/// spaces are trimmed down to at least one character.
pub(crate) fn strip_comment(s: &str) -> String {
    let units: Vec<u16> = s.encode_utf16().collect();
    let mut end = units.len();
    if let Some(idx) = units.iter().position(|&u| u == u16::from(b'#')) {
        if idx == 0 {
            return String::new();
        }
        end = idx - 1;
    }
    while end > 1 && units[end - 1] == u16::from(b' ') {
        end -= 1;
    }
    String::from_utf16_lossy(&units[..end])
}

/// srgutils' `InternalUtils.toDesc`.
fn to_desc(t: &str) -> std::result::Result<String, String> {
    if let Some(inner) = t.strip_suffix("[]") {
        return Ok(format!("[{}", to_desc(inner)?));
    }
    let primitive = match t {
        "int" => "I",
        "void" => "V",
        "boolean" => "Z",
        "byte" => "B",
        "char" => "C",
        "short" => "S",
        "double" => "D",
        "float" => "F",
        "long" => "J",
        _ if t.contains('/') => return Ok(format!("L{t};")),
        _ => return Err(format!("Invalid toDesc input: {t}")),
    };
    Ok(primitive.to_string())
}

struct Method {
    name: String,
    desc: String,
    obf: String,
}

#[derive(Default)]
struct Class {
    obf: String,
    /// Field name -> obfuscated name. Keyed by name; a repeat replaces the value.
    fields: HashMap<String, String>,
    /// First-insertion order (what `java.util.HashMap` keeps within a bucket),
    /// keyed by `name + desc`; a repeat replaces the obfuscated name in place.
    methods: Vec<Method>,
    method_index: HashMap<String, usize>,
}

impl Class {
    fn put_method(&mut self, name: &str, desc: String, obf: &str) {
        let key = format!("{name}{desc}");
        match self.method_index.get(&key) {
            Some(&i) => self.methods[i].obf = obf.to_string(),
            None => {
                self.method_index.insert(key, self.methods.len());
                self.methods.push(Method {
                    name: name.to_string(),
                    desc,
                    obf: obf.to_string(),
                });
            }
        }
    }
}

/// The table size `java.util.HashMap` ends with after these hashes are put in
/// this order (srgutils keys methods by `name + desc` in a default HashMap).
/// A bin that reaches 9 nodes doubles the table while it is smaller than 64,
/// and treeifies after that. Iteration order then follows the tree, which is
/// not reproduced, so it is refused rather than guessed.
fn table_cap(hashes: &[u32]) -> std::result::Result<usize, String> {
    let recount = |cap: usize, upto: usize| {
        let mut c = vec![0u32; cap];
        for h in &hashes[..upto] {
            c[(*h as usize) & (cap - 1)] += 1;
        }
        c
    };
    let mut cap = 16usize;
    let mut counts = vec![0u32; cap];
    for (i, h) in hashes.iter().enumerate() {
        let size = i + 1;
        let b = (*h as usize) & (cap - 1);
        counts[b] += 1;
        if counts[b] >= 9 {
            if cap < 64 {
                cap *= 2;
                counts = recount(cap, size);
            } else {
                return Err(
                    "a java.util.HashMap bucket would treeify; srgutils' method order cannot be reproduced"
                        .into(),
                );
            }
        }
        if size > cap * 3 / 4 {
            cap *= 2;
            counts = recount(cap, size);
        }
    }
    Ok(cap)
}

/// Convert ProGuard mappings to TSRG exactly as srgutils 0.5.10 writes them.
pub(crate) fn proguard_to_tsrg(text: &str) -> std::result::Result<String, String> {
    let lines: Vec<&str> = java_lines(text)
        .into_iter()
        .filter(|l| !l.is_empty())
        .collect();
    // srgutils detects the format from the first line that is not a comment.
    match lines.iter().find(|l| !strip_comment(l).is_empty()) {
        Some(first) if first.contains(" -> ") => {}
        _ => return Err("not a ProGuard mapping file".into()),
    }

    let mut classes: HashMap<String, Class> = HashMap::new();
    let mut current: Option<String> = None;
    for raw in lines {
        let stripped = strip_comment(raw);
        if stripped.is_empty() {
            continue;
        }
        let line = stripped.replace('.', "/");
        if !line.starts_with("    ") && line.ends_with(':') {
            let mut parts = line.split(" -> ");
            let original = parts.next().unwrap_or_default().to_string();
            let segment = parts
                .next()
                .ok_or_else(|| format!("class line without ' -> ': {raw}"))?;
            let mut obf = segment.to_string();
            obf.pop(); // srgutils: `substring(0, length - 1)`, the trailing ':'
                       // A repeated header replaces the class, members included (`retPut`).
            classes.insert(
                original.clone(),
                Class {
                    obf,
                    ..Class::default()
                },
            );
            current = Some(original);
        } else if line.contains('(') && line.contains(')') {
            let cls = current
                .as_ref()
                .and_then(|c| classes.get_mut(c))
                .ok_or_else(|| format!("Invalid PG line, missing class: {raw}"))?;
            let mut l = java_trim(&line);
            if let Some(i) = l.find(':') {
                let j = l[i + 1..]
                    .find(':')
                    .map(|j| i + 1 + j)
                    .ok_or_else(|| format!("line range without a second ':': {raw}"))?;
                l[..i]
                    .parse::<i32>()
                    .and(l[i + 1..j].parse::<i32>())
                    .map_err(|_| format!("line range is not numeric: {raw}"))?;
                l = &l[j + 1..];
            }
            let obf = l
                .split(" -> ")
                .nth(1)
                .ok_or_else(|| format!("method line without ' -> ': {raw}"))?;
            let ret = to_desc(l.split(' ').next().unwrap_or_default())?;
            let (space, open, close) = match (l.find(' '), l.find('('), l.find(')')) {
                (Some(s), Some(o), Some(c)) if s < o && o < c => (s, o, c),
                _ => return Err(format!("malformed method line: {raw}")),
            };
            let name = &l[space + 1..open];
            let mut desc = String::from("(");
            for arg in java_split(&l[open + 1..close], ',') {
                if arg.is_empty() {
                    break;
                }
                desc.push_str(&to_desc(arg)?);
            }
            desc.push(')');
            desc.push_str(&ret);
            cls.put_method(name, desc, obf);
        } else {
            let cls = current
                .as_ref()
                .and_then(|c| classes.get_mut(c))
                .ok_or_else(|| format!("Invalid PG line, missing class: {raw}"))?;
            let pts = java_split(java_trim(&line), ' ');
            if pts.len() < 4 {
                return Err(format!("malformed field line: {raw}"));
            }
            to_desc(pts[0])?;
            cls.fields.insert(pts[1].to_string(), pts[3].to_string());
        }
    }

    let mut out = String::with_capacity(text.len());
    let mut names: Vec<&String> = classes.keys().collect();
    names.sort_by(|a, b| java_cmp(a, b));
    for original in names {
        let cls = &classes[original];
        out.push_str(original);
        out.push(' ');
        out.push_str(&cls.obf);
        out.push('\n');

        let mut fields: Vec<(&String, &String)> = cls.fields.iter().collect();
        fields.sort_by(|a, b| java_cmp(a.0, b.0));
        for (name, obf) in fields {
            out.push('\t');
            out.push_str(name);
            out.push(' ');
            out.push_str(obf);
            out.push('\n');
        }

        let hashes: Vec<u32> = cls
            .methods
            .iter()
            .map(|m| spread(java_hash(&format!("{}{}", m.name, m.desc))))
            .collect();
        let cap = if hashes.is_empty() {
            16
        } else {
            table_cap(&hashes)?
        };
        let mut order: Vec<usize> = (0..cls.methods.len()).collect(); // insertion order
        order.sort_by_key(|&i| (hashes[i] as usize) & (cap - 1)); // HashMap iteration (stable)
        order.sort_by(|&a, &b| java_cmp(&cls.methods[a].name, &cls.methods[b].name)); // srgutils' stable sort
        for i in order {
            let m = &cls.methods[i];
            out.push('\t');
            out.push_str(&m.name);
            out.push(' ');
            out.push_str(&m.desc);
            out.push(' ');
            out.push_str(&m.obf);
            out.push('\n');
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> String {
        std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/mojmaps")
                .join(name),
        )
        .expect("fixture")
    }

    #[test]
    fn synthetic_proguard_converts_exactly_as_srgutils_0_5_10() {
        let got = proguard_to_tsrg(&fixture("synthetic.proguard")).expect("converts");
        let want = fixture("synthetic.tsrg");
        assert_eq!(got, want);
    }

    #[test]
    fn strip_comment_drops_the_char_before_an_inline_hash() {
        assert_eq!(strip_comment("# whole line"), "");
        assert_eq!(strip_comment("    int x -> a # c"), "    int x -> a");
        assert_eq!(strip_comment("ab#"), "a");
        assert_eq!(strip_comment("   "), " ");
    }

    #[test]
    fn java_hash_matches_string_hash_code() {
        // "hello".hashCode() == 99162322; "" -> 0; overflow wraps.
        assert_eq!(java_hash("hello"), 99_162_322);
        assert_eq!(java_hash(""), 0);
        assert_eq!(java_hash("polygenelubricants"), i32::MIN);
    }

    #[test]
    fn a_non_proguard_file_is_refused() {
        assert!(proguard_to_tsrg("a b\n\tc d\n").is_err());
    }

    #[test]
    fn unsupported_flags_are_refused_not_ignored() {
        for flag in ["--format", "--skipIfExists", "--bogus"] {
            let args: Vec<String> = [
                "--task",
                "DOWNLOAD_MOJMAPS",
                "--version",
                "1.21.1",
                "--side",
                "client",
                "--output",
                "o",
                flag,
            ]
            .iter()
            .map(|s| s.to_string())
            .collect();
            let err = parse_args("net.minecraftforge:installertools:1.4.3", &args).unwrap_err();
            assert!(
                matches!(err, crate::error::Error::ForgeUnsupportedProcessor { ref coord } if coord.contains(flag)),
                "{flag}: {err:?}"
            );
        }
    }

    #[test]
    fn parses_the_supported_flags() {
        let args: Vec<String> = [
            "--task",
            "DOWNLOAD_MOJMAPS",
            "--sanitize",
            "--version",
            "1.21.1",
            "--side",
            "client",
            "--output",
            "o.tsrg",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let a = parse_args("c", &args).unwrap();
        assert_eq!(
            a,
            MojmapsArgs {
                version: "1.21.1".into(),
                side: "client".into(),
                output: "o.tsrg".into(),
                sanitize: true,
            }
        );
    }
}
