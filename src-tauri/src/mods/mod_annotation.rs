//! The `@Mod` annotation — how Forge ≤ 1.12.2 declares a mod.
//!
//! On that era a mod IS a class carrying `@Mod`, and FML reads three things
//! off it: the mod-id, the requirements (`dependencies`), and the version every
//! requirement on that mod is measured against (`version`). `mcmod.info` is
//! display metadata: FML falls back to its `version` only when neither the
//! annotation nor `version.properties` names one (`FMLModContainer.bindMetadata`,
//! Forge 1.12.x L239-259).
//!
//! A real, if minimal, JVMS §4 class-file reader. Its predecessor searched the
//! raw bytes for `required-after:` and widened the hit to the surrounding
//! printable run; for a 33–126 byte string the `CONSTANT_Utf8` length byte is
//! itself printable, so it was glued onto the first clause and that clause was
//! silently dropped. And no byte pattern identifies the VERSION, which is what a
//! provider's range check needs.
//!
//! Every read is bounds-checked and malformed input is `None`, never a panic:
//! these bytes come from whatever jar the user dropped into `mods/`.

use std::collections::HashSet;
use std::io::{Cursor, Read};

use serde::{Deserialize, Serialize};

use crate::mods::local::{
    mcmod_info_list, parse_legacy_dependency_string, DeclaredDep, DescriptorSource, ProvidedMod,
};

type Zip<'a> = zip::ZipArchive<Cursor<&'a [u8]>>;

/// `@Mod` on Forge 1.8 – 1.12.2.
const MOD_DESCRIPTOR: &[u8] = b"Lnet/minecraftforge/fml/common/Mod;";
/// `@Mod` on Forge 1.7.10 and older, before the `cpw.mods.fml` repackaging.
const CPW_MOD_DESCRIPTOR: &[u8] = b"Lcpw/mods/fml/common/Mod;";

/// Element-value nesting allowed. `@Mod` itself needs two levels
/// (`customProperties = { @CustomProperty(..) }`); the bound exists so a crafted
/// class cannot recurse the blocking worker's stack away.
const MAX_ELEMENT_DEPTH: u32 = 16;

/// Largest `.class` entry inflated. Real classes are kilobytes; the cap turns a
/// deflate bomb into "could not tell" rather than an allocation failure that
/// aborts the whole process.
const MAX_CLASS_BYTES: u64 = 32 * 1024 * 1024;
/// Largest `version.properties` read — a handful of lines in practice.
const MAX_PROPERTIES_BYTES: u64 = 64 * 1024;
/// Largest `mcmod.info` read for `useDependencyInformation`.
const MAX_MCMOD_INFO_BYTES: u64 = 8 * 1024 * 1024;

/// What the `@Mod` annotations of one jar declare, folded into the pre-flight's
/// manifest by `preflight::JarScan::into_parts`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LegacyAnnotations {
    /// The requirements FML takes from the annotations, from every `@Mod` class
    /// in the jar.
    pub deps: Vec<DeclaredDep>,
    /// One provider per `@Mod` class, tagged `McmodAnnotation`. `version` is
    /// FML's first two steps — the annotation, then `version.properties` — or
    /// `None`, which leaves `mcmod.info` to answer through
    /// `preflight::descriptor_rank`.
    pub provided: Vec<ProvidedMod>,
}

/// The `@Mod` elements FML's dependency check reads.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct ModAnnotation {
    pub modid: Option<String>,
    pub version: Option<String>,
    pub dependencies: Option<String>,
    /// Tri-state on purpose: FML sets `overridesMetadata` only when the key is
    /// present (`FMLModContainer.java:213-216`), so "absent" and `true` agree
    /// and only `false` differs.
    pub use_metadata: Option<bool>,
}

