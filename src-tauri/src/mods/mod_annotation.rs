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
/// One `diag!` line names what failed. A coremod's plugin or mod container
/// that exists but cannot be read is "could not tell" too: it might be what
/// provides a required id ([`coremod_containers`]).
///
/// FML's jar-level exclusions (a coremod without `FMLCorePluginContainsFMLMod`,
/// `TweakClass`, `ModSide`, `ModType`) are deliberately NOT mirrored: Mixin's FML
/// agent re-admits some of those jars, and mirroring the exclusions without that
/// rule would delete real providers.
pub(crate) fn read_jar_legacy_annotations(jar_bytes: &[u8]) -> Option<LegacyAnnotations> {
    let mut zip = zip::ZipArchive::new(Cursor::new(jar_bytes)).ok()?;
    let read = scan_classes(&mut zip)
        .and_then(|found| declarations(&mut zip, found))
        .and_then(|mut out| {
            for container in coremod_containers(&mut zip)? {
                if !out.provided.iter().any(|p| p.mod_id == container.mod_id) {
                    out.provided.push(container);
                }
            }
            Ok(out)
        });
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
    // Read back by index: zip 9 finds a name by its raw bytes, which a CP437
    // name decoded to text no longer matches.
    let names: Vec<(usize, String)> = crate::zip_entry::indexed_names(zip)
        .filter(|(_, n)| fml_scans_entry(n))
        .map(|(i, n)| (i, n.into_owned()))
        .collect();
    let mut found = Vec::new();
    for (index, name) in &names {
        let entry = zip
            .by_index(*index)
            .map_err(|e| format!("{name} is listed but cannot be opened: {e}"))?;
        let bytes = read_entry_capped(entry, name, MAX_CLASS_BYTES)?;
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
fn declarations(zip: &mut Zip<'_>, found: Vec<ModAnnotation>) -> Result<LegacyAnnotations, String> {
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
        match read_capped(zip, "mcmod.info", MAX_MCMOD_INFO_BYTES)? {
            Some(b) => uses_dependency_information(&String::from_utf8_lossy(&b)),
            None => McmodDepsRule::Listed(HashSet::new()),
        }
    } else {
        McmodDepsRule::Listed(HashSet::new())
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
        let fml_reads_mcmod_deps = a.use_metadata != Some(false) && mcmod_rules.covers(&modid);
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

// ── coremod containers ─────────────────────────────────────────────────────

/// Largest `META-INF/MANIFEST.MF` read. Real ones are a few KiB of digests.
const MAX_MANIFEST_BYTES: u64 = 4 * 1024 * 1024;
/// Longest method code a class file may hold (JVMS §4.7.3: `code_length` < 65536).
const MAX_CODE_BYTES: usize = 65_535;

/// The `ModMetadata` classes a container assigns, 1.8–1.12.2 then ≤ 1.7.10.
const MOD_METADATA_CLASSES: [&[u8]; 2] = [
    b"net/minecraftforge/fml/common/ModMetadata",
    b"cpw/mods/fml/common/ModMetadata",
];

/// The mods a coremod jar registers WITHOUT `@Mod`. The manifest names a
/// loading plugin (`FMLCorePlugin`); FML calls its `getModContainerClass()` and
/// loads the class it returns (a `DummyModContainer` subclass) like any mod,
/// under the `ModMetadata.modId` that class assigns. TNTUtils' `@Mod` requires
/// `tnt_utilities_core`, which its own jar provides this way.
///
/// Read as FML runs it: the container is what `getModContainerClass()`'s code
/// loads — a string (`ldc "a.b.C"`) or a class literal (`C.class.getName()`) —
/// and its id is a String stored straight into `ModMetadata.modId`
/// (`md.modId = "x"`), its version the same into `ModMetadata.version`.
///
/// `Ok(vec![])` where FML would build no container we can name: no plugin
/// attribute, no plugin class, a method returning null, a choice between
/// several classes, or a container whose id depends on a branch. Those keep
/// the answer this pass did not change: a requirement on the id stays
/// missing. `Err`: a manifest, plugin or container that exists but cannot be
/// read — "could not tell", which the caller turns into the jar's own.
///
/// What this cannot see is FML skipping the coremod altogether (an
/// `@MCVersion` mismatch, a `TweakClass` jar without Mixin): the container's id
/// is then reported as provided although FML loads nothing.
fn coremod_containers(zip: &mut Zip<'_>) -> Result<Vec<ProvidedMod>, String> {
    let Some(manifest) = read_capped(zip, "META-INF/MANIFEST.MF", MAX_MANIFEST_BYTES)? else {
        return Ok(Vec::new());
    };
    let text = String::from_utf8_lossy(&manifest);
    let Some(plugin) = manifest_main_attribute(&text, "FMLCorePlugin") else {
        return Ok(Vec::new());
    };
    let plugin_entry = format!("{}.class", plugin.replace('.', "/"));
    // A plugin FML cannot find registers no container either.
    let Some(plugin_bytes) = read_capped(zip, &plugin_entry, MAX_CLASS_BYTES)? else {
        return Ok(Vec::new());
    };
    let plugin_class = class_methods(&plugin_bytes)
        .ok_or_else(|| format!("{plugin_entry} is not a readable class"))?;
    let named = plugin_class
        .container_class_names()
        .ok_or_else(|| format!("{plugin_entry}: getModContainerClass does not decode"))?;
    // None, or a choice between classes FML resolves at run time: nothing to name.
    let [container] = named.as_slice() else {
        return Ok(Vec::new());
    };
    let entry = format!("{container}.class");
    let Some(bytes) = read_capped(zip, &entry, MAX_CLASS_BYTES)? else {
        return Ok(Vec::new()); // FML fails to load it: no container
    };
    let class = class_methods(&bytes).ok_or_else(|| format!("{entry} is not a readable class"))?;
    let stores = class
        .metadata_stores()
        .ok_or_else(|| format!("{entry}: its code does not decode"))?;
    let [mod_id] = stores.mod_ids.as_slice() else {
        return Ok(Vec::new());
    };
    let version = match stores.versions.as_slice() {
        [v] if !v.is_empty() => Some(v.clone()),
        _ => None,
    };
    Ok(vec![ProvidedMod {
        mod_id: mod_id.clone(),
        version,
        source: DescriptorSource::CoremodContainer,
    }])
}

/// A main-section attribute of a JAR manifest (the part before the first blank
/// line), with continuation lines (a leading space) joined, as
/// `java.util.jar.Manifest` reads it. Names compare case-insensitively.
fn manifest_main_attribute(text: &str, name: &str) -> Option<String> {
    let mut logical: Vec<String> = Vec::new();
    for raw in text.lines() {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if line.is_empty() {
            break;
        }
        match (line.strip_prefix(' '), logical.last_mut()) {
            (Some(rest), Some(last)) => last.push_str(rest),
            _ => logical.push(line.to_string()),
        }
    }
    logical.iter().find_map(|l| {
        let (k, v) = l.split_once(':')?;
        k.trim()
            .eq_ignore_ascii_case(name)
            .then(|| v.trim().to_string())
            .filter(|v| !v.is_empty())
    })
}

/// One method's name, descriptor and code.
struct Method<'a> {
    name: &'a [u8],
    descriptor: &'a [u8],
    code: Option<Code<'a>>,
}

/// A `Code` attribute: the bytecode and its exception handlers' entry points,
/// which are jump targets like any branch's.
struct Code<'a> {
    bytes: &'a [u8],
    handlers: Vec<usize>,
}

/// A class file's constant pool and methods.
struct ClassMethods<'a> {
    pool: Vec<Constant<'a>>,
    methods: Vec<Method<'a>>,
}