/// Read every `@Mod` annotation of a jar, the way FML ≤ 1.12.2 discovers mods.
///
/// `None` is "could not tell": a class FML would scan would not inflate, was
/// over the cap, or would not parse — it could have been the `@Mod` class, and
/// FML itself ignores a jar it cannot read (`JarDiscoverer.java:81`) — or a
/// file the answer depends on could not be read. The caller turns that into the
/// scan's own "could not tell", which is listed as unjudged and never cached.
/// One `diag!` line names what failed.
///
/// FML's jar-level exclusions (a coremod without `FMLCorePluginContainsFMLMod`,
/// `TweakClass`, `ModSide`, `ModType`) are deliberately NOT mirrored: Mixin's FML
/// agent re-admits some of those jars, and mirroring the exclusions without that
/// rule would delete real providers.
pub(crate) fn read_jar_legacy_annotations(jar_bytes: &[u8]) -> Option<LegacyAnnotations> {
    let mut zip = zip::ZipArchive::new(Cursor::new(jar_bytes)).ok()?;
    let read = scan_classes(&mut zip).and_then(|found| declarations(&mut zip, found));
    match read {
        Ok(out) => Some(out),
        Err(reason) => {
            crate::diag!("[mods] legacy @Mod scan could not read a jar: {reason}");
            None
        }
    }
}

/// Every `@Mod` annotation in the classes FML's discovery would scan.
fn scan_classes(zip: &mut Zip<'_>) -> Result<Vec<ModAnnotation>, String> {
    // Names from the central directory: an entry whose local header is damaged
    // then fails the read below instead of silently vanishing from the list.
    let names: Vec<String> = zip
        .file_names()
        .filter(|n| fml_scans_entry(n))
        .map(str::to_string)
        .collect();
    let mut found = Vec::new();
    for name in &names {
        let bytes = read_capped(zip, name, MAX_CLASS_BYTES)?
            .ok_or_else(|| format!("{name} is listed but cannot be opened"))?;
        let candidate = find_subslice(&bytes, MOD_DESCRIPTOR).is_some()
            || find_subslice(&bytes, CPW_MOD_DESCRIPTOR).is_some();
        if !candidate {
            continue;
        }
        let annotations =
            mod_annotations(&bytes).ok_or_else(|| format!("{name} is not a readable class"))?;
        found.extend(annotations);
    }
    Ok(found)
}

/// FML's discovery filter for one entry: `JarDiscoverer` skips `__MACOSX`, then
/// `ITypeDiscoverer.classFile` (`[^\s\$]+(\$[^\s]+)?\.class$`, matched whole)
/// admits a name with no whitespace that does not start with `$`.
fn fml_scans_entry(name: &str) -> bool {
    !name.starts_with("__MACOSX")
        && !name.starts_with('$')
        && !name.contains(char::is_whitespace)
        && name
            .strip_suffix(".class")
            .is_some_and(|stem| !stem.is_empty())
}

/// Turn a jar's annotations into what the pre-flight reads: providers at FML's
/// version (steps 1–2) and the requirements FML takes from the annotation.
fn declarations(
    zip: &mut Zip<'_>,
    found: Vec<ModAnnotation>,
) -> Result<LegacyAnnotations, String> {
    let has_modid = |a: &ModAnnotation| a.modid.as_deref().is_some_and(|m| !m.is_empty());
    // Each file is read only if some annotation needs it: an unreadable file
    // nobody consults must not make the jar "could not tell".
    let properties = if found
        .iter()
        .any(|a| has_modid(a) && a.version.as_deref().is_none_or(str::is_empty))
    {
        read_capped(zip, "version.properties", MAX_PROPERTIES_BYTES)?.map(|b| latin1(&b))
    } else {
        None
    };
    let mcmod_rules = if found
        .iter()
        .any(|a| has_modid(a) && a.dependencies.as_deref().is_some_and(|d| !d.is_empty()))
    {
        read_capped(zip, "mcmod.info", MAX_MCMOD_INFO_BYTES)?
            .map(|b| uses_dependency_information(&String::from_utf8_lossy(&b)))
            .unwrap_or_default()
    } else {
        HashSet::new()
    };

    let mut out = LegacyAnnotations::default();
    for a in found {
        // FML builds no container for an annotation without a mod-id, so it
        // provides nothing and requires nothing.
        let Some(modid) = a.modid.filter(|m| !m.is_empty()) else {
            continue;
        };
        let version = match a.version.filter(|v| !v.is_empty()) {
            Some(v) => Some(v),
            None => match properties.as_deref() {
                Some(text) => properties_value(text, &format!("{modid}.version"))?,
                None => None,
            },
        };
        // `overridesMetadata || !useDependencyInformation` (FMLModContainer.java
        // :218): with the flag set in `mcmod.info` and no `useMetadata = false`,
        // FML takes this mod's requirements from `mcmod.info` instead. Dropping
        // the clauses can only miss a requirement, never invent one; reading
        // `requiredMods` is a follow-up.
        let fml_reads_mcmod_deps = a.use_metadata != Some(false) && mcmod_rules.contains(&modid);
        if !fml_reads_mcmod_deps {
            for dep in a
                .dependencies
                .as_deref()
                .map(parse_legacy_dependency_string)
                .unwrap_or_default()
            {
                if !out
                    .deps
                    .iter()
                    .any(|d| d.dep_id == dep.dep_id && d.range == dep.range)
                {
                    out.deps.push(dep);
                }
            }
        }
        out.provided.push(ProvidedMod {
            mod_id: modid,
            version,
            source: DescriptorSource::McmodAnnotation,
        });
    }
    Ok(out)
}

/// FML step 2: `Properties.getProperty(key)` over `version.properties`, for the
/// plain line shape real files use. `Ok(None)`: absent or empty — FML moves on
/// to `mcmod.info`. `Err`: the line defining `key` uses a backslash (an escape or
/// a continuation), which `Properties.load` would rewrite; that is not guessed at.
fn properties_value(text: &str, key: &str) -> Result<Option<String>, String> {
    let mut value = None;
    for raw in text.lines() {
        let line = raw.trim_start();
        if line.is_empty() || line.starts_with('#') || line.starts_with('!') {
            continue;
        }
        let end = line
            .find(|c: char| c == '=' || c == ':' || c.is_whitespace())
            .unwrap_or(line.len());
        let (k, rest) = line.split_at(end);
        if k != key {
            continue;
        }
        if line.contains('\\') {
            return Err(format!(
                "version.properties: the line defining {key} uses a backslash escape"
            ));
        }
        let rest = rest.trim_start();
        let v = rest.strip_prefix(['=', ':']).unwrap_or(rest).trim();
        // The last definition wins, as `Properties.load` overwrites.
        value = (!v.is_empty()).then(|| v.to_string());
    }
    Ok(value)
}

/// Mod-ids whose `mcmod.info` entry sets `useDependencyInformation`, keyed by
/// the exact mod-id `MetadataCollection.getMetadataForId` looks up. Gson reads a
/// boolean from a JSON `true` or from a string `Boolean.parseBoolean` accepts.
/// An unparseable file is FML's empty collection ("It will be ignored"): none.
fn uses_dependency_information(json_text: &str) -> HashSet<String> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json_text) else {
        return HashSet::new();
    };
    let Some(list) = mcmod_info_list(&v) else {
        return HashSet::new();
    };
    let flag = |f: &serde_json::Value| {
        f.as_bool()
            .unwrap_or_else(|| f.as_str().is_some_and(|s| s.eq_ignore_ascii_case("true")))
    };
    list.iter()
        .filter(|m| m.get("useDependencyInformation").is_some_and(flag))
        .filter_map(|m| m.get("modid").and_then(|x| x.as_str()).map(str::to_string))
        .collect()
}

/// ISO-8859-1, the encoding `Properties.load(InputStream)` reads.
fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| char::from(b)).collect()
}