/// `None`: not a readable class file.
fn class_methods(class: &[u8]) -> Option<ClassMethods<'_>> {
    let mut r = Reader::new(class);
    if r.u4()? != 0xCAFE_BABE {
        return None;
    }
    r.skip(4)?;
    let pool = constant_pool(&mut r)?;
    r.skip(6)?; // access_flags, this_class, super_class
    let interfaces = usize::from(r.u2()?);
    r.skip(interfaces * 2)?;
    skip_members(&mut r)?; // fields
    let mut methods = Vec::new();
    for _ in 0..r.u2()? {
        r.skip(2)?; // access_flags
        let name = utf8(&pool, r.u2()?)?;
        let descriptor = utf8(&pool, r.u2()?)?;
        let mut code = None;
        for _ in 0..r.u2()? {
            let attribute = utf8(&pool, r.u2()?)?;
            let len = usize::try_from(r.u4()?).ok()?;
            let body = r.take(len)?;
            if attribute == b"Code" {
                code = Some(code_attribute(body)?);
            }
        }
        methods.push(Method {
            name,
            descriptor,
            code,
        });
    }
    // The class's own attributes close the file: a class cut anywhere is
    // unreadable, as FML's class reader would find it.
    for _ in 0..r.u2()? {
        r.skip(2)?;
        let len = usize::try_from(r.u4()?).ok()?;
        r.skip(len)?;
    }
    Some(ClassMethods { pool, methods })
}