/// A zip entry's bytes, capped. `Ok(None)`: no such entry. `Err`: present, but it
/// would not inflate or is larger than `cap`.
fn read_capped(zip: &mut Zip<'_>, name: &str, cap: u64) -> Result<Option<Vec<u8>>, String> {
    let entry = match zip.by_name(name) {
        Ok(entry) => entry,
        Err(zip::result::ZipError::FileNotFound) => return Ok(None),
        Err(e) => return Err(format!("{name}: {e}")),
    };
    if entry.size() > cap {
        return Err(format!(
            "{name} declares {} bytes, over the {cap}-byte cap",
            entry.size()
        ));
    }
    let mut buf = Vec::new();
    entry
        .take(cap + 1)
        .read_to_end(&mut buf)
        .map_err(|e| format!("{name}: {e}"))?;
    if buf.len() as u64 > cap {
        return Err(format!("{name} inflates past the {cap}-byte cap"));
    }
    Ok(Some(buf))
}

fn find_subslice(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || needle.len() > hay.len() {
        return None;
    }
    hay.windows(needle.len()).position(|w| w == needle)
}

// ── the class file (JVMS §4) ───────────────────────────────────────────────

/// Every class-level `@Mod` annotation of one class file. `None`: not a
/// readable class file. `Some(vec![])`: a class without `@Mod`.
pub(crate) fn mod_annotations(class: &[u8]) -> Option<Vec<ModAnnotation>> {
    let mut r = Reader::new(class);
    if r.u4()? != 0xCAFE_BABE {
        return None;
    }
    r.skip(4)?; // minor_version, major_version
    let pool = constant_pool(&mut r)?;
    r.skip(6)?; // access_flags, this_class, super_class
    let interfaces = usize::from(r.u2()?);
    r.skip(interfaces * 2)?;
    skip_members(&mut r)?; // fields
    skip_members(&mut r)?; // methods
    let mut out = Vec::new();
    for _ in 0..r.u2()? {
        let name = utf8(&pool, r.u2()?)?;
        let len = usize::try_from(r.u4()?).ok()?;
        let body = r.take(len)?;
        // FML's `ModClassVisitor.visitAnnotation(name, runtimeVisible)` ignores
        // the visibility flag, so both attributes count.
        if name != b"RuntimeVisibleAnnotations" && name != b"RuntimeInvisibleAnnotations" {
            continue;
        }
        let mut a = Reader::new(body);
        for _ in 0..a.u2()? {
            let (descriptor, pairs) = annotation(&mut a, &pool, 0)?;
            if descriptor == MOD_DESCRIPTOR || descriptor == CPW_MOD_DESCRIPTOR {
                out.push(mod_annotation(pairs));
            }
        }
    }
    Some(out)
}

fn mod_annotation(pairs: Vec<(&[u8], Value<'_>)>) -> ModAnnotation {
    let text = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    let mut m = ModAnnotation::default();
    for (name, value) in pairs {
        match (name, value) {
            (b"modid", Value::Str(s)) => m.modid = Some(text(s)),
            (b"version", Value::Str(s)) => m.version = Some(text(s)),
            (b"dependencies", Value::Str(s)) => m.dependencies = Some(text(s)),
            (b"useMetadata", Value::Bool(b)) => m.use_metadata = Some(b),
            _ => {}
        }
    }
    m
}

/// A cursor over a byte slice where every read is checked.
struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(n)?;
        let out = self.bytes.get(self.pos..end)?;
        self.pos = end;
        Some(out)
    }

    fn skip(&mut self, n: usize) -> Option<()> {
        self.take(n).map(|_| ())
    }

    fn u1(&mut self) -> Option<u8> {
        self.take(1)?.first().copied()
    }

    fn u2(&mut self) -> Option<u16> {
        let b: [u8; 2] = self.take(2)?.try_into().ok()?;
        Some(u16::from_be_bytes(b))
    }

    fn u4(&mut self) -> Option<u32> {
        let b: [u8; 4] = self.take(4)?.try_into().ok()?;
        Some(u32::from_be_bytes(b))
    }
}