/// JVMS §4.7.3: max_stack, max_locals, the code, then the exception table.
fn code_attribute(body: &[u8]) -> Option<Code<'_>> {
    let mut c = Reader::new(body);
    c.skip(4)?;
    let len = usize::try_from(c.u4()?).ok()?;
    // JVMS §4.7.3: 1..=65535 bytes. Anything else is no class a JVM loads, and
    // a crafted multi-megabyte method would cost an allocation per instruction.
    if len == 0 || len > MAX_CODE_BYTES {
        return None;
    }
    let bytes = c.take(len)?;
    let mut handlers = Vec::new();
    for _ in 0..c.u2()? {
        c.skip(4)?; // start_pc, end_pc
        handlers.push(usize::from(c.u2()?));
        c.skip(2)?; // catch_type
    }
    Some(Code { bytes, handlers })
}

/// What a class stores straight into `ModMetadata`: distinct values, in code
/// order.
struct MetadataStores {
    mod_ids: Vec<String>,
    versions: Vec<String>,
}

impl<'a> ClassMethods<'a> {
    /// Internal names (`a/b/C`) of the classes `getModContainerClass()`'s code
    /// loads: `ldc` of a String shaped like a class name, or of a class (the
    /// literal behind `C.class.getName()`). `Some(vec![])` when the method is
    /// absent or loads none (`return null`). `None`: its code does not decode.
    fn container_class_names(&self) -> Option<Vec<String>> {
        let Some(method) = self
            .methods
            .iter()
            .find(|m| m.name == b"getModContainerClass" && m.descriptor == b"()Ljava/lang/String;")
        else {
            return Some(Vec::new());
        };
        let Some(code) = &method.code else {
            return Some(Vec::new()); // abstract or native: nothing to read
        };
        let mut out: Vec<String> = Vec::new();
        for insn in decode(code.bytes)? {
            let name = match self.loaded_constant(code.bytes, &insn) {
                Some(Constant::Str(i)) => {
                    let text = String::from_utf8_lossy(utf8(&self.pool, i)?).into_owned();
                    if !looks_like_class_name(&text) {
                        continue;
                    }
                    text.replace('.', "/")
                }
                Some(Constant::Class(i)) => {
                    String::from_utf8_lossy(utf8(&self.pool, i)?).into_owned()
                }
                _ => continue,
            };
            if !out.contains(&name) {
                out.push(name);
            }
        }
        Some(out)
    }

    /// Every String an `ldc`/`ldc_w` loads and the very next instruction stores
    /// into `ModMetadata.modId` or `.version`, in the class's constructors —
    /// javac's `md.modId = "x"`. A pair
    /// either of whose instructions is a jump target is skipped: its value
    /// depends on a branch (`md.modId = c ? "a" : "b"` ends in a jump onto the
    /// store). `None`: some method's code does not decode.
    fn metadata_stores(&self) -> Option<MetadataStores> {
        let mut out = MetadataStores {
            mod_ids: Vec::new(),
            versions: Vec::new(),
        };
        // The constructor: where a container assigns the metadata FML reads
        // (`super(new ModMetadata()); md.modId = …`). A store in another method
        // may never run, or fill a second `ModMetadata`.
        let constructors = self.methods.iter().filter(|m| m.name == b"<init>");
        for code in constructors.filter_map(|m| m.code.as_ref()) {
            let insns = decode(code.bytes)?;
            let mut targets: HashSet<usize> = code.handlers.iter().copied().collect();
            for insn in &insns {
                targets.extend(branch_targets(code.bytes, insn)?);
            }
            for pair in insns.windows(2) {
                let (load, store) = (&pair[0], &pair[1]);
                if store.op != 0xB5 || targets.contains(&load.pc) || targets.contains(&store.pc) {
                    continue;
                }
                let Some(Constant::Str(text)) = self.loaded_constant(code.bytes, load) else {
                    continue;
                };
                let Some((owner, field)) = field_ref(&self.pool, u2_at(code.bytes, store.pc + 1)?)
                else {
                    continue;
                };
                if !MOD_METADATA_CLASSES.contains(&owner) {
                    continue;
                }
                let list = match field {
                    b"modId" => &mut out.mod_ids,
                    b"version" => &mut out.versions,
                    _ => continue,
                };
                let value = String::from_utf8_lossy(utf8(&self.pool, text)?).into_owned();
                if !list.contains(&value) {
                    list.push(value);
                }
            }
        }
        Some(out)
    }

    /// The constant an `ldc` / `ldc_w` loads; `None` for any other instruction.
    fn loaded_constant(&self, code: &[u8], insn: &Insn) -> Option<Constant<'a>> {
        let index = match insn.op {
            0x12 => u16::from(*code.get(insn.pc + 1)?),
            0x13 => u2_at(code, insn.pc + 1)?,
            _ => return None,
        };
        self.pool.get(usize::from(index)).copied()
    }
}