/// The constants this reader resolves; every other tag is skipped by size.
#[derive(Debug, Clone, Copy)]
enum Constant<'a> {
    Utf8(&'a [u8]),
    Integer(i32),
    Other,
}

fn constant_pool<'a>(r: &mut Reader<'a>) -> Option<Vec<Constant<'a>>> {
    let count = usize::from(r.u2()?);
    // Index 0 is unused by the format; a placeholder keeps indices aligned.
    let mut pool = Vec::with_capacity(count);
    pool.push(Constant::Other);
    while pool.len() < count {
        match r.u1()? {
            1 => {
                let len = usize::from(r.u2()?);
                pool.push(Constant::Utf8(r.take(len)?));
            }
            3 => {
                let b: [u8; 4] = r.take(4)?.try_into().ok()?;
                pool.push(Constant::Integer(i32::from_be_bytes(b)));
            }
            4 => {
                r.skip(4)?;
                pool.push(Constant::Other);
            }
            // Long and Double take two slots (JVMS §4.4.5).
            5 | 6 => {
                r.skip(8)?;
                pool.push(Constant::Other);
                pool.push(Constant::Other);
            }
            7 | 8 | 16 | 19 | 20 => {
                r.skip(2)?;
                pool.push(Constant::Other);
            }
            9 | 10 | 11 | 12 | 17 | 18 => {
                r.skip(4)?;
                pool.push(Constant::Other);
            }
            15 => {
                r.skip(3)?;
                pool.push(Constant::Other);
            }
            _ => return None,
        }
    }
    Some(pool)
}

fn utf8<'a>(pool: &[Constant<'a>], index: u16) -> Option<&'a [u8]> {
    match pool.get(usize::from(index))? {
        Constant::Utf8(b) => Some(b),
        _ => None,
    }
}

fn integer(pool: &[Constant<'_>], index: u16) -> Option<i32> {
    match pool.get(usize::from(index))? {
        Constant::Integer(v) => Some(*v),
        _ => None,
    }
}

/// Fields and methods share one layout; only their attributes vary in size.
fn skip_members(r: &mut Reader<'_>) -> Option<()> {
    for _ in 0..r.u2()? {
        r.skip(6)?; // access_flags, name_index, descriptor_index
        for _ in 0..r.u2()? {
            r.skip(2)?; // attribute_name_index
            let len = usize::try_from(r.u4()?).ok()?;
            r.skip(len)?;
        }
    }
    Some(())
}

/// One element value, reduced to what `@Mod` needs.
enum Value<'a> {
    Str(&'a [u8]),
    Bool(bool),
    Other,
}

/// One annotation (JVMS §4.7.16): its type descriptor and `(name, value)` pairs.
type Annotation<'a> = (&'a [u8], Vec<(&'a [u8], Value<'a>)>);

fn annotation<'a>(
    r: &mut Reader<'a>,
    pool: &[Constant<'a>],
    depth: u32,
) -> Option<Annotation<'a>> {
    let descriptor = utf8(pool, r.u2()?)?;
    let mut pairs = Vec::new();
    for _ in 0..r.u2()? {
        let name = utf8(pool, r.u2()?)?;
        pairs.push((name, element_value(r, pool, depth + 1)?));
    }
    Some((descriptor, pairs))
}