/// `a.b.C`: dot-separated Java identifiers, at least one dot.
fn looks_like_class_name(text: &str) -> bool {
    text.contains('.')
        && text.split('.').all(|part| {
            let mut chars = part.chars();
            chars
                .next()
                .is_some_and(|c| c.is_alphabetic() || c == '_' || c == '$')
                && chars.all(|c| c.is_alphanumeric() || c == '_' || c == '$')
        })
}

/// A `CONSTANT_Fieldref`'s owning class (internal name) and field name.
fn field_ref<'a>(pool: &[Constant<'a>], index: u16) -> Option<(&'a [u8], &'a [u8])> {
    let Constant::FieldRef(class, nat) = *pool.get(usize::from(index))? else {
        return None;
    };
    let Constant::Class(class_name) = *pool.get(usize::from(class))? else {
        return None;
    };
    let Constant::NameAndType(name, _descriptor) = *pool.get(usize::from(nat))? else {
        return None;
    };
    Some((utf8(pool, class_name)?, utf8(pool, name)?))
}

/// One decoded instruction: where it starts and its opcode.
#[derive(Debug, Clone, Copy)]
struct Insn {
    pc: usize,
    op: u8,
}

/// Every instruction of a method's code, in order. `None`: an instruction does
/// not decode.
fn decode(code: &[u8]) -> Option<Vec<Insn>> {
    let mut out = Vec::new();
    let mut pc = 0;
    while pc < code.len() {
        let len = instruction_len(code, pc)?;
        out.push(Insn { pc, op: code[pc] });
        pc += len;
    }
    Some(out)
}

fn u2_at(code: &[u8], at: usize) -> Option<u16> {
    let b: [u8; 2] = code.get(at..at.checked_add(2)?)?.try_into().ok()?;
    Some(u16::from_be_bytes(b))
}

fn i32_at(code: &[u8], at: usize) -> Option<i32> {
    let b: [u8; 4] = code.get(at..at.checked_add(4)?)?.try_into().ok()?;
    Some(i32::from_be_bytes(b))
}

/// Where a branch instruction may jump (JVMS §6.5): its offsets are relative to
/// its own pc. Empty for any other instruction. `None`: a target outside the
/// address space.
fn branch_targets(code: &[u8], insn: &Insn) -> Option<Vec<usize>> {
    let at = |offset: i64| usize::try_from(i64::try_from(insn.pc).ok()? + offset).ok();
    let pc = insn.pc;
    let offsets: Vec<i64> = match insn.op {
        // if*, goto, jsr, ifnull, ifnonnull: a signed 16-bit offset.
        0x99..=0xA8 | 0xC6 | 0xC7 => vec![i64::from(u2_at(code, pc + 1)? as i16)],
        // goto_w, jsr_w.
        0xC8 | 0xC9 => vec![i64::from(i32_at(code, pc + 1)?)],
        0xAA => {
            let base = (pc + 4) & !3;
            let (low, high) = (i32_at(code, base + 4)?, i32_at(code, base + 8)?);
            let entries = usize::try_from(i64::from(high) - i64::from(low) + 1).ok()?;
            let mut v = vec![i64::from(i32_at(code, base)?)];
            for k in 0..entries {
                v.push(i64::from(i32_at(code, base + 12 + k * 4)?));
            }
            v
        }
        0xAB => {
            let base = (pc + 4) & !3;
            let pairs = usize::try_from(i32_at(code, base + 4)?).ok()?;
            let mut v = vec![i64::from(i32_at(code, base)?)];
            for k in 0..pairs {
                v.push(i64::from(i32_at(code, base + 8 + k * 8 + 4)?));
            }
            v
        }
        _ => Vec::new(),
    };
    offsets.into_iter().map(at).collect()
}