/// JVMS §4.7.16.1: every tag is read or skipped structurally, so an element
/// `@Mod` does not need can never desynchronise the cursor.
fn element_value<'a>(
    r: &mut Reader<'a>,
    pool: &[Constant<'a>],
    depth: u32,
) -> Option<Value<'a>> {
    if depth > MAX_ELEMENT_DEPTH {
        return None;
    }
    match r.u1()? {
        b's' => Some(Value::Str(utf8(pool, r.u2()?)?)),
        b'Z' => Some(Value::Bool(integer(pool, r.u2()?)? != 0)),
        b'B' | b'C' | b'D' | b'F' | b'I' | b'J' | b'S' | b'c' => {
            r.skip(2)?;
            Some(Value::Other)
        }
        b'e' => {
            r.skip(4)?; // type_name_index, const_name_index
            Some(Value::Other)
        }
        b'@' => {
            annotation(r, pool, depth + 1)?;
            Some(Value::Other)
        }
        b'[' => {
            for _ in 0..r.u2()? {
                element_value(r, pool, depth + 1)?;
            }
            Some(Value::Other)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    macro_rules! fixture {
        ($path:literal) => {
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/legacy_mod/classes/",
                $path
            ))
            .as_slice()
        };
    }

    fn jar(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut w = zip::ZipWriter::new(Cursor::new(&mut buf));
            for (name, body) in entries {
                w.start_file(*name, SimpleFileOptions::default()).unwrap();
                w.write_all(body).unwrap();
            }
            w.finish().unwrap();
        }
        buf
    }

    fn one(class: &[u8]) -> ModAnnotation {
        let mut all = mod_annotations(class).expect("a javac-compiled class parses");
        assert_eq!(all.len(), 1, "{all:?}");
        all.remove(0)
    }

    #[test]
    fn reads_the_elements_fml_reads_from_a_compiled_class() {
        // OreLib.class also carries long and double constants, a field, an
        // `@Mod.EventHandler` method, and `@Marker` BEFORE `@Mod` whose values
        // use every other element tag (B C D F I J S Z e c [ @).
        let m = one(fixture!("fixture/OreLib.class"));
        assert_eq!(m.modid.as_deref(), Some("orelib"));
        assert_eq!(m.version.as_deref(), Some("3.6.0.1"));
        assert_eq!(
            m.dependencies.as_deref(),
            Some("required-after:forge@[14.23.5.2779,);")
        );
        assert_eq!(m.use_metadata, Some(true));
    }

    #[test]
    fn an_element_left_at_its_default_reads_as_absent() {
        // javac writes no element for a default, which is what FML's ASM
        // descriptor map sees too.
        let m = one(fixture!("fixture/BopNoVersion.class"));
        assert_eq!(m.modid.as_deref(), Some("biomesoplenty"));
        assert_eq!(m.version, None);
        assert_eq!(m.use_metadata, None);
        assert_eq!(
            one(fixture!("fixture/UdiDepExplicit.class")).use_metadata,
            Some(false)
        );
    }

    #[test]
    fn both_eras_and_both_retentions_are_read() {
        let old = one(fixture!("cpwfixture/OldMod.class"));
        assert_eq!(old.modid.as_deref(), Some("oldmod"));
        // CLASS retention lands in RuntimeInvisibleAnnotations, which FML's
        // `ModClassVisitor.visitAnnotation` reads too — it ignores the flag.
        let hidden = one(fixture!("invisible/InvisibleMod.class"));
        assert_eq!(hidden.modid.as_deref(), Some("invisiblemod"));
        assert_eq!(hidden.version.as_deref(), Some("2.0"));
    }

    #[test]
    fn a_class_that_only_mentions_the_syntax_is_not_a_mod() {
        // It carries the descriptor and `required-after:` as string constants
        // and an `@Mod.EventHandler` method: it passes the pre-filter, and the
        // structural read must still find no class-level `@Mod`.
        let bytes = fixture!("fixture/Mention.class");
        assert!(
            find_subslice(bytes, MOD_DESCRIPTOR).is_some(),
            "reaches the parser"
        );
        assert_eq!(mod_annotations(bytes), Some(vec![]));
    }

    #[test]
    fn malformed_bytes_are_none_never_a_panic() {
        let bytes = fixture!("fixture/OreLib.class");
        for len in 0..bytes.len() {
            assert_eq!(mod_annotations(&bytes[..len]), None, "truncated to {len}");
        }
        let mut bad_magic = bytes.to_vec();
        bad_magic[0] = 0;
        assert_eq!(mod_annotations(&bad_magic), None);
    }

    #[test]
    fn a_deeply_nested_element_value_is_refused_without_overflowing() {
        let pool = [
            Constant::Other,
            Constant::Utf8(b"Lx;".as_slice()),
            Constant::Utf8(b"v".as_slice()),
        ];
        // type #1, one pair named #2, then an array of an array of … 100k deep.
        let mut bytes = vec![0, 1, 0, 1, 0, 2];
        for _ in 0..100_000 {
            bytes.extend_from_slice(&[b'[', 0, 1]);
        }
        assert!(annotation(&mut Reader::new(&bytes), &pool, 0).is_none());
    }

    #[test]
    fn every_mod_class_of_a_jar_counts() {
        let j = jar(&[
            ("ex/A.class", fixture!("fixture/AtlasA.class")),
            ("ex/B.class", fixture!("fixture/AtlasB.class")),
            ("ex/Mention.class", fixture!("fixture/Mention.class")),
        ]);
        let out = read_jar_legacy_annotations(&j).expect("readable");
        let mut ids: Vec<(&str, Option<&str>)> = out
            .provided
            .iter()
            .map(|p| (p.mod_id.as_str(), p.version.as_deref()))
            .collect();
        ids.sort();
        assert_eq!(
            ids,
            vec![
                ("antiqueatlas", Some("4.6.3")),
                ("antiqueatlasoverlay", Some("1.2"))
            ]
        );
        assert!(out
            .provided
            .iter()
            .all(|p| p.source == DescriptorSource::McmodAnnotation));
        let deps: Vec<&str> = out.deps.iter().map(|d| d.dep_id.as_str()).collect();
        assert_eq!(deps, vec!["antiqueatlas"], "`after:forge` is load order only");
    }

    #[test]
    fn version_falls_back_to_version_properties_as_fml_does() {
        // The second line differs only in case: `getProperty` is case-sensitive.
        let props = b"# build\nbiomesoplenty.version=7.0.1.2445\nBiomesOPlenty.version=7.0.1\n";
        let j = jar(&[
            ("b/Bop.class", fixture!("fixture/BopNoVersion.class")),
            ("version.properties", props.as_slice()),
        ]);
        let out = read_jar_legacy_annotations(&j).unwrap();
        assert_eq!(out.provided[0].version.as_deref(), Some("7.0.1.2445"));
        // No file: nothing to say here — `mcmod.info` answers later, by rank.
        let bare = jar(&[("b/Bop.class", fixture!("fixture/BopNoVersion.class"))]);
        assert_eq!(
            read_jar_legacy_annotations(&bare).unwrap().provided[0].version,
            None
        );
    }

    #[test]
    fn version_properties_is_read_like_java_properties() {
        let k = "m.version";
        let some = |v: &str| Ok(Some(v.to_string()));
        assert_eq!(properties_value("m.version=1.0", k), some("1.0"));
        assert_eq!(properties_value("m.version: 1.0", k), some("1.0"));
        assert_eq!(properties_value("m.version 1.0", k), some("1.0"));
        assert_eq!(properties_value("  m.version = 1.0  ", k), some("1.0"));
        assert_eq!(
            properties_value("#m.version=1.0\n!m.version=2.0", k),
            Ok(None)
        );
        assert_eq!(
            properties_value("m.version=1.0\nm.version=2.0", k),
            some("2.0"),
            "the last definition wins"
        );
        assert_eq!(
            properties_value("M.version=1.0", k),
            Ok(None),
            "case-sensitive"
        );
        assert_eq!(
            properties_value("m.version=", k),
            Ok(None),
            "empty continues"
        );
        assert!(
            properties_value("m.version=1.\\\n  0", k).is_err(),
            "a continuation is not guessed at"
        );
        assert_eq!(
            properties_value("other=a\\b\nm.version=1.0", k),
            some("1.0"),
            "only the key's own line matters"
        );
    }

    #[test]
    fn a_needed_file_that_cannot_be_read_is_could_not_tell() {
        let escaped = b"biomesoplenty.version=7.0\\u0031".as_slice();
        let j = jar(&[
            ("b/Bop.class", fixture!("fixture/BopNoVersion.class")),
            ("version.properties", escaped),
        ]);
        assert_eq!(read_jar_legacy_annotations(&j), None);
        // A mod that names its own version never consults the file.
        let named = jar(&[
            ("o/OreLib.class", fixture!("fixture/OreLib.class")),
            ("version.properties", b"orelib.version=\\x".as_slice()),
        ]);
        assert!(read_jar_legacy_annotations(&named).is_some());
    }

    #[test]
    fn use_dependency_information_hands_the_requirements_to_mcmod_info() {
        let with = |info: &[u8], class: &[u8]| {
            read_jar_legacy_annotations(&jar(&[("u/U.class", class), ("mcmod.info", info)]))
                .expect("readable")
                .deps
                .len()
        };
        let udi = br#"[{"modid":"udidep","useDependencyInformation":true}]"#.as_slice();
        assert_eq!(
            with(udi, fixture!("fixture/UdiDep.class")),
            0,
            "mcmod.info rules"
        );
        assert_eq!(
            with(udi, fixture!("fixture/UdiDepExplicit.class")),
            1,
            "`useMetadata = false` forces the annotation"
        );
        let other_case = br#"[{"modid":"UdiDep","useDependencyInformation":true}]"#.as_slice();
        assert_eq!(
            with(other_case, fixture!("fixture/UdiDep.class")),
            1,
            "exact mod-id"
        );
        let stringly =
            br#"{"modList":[{"modid":"udidep","useDependencyInformation":"TRUE"}]}"#.as_slice();
        assert_eq!(
            with(stringly, fixture!("fixture/UdiDep.class")),
            0,
            "Gson's boolean"
        );
        assert_eq!(
            with(b"{ not json".as_slice(), fixture!("fixture/UdiDep.class")),
            1,
            "an unparseable mcmod.info is FML's empty collection"
        );
    }

    #[test]
    fn fml_discovery_filter() {
        // A broken candidate under __MACOSX is never scanned at all.
        let full = fixture!("fixture/EnhancedVisuals.class");
        let truncated = &full[..full.len() - 8];
        let j = jar(&[
            ("__MACOSX/ex/._A.class", truncated),
            ("ex/A.class", fixture!("fixture/AtlasA.class")),
        ]);
        assert_eq!(read_jar_legacy_annotations(&j).unwrap().provided.len(), 1);
        // Outside it, the same bytes make the jar "could not tell".
        assert_eq!(
            read_jar_legacy_annotations(&jar(&[("ev/EV.class", truncated)])),
            None
        );
        assert!(!fml_scans_entry("ex/With Space.class"));
        assert!(!fml_scans_entry("$Root.class"));
        assert!(fml_scans_entry("ex/Outer$Inner.class"));
        assert!(!fml_scans_entry("ex/readme.txt"));
        assert!(!fml_scans_entry(".class"));
    }

    #[test]
    fn reads_are_capped() {
        let j = jar(&[("big.txt", [b'a'; 100].as_slice())]);
        let mut z = zip::ZipArchive::new(Cursor::new(j.as_slice())).unwrap();
        assert!(read_capped(&mut z, "big.txt", 99).is_err());
        assert_eq!(
            read_capped(&mut z, "big.txt", 100).unwrap().map(|b| b.len()),
            Some(100)
        );
        assert_eq!(read_capped(&mut z, "absent", 100), Ok(None));
    }
}