/// The length of the instruction at `pc` (JVMS §6.5). `None`: an opcode no
/// class file may contain, or one whose operands run past the code.
fn instruction_len(code: &[u8], pc: usize) -> Option<usize> {
    let op = *code.get(pc)?;
    // Switch operands start at the next multiple of four from the code start.
    let aligned = (pc + 4) & !3;
    let len = match op {
        0x00..=0x0F | 0x1A..=0x35 | 0x3B..=0x83 | 0x85..=0x98 | 0xAC..=0xB1 => 1,
        0xBE | 0xBF | 0xC2 | 0xC3 => 1,
        0x10 | 0x12 | 0x15..=0x19 | 0x36..=0x3A | 0xA9 | 0xBC => 2,
        0x11 | 0x13 | 0x14 | 0x84 | 0x99..=0xA8 | 0xB2..=0xB8 | 0xBB | 0xBD => 3,
        0xC0 | 0xC1 | 0xC6 | 0xC7 => 3,
        0xC5 => 4,
        0xB9 | 0xBA | 0xC8 | 0xC9 => 5,
        0xC4 => match *code.get(pc + 1)? {
            0x84 => 6, // wide iinc
            _ => 4,
        },
        0xAA => {
            let (low, high) = (i32_at(code, aligned + 4)?, i32_at(code, aligned + 8)?);
            let entries = usize::try_from(i64::from(high) - i64::from(low) + 1).ok()?;
            (aligned - pc) + 12 + entries.checked_mul(4)?
        }
        0xAB => {
            let pairs = usize::try_from(i32_at(code, aligned + 4)?).ok()?;
            (aligned - pc) + 8 + pairs.checked_mul(8)?
        }
        // 0xCA breakpoint, 0xFE/0xFF impdep and the unassigned rest: reserved,
        // never in a class file.
        _ => return None,
    };
    (pc.checked_add(len)? <= code.len()).then_some(len)
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

/// Which mods FML takes the requirements of from `mcmod.info` rather than from
/// the annotation (`useDependencyInformation`).
enum McmodDepsRule {
    /// Exactly these mod-ids, matched as `MetadataCollection.getMetadataForId`
    /// matches them: by exact mod-id.
    Listed(HashSet<String>),
    /// Could not tell: the file names the flag but our strict parser rejects
    /// it, while FML's Gson parser is lenient (`JsonParser.parse(Reader)`
    /// accepts comments, single quotes, unquoted names). Every mod of the jar is
    /// treated as setting it, which can only drop requirements, never invent one.
    Unknown,
}

impl McmodDepsRule {
    fn covers(&self, modid: &str) -> bool {
        match self {
            Self::Listed(ids) => ids.contains(modid),
            Self::Unknown => true,
        }
    }
}

/// Read `useDependencyInformation` out of an `mcmod.info`. Gson reads a boolean
/// from a JSON `true` or from a string `Boolean.parseBoolean` accepts. A file
/// our parser rejects is FML's empty collection ("It will be ignored") only if
/// Gson rejects it too, which cannot be known here — so it lists nobody when it
/// never names the flag, and is `Unknown` when it does.
fn uses_dependency_information(json_text: &str) -> McmodDepsRule {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json_text) else {
        return if json_text.contains("useDependencyInformation") {
            McmodDepsRule::Unknown
        } else {
            McmodDepsRule::Listed(HashSet::new())
        };
    };
    let Some(list) = mcmod_info_list(&v) else {
        return McmodDepsRule::Listed(HashSet::new());
    };
    let flag = |f: &serde_json::Value| {
        f.as_bool()
            .unwrap_or_else(|| f.as_str().is_some_and(|s| s.eq_ignore_ascii_case("true")))
    };
    McmodDepsRule::Listed(
        list.iter()
            .filter(|m| m.get("useDependencyInformation").is_some_and(flag))
            .filter_map(|m| m.get("modid").and_then(|x| x.as_str()).map(str::to_string))
            .collect(),
    )
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
    read_entry_capped(entry, name, cap).map(Some)
}

/// An open entry's bytes, capped: `Err` when it would not inflate or is larger
/// than `cap`. `name` is for the message.
fn read_entry_capped(
    entry: zip::read::ZipFile<'_, Cursor<&[u8]>>,
    name: &str,
    cap: u64,
) -> Result<Vec<u8>, String> {
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
    Ok(buf)
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
                // `@Mod` is not repeatable, so javac never writes a second one.
                // A class that carries more is crafted, and refusing it here —
                // before collecting — keeps a few bytes per empty annotation
                // from multiplying into an allocation that aborts the process.
                if !out.is_empty() {
                    return None;
                }
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
/// Indices are into the same pool, as the class file stores them.
#[derive(Debug, Clone, Copy)]
enum Constant<'a> {
    Utf8(&'a [u8]),
    Integer(i32),
    /// `CONSTANT_Class`: the index of its internal name (`a/b/C`).
    Class(u16),
    /// `CONSTANT_String`: the index of its text.
    Str(u16),
    /// `CONSTANT_Fieldref`: the owning class, then the name-and-type.
    FieldRef(u16, u16),
    /// `CONSTANT_NameAndType`: the name, then the descriptor.
    NameAndType(u16, u16),
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
            7 => pool.push(Constant::Class(r.u2()?)),
            8 => pool.push(Constant::Str(r.u2()?)),
            16 | 19 | 20 => {
                r.skip(2)?;
                pool.push(Constant::Other);
            }
            9 => pool.push(Constant::FieldRef(r.u2()?, r.u2()?)),
            12 => pool.push(Constant::NameAndType(r.u2()?, r.u2()?)),
            10 | 11 | 17 | 18 => {
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

fn annotation<'a>(r: &mut Reader<'a>, pool: &[Constant<'a>], depth: u32) -> Option<Annotation<'a>> {
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
fn element_value<'a>(r: &mut Reader<'a>, pool: &[Constant<'a>], depth: u32) -> Option<Value<'a>> {
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

    /// A class whose entry name is not UTF-8 (CP437, no UTF-8 flag) is read
    /// like any other. zip 9 finds a name by its raw bytes, so the scan reads
    /// a listed entry back by its index, never by its decoded name.
    #[test]
    fn a_class_named_in_cp437_is_read() {
        let mut j = jar(&[("ex/Xtlas.class", fixture!("fixture/AtlasA.class"))]);
        let raw = b"ex/Xtlas.class";
        let mut at = 0;
        let mut patched = 0;
        while let Some(i) = j[at..].windows(raw.len()).position(|w| w == raw) {
            j[at + i + 3] = 0x8E; // CP437 'Ä'; alone it is not UTF-8
            at += i + raw.len();
            patched += 1;
        }
        assert_eq!(patched, 2, "the local header and the central directory");
        let out = read_jar_legacy_annotations(&j).expect("readable");
        let ids: Vec<&str> = out.provided.iter().map(|p| p.mod_id.as_str()).collect();
        assert_eq!(ids, ["antiqueatlas"]);
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
        assert_eq!(
            deps,
            vec!["antiqueatlas"],
            "`after:forge` is load order only"
        );
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

        // Present but over its cap: not "absent", which would hand the version
        // to `mcmod.info`.
        let mut big = vec![b'#'; MAX_PROPERTIES_BYTES as usize];
        big.extend_from_slice(b"\nbiomesoplenty.version=7.0.1.2445\n");
        let huge_props = jar(&[
            ("b/Bop.class", fixture!("fixture/BopNoVersion.class")),
            ("version.properties", big.as_slice()),
        ]);
        assert_eq!(read_jar_legacy_annotations(&huge_props), None);

        // The same for `mcmod.info` when an annotation declares requirements:
        // it decides whether FML reads them at all.
        let huge_info = vec![b' '; MAX_MCMOD_INFO_BYTES as usize + 1];
        let j = jar(&[
            ("u/U.class", fixture!("fixture/UdiDep.class")),
            ("mcmod.info", huge_info.as_slice()),
        ]);
        assert_eq!(read_jar_legacy_annotations(&j), None);
    }

    /// `JsonParser.parse(Reader)` is lenient, so FML accepts an `mcmod.info` our
    /// strict parser rejects. When such a file names the flag, whether FML reads
    /// the annotation's clauses cannot be told — and the answer that can never
    /// invent a requirement is "it does not".
    #[test]
    fn an_mcmod_info_only_gson_accepts_cannot_hand_us_the_requirements() {
        let lenient =
            b"// generated\n[{\"modid\":\"udidep\",\"useDependencyInformation\":true}]".as_slice();
        let j = jar(&[
            ("u/U.class", fixture!("fixture/UdiDep.class")),
            ("mcmod.info", lenient),
        ]);
        assert!(read_jar_legacy_annotations(&j).unwrap().deps.is_empty());
    }

    /// A zip entry may declare a smaller size than it inflates to; the cap is
    /// enforced on the bytes read, not on the header's claim.
    #[test]
    fn a_lying_entry_size_does_not_slip_past_the_cap() {
        let mut j = jar(&[("big.txt", [b'a'; 200].as_slice())]);
        // Patch the uncompressed size (200) down to 10 in the local header
        // (offset 22) and the central directory header (offset 24).
        let patch = |bytes: &mut Vec<u8>, sig: &[u8; 4], offset: usize| {
            let at = find_subslice(bytes, sig).expect("header present") + offset;
            bytes[at..at + 4].copy_from_slice(&10u32.to_le_bytes());
        };
        patch(&mut j, b"PK\x03\x04", 22);
        patch(&mut j, b"PK\x01\x02", 24);
        let mut z = zip::ZipArchive::new(Cursor::new(j.as_slice())).unwrap();
        assert_eq!(
            z.by_name("big.txt").unwrap().size(),
            10,
            "the lie is believed"
        );
        assert!(read_capped(&mut z, "big.txt", 100).is_err());
    }

    /// `@Mod` is not repeatable; a class carrying two is crafted and is refused
    /// outright rather than collected. Hand-built, because javac cannot emit it.
    #[test]
    fn a_class_with_two_mod_annotations_is_refused() {
        let class = |annotations: u16| {
            let mut c = vec![0xCA, 0xFE, 0xBA, 0xBE, 0, 0, 0, 52, 0, 3];
            for s in [b"RuntimeVisibleAnnotations".as_slice(), MOD_DESCRIPTOR] {
                c.push(1);
                c.extend_from_slice(&(s.len() as u16).to_be_bytes());
                c.extend_from_slice(s);
            }
            // access, this, super, interfaces, fields, methods, one attribute.
            c.extend_from_slice(&[0, 0x21, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 1]);
            let body_len = 2 + 4 * u32::from(annotations);
            c.extend_from_slice(&body_len.to_be_bytes());
            c.extend_from_slice(&annotations.to_be_bytes());
            for _ in 0..annotations {
                c.extend_from_slice(&[0, 2, 0, 0]); // type #2, no pairs
            }
            c
        };
        assert_eq!(
            mod_annotations(&class(1)),
            Some(vec![ModAnnotation::default()])
        );
        assert_eq!(mod_annotations(&class(2)), None);
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
            with(
                b"not json at all".as_slice(),
                fixture!("fixture/UdiDep.class")
            ),
            1,
            "an unparseable mcmod.info that never names the flag is no rule"
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
            read_capped(&mut z, "big.txt", 100)
                .unwrap()
                .map(|b| b.len()),
            Some(100)
        );
        assert_eq!(read_capped(&mut z, "absent", 100), Ok(None));
    }

    // ── coremod containers ───────────────────────────────────────────────

    /// A manifest naming `plugin` as the jar's `FMLCorePlugin`.
    fn manifest(plugin: &str) -> Vec<u8> {
        format!(
            "Manifest-Version: 1.0\r\nFMLCorePlugin: {plugin}\r\n\
             FMLCorePluginContainsFMLMod: true\r\n\r\n"
        )
        .into_bytes()
    }

    /// TNTUtils' shape: the plugin names its container by a string, the
    /// container assigns `ModMetadata`, and the jar's `@Mod` requires that id.
    /// `DecoyContainer` is a container the plugin never names.
    fn tntutils_shaped(manifest_bytes: Option<&[u8]>, container: &[u8]) -> Vec<u8> {
        let mut entries: Vec<(&str, &[u8])> = vec![
            (
                "coremodfixture/CorePlugin.class",
                fixture!("coremodfixture/CorePlugin.class"),
            ),
            ("coremodfixture/CoreContainer.class", container),
            (
                "coremodfixture/CoreTransformer.class",
                fixture!("coremodfixture/CoreTransformer.class"),
            ),
            (
                "coremodfixture/DecoyContainer.class",
                fixture!("coremodfixture/DecoyContainer.class"),
            ),
            (
                "coremodfixture/NeedsCore.class",
                fixture!("coremodfixture/NeedsCore.class"),
            ),
        ];
        if let Some(m) = manifest_bytes {
            entries.push(("META-INF/MANIFEST.MF", m));
        }
        jar(&entries)
    }

    fn provided(out: &LegacyAnnotations) -> Vec<(&str, Option<&str>)> {
        out.provided
            .iter()
            .map(|p| (p.mod_id.as_str(), p.version.as_deref()))
            .collect()
    }

    #[test]
    fn a_coremod_container_provides_the_id_its_jar_requires() {
        let m = manifest("coremodfixture.CorePlugin");
        let j = tntutils_shaped(Some(&m), fixture!("coremodfixture/CoreContainer.class"));
        let out = read_jar_legacy_annotations(&j).expect("readable");
        // `decoy_core` is in the jar too, but FML never builds a container the
        // plugin does not name.
        assert_eq!(
            provided(&out),
            [("needs_core", Some("1.0")), ("test_core", Some("1.2.3"))]
        );
        let sources: Vec<DescriptorSource> = out.provided.iter().map(|p| p.source).collect();
        assert_eq!(
            sources,
            [
                DescriptorSource::McmodAnnotation,
                DescriptorSource::CoremodContainer
            ]
        );
        assert!(
            out.deps.iter().any(|d| d.dep_id == "test_core"),
            "{:?}",
            out.deps
        );
    }

    #[test]
    fn without_fml_core_plugin_no_container_is_read() {
        let container = fixture!("coremodfixture/CoreContainer.class");
        let out = read_jar_legacy_annotations(&tntutils_shaped(None, container)).unwrap();
        assert_eq!(provided(&out), [("needs_core", Some("1.0"))]);
        let other = b"Manifest-Version: 1.0\r\nMain-Class: x.Y\r\n\r\n";
        let out = read_jar_legacy_annotations(&tntutils_shaped(Some(other), container)).unwrap();
        assert_eq!(provided(&out), [("needs_core", Some("1.0"))]);
    }

    #[test]
    fn a_container_named_by_a_class_literal_behind_switches_is_read() {
        let m = manifest("coremodfixture.SwitchPlugin");
        let j = jar(&[
            ("META-INF/MANIFEST.MF", &m),
            (
                "coremodfixture/SwitchPlugin.class",
                fixture!("coremodfixture/SwitchPlugin.class"),
            ),
            (
                "coremodfixture/SwitchContainer.class",
                fixture!("coremodfixture/SwitchContainer.class"),
            ),
        ]);
        let out = read_jar_legacy_annotations(&j).expect("readable");
        assert_eq!(provided(&out), [("switch_core", None)]);
    }

    #[test]
    fn a_1_7_10_container_is_read() {
        let m = manifest("cpwcoremodfixture.OldCorePlugin");
        let j = jar(&[
            ("META-INF/MANIFEST.MF", &m),
            (
                "cpwcoremodfixture/OldCorePlugin.class",
                fixture!("cpwcoremodfixture/OldCorePlugin.class"),
            ),
            (
                "cpwcoremodfixture/OldCoreContainer.class",
                fixture!("cpwcoremodfixture/OldCoreContainer.class"),
            ),
        ]);
        let out = read_jar_legacy_annotations(&j).expect("readable");
        assert_eq!(provided(&out), [("old_core", Some("0.9"))]);
    }

    #[test]
    fn an_unreadable_container_is_could_not_tell() {
        // It might have provided the id: the jar's answer is "could not tell",
        // which is never cached, not a measured "provides nothing".
        let m = manifest("coremodfixture.CorePlugin");
        let whole = fixture!("coremodfixture/CoreContainer.class");
        for cut in 0..whole.len() {
            let j = tntutils_shaped(Some(&m), &whole[..cut]);
            assert_eq!(read_jar_legacy_annotations(&j), None, "cut at {cut}");
        }
    }

    #[test]
    fn a_plugin_returning_null_registers_no_container() {
        // The plugin mentions CoreContainer's name elsewhere; only what
        // getModContainerClass() loads counts.
        let m = manifest("coremodfixture.NullPlugin");
        let j = jar(&[
            ("META-INF/MANIFEST.MF", &m),
            (
                "coremodfixture/NullPlugin.class",
                fixture!("coremodfixture/NullPlugin.class"),
            ),
            (
                "coremodfixture/CoreContainer.class",
                fixture!("coremodfixture/CoreContainer.class"),
            ),
        ]);
        assert_eq!(provided(&read_jar_legacy_annotations(&j).unwrap()), []);
    }

    #[test]
    fn an_id_chosen_by_a_branch_is_not_reported() {
        // `modId = legacy ? "ternary_old" : "ternary_new"`: javac leaves only the
        // second string beside the store, and both sit on jump targets.
        let m = manifest("coremodfixture.TernaryPlugin");
        let j = jar(&[
            ("META-INF/MANIFEST.MF", &m),
            (
                "coremodfixture/TernaryPlugin.class",
                fixture!("coremodfixture/TernaryPlugin.class"),
            ),
            (
                "coremodfixture/TernaryContainer.class",
                fixture!("coremodfixture/TernaryContainer.class"),
            ),
        ]);
        assert_eq!(provided(&read_jar_legacy_annotations(&j).unwrap()), []);
    }

    #[test]
    fn a_manifest_attribute_is_read_across_continuation_lines() {
        let text = "Manifest-Version: 1.0\r\nFMLCorePlugin: coremodfixture.Core\r\n Plugin\r\n\
                    \r\nName: x\r\nFMLCorePlugin: no.Section\r\n";
        assert_eq!(
            manifest_main_attribute(text, "fmlcoreplugin").as_deref(),
            Some("coremodfixture.CorePlugin")
        );
        // An attribute only in a per-entry section is not the jar's.
        let per_entry = "Manifest-Version: 1.0\n\nName: x\nFMLCorePlugin: a.B\n";
        assert_eq!(manifest_main_attribute(per_entry, "FMLCorePlugin"), None);
    }

    #[test]
    fn instruction_lengths_cover_wide_and_refuse_reserved_opcodes() {
        assert_eq!(instruction_len(&[0xC4, 0x84, 0, 1, 0, 1], 0), Some(6)); // wide iinc
        assert_eq!(instruction_len(&[0xC4, 0x15, 1, 0], 0), Some(4)); // wide iload
        assert_eq!(instruction_len(&[0xCA], 0), None); // breakpoint
        assert_eq!(instruction_len(&[0xFF], 0), None); // impdep2
        assert_eq!(instruction_len(&[0x13, 0], 0), None); // ldc_w cut short
                                                          // lookupswitch at pc 1: two padding bytes, default, one pair.
        let mut code = vec![0x00, 0xAB, 0, 0];
        code.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 7, 0, 0, 0, 9]);
        assert_eq!(instruction_len(&code, 1), Some(code.len() - 1));
        // A table claiming more entries than the code holds is refused.
        let mut huge = vec![0xAA, 0, 0, 0];
        huge.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0, 0x7F, 0xFF, 0xFF, 0xFF]);
        assert_eq!(instruction_len(&huge, 0), None);
    }
}
