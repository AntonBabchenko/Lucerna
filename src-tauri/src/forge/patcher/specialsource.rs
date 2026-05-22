//! SpecialSource — bytecode remapper invoked as a Java subprocess.
//!
//! Forge's transitional-era (1.13-1.16) install pipeline runs
//! SpecialSource to remap the slim vanilla MC jar from obfuscated
//! names to SRG names. Forge's subsequent BinaryPatcher (GDiff)
//! patches reference byte offsets in this SRG output, so the
//! remapper output must be byte-identical to what Java SpecialSource
//! 1.8.5/1.11.0 produces.
//!
//! Pure-Rust byte-fidelity proved impractical (see
//! `docs/superpowers/specs/2026-05-16-forge-loader-design.md#addendum-2026-05-17-b--specialsource-shell-out`),
//! so we shell out to the canonical Java implementation. The processor
//! JAR + classpath dependencies are passed to `java -cp ... <main>`.
//!
//! The pure-Rust `remap_class_bytes` / `remap_descriptor` / `ObfIndex`
//! infrastructure below is retained as `#[allow(dead_code)]` for
//! reference and potential future revival.

use crate::error::Result;
use crate::forge::mappings::ObfIndex;
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
            other => return Err(patcher_fail("specialsource", &format!("unknown flag: {other}"))),
        }
    }
    if live { return Err(patcher_fail("specialsource", &"--live not supported")); }
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
    ctx.java_bin.clone().unwrap_or_else(|| PathBuf::from("java"))
}

// ---------------------------------------------------------------------------
// Pure-Rust bytecode walker — retained as dead code for reference.
//
// The implementation below (Tasks 12-13) correctly handles the JVM classfile
// constant-pool structure but cannot produce byte-identical output to Java
// SpecialSource 1.11.0 due to differences in pool ordering and entry sizes.
// Forge's binarypatcher encodes byte-offset COPY commands against Java's
// output, so any divergence breaks the install.
//
// Retained here (rather than deleted) because:
//   a) It documents the classfile-format investigation work done in Phase 2α.
//   b) It can be revived if someone produces a byte-faithful port later.
//   c) The `remap_descriptor` helper is correct and may be reused in other
//      contexts (e.g. descriptor-only validation, FART research).
//
// To revive: remove the `#[allow(dead_code)]` markers, wire remap_class_bytes
// back into a pure-Rust run() that zips the jar, add the golden-fixture tests
// from git history, and run against Java SpecialSource to compare output.
// ---------------------------------------------------------------------------

/// Remap a JVM field/method descriptor string, replacing `L<internal/name>;`
/// tokens whose class names appear in `idx`.
#[allow(dead_code)]
pub fn remap_descriptor(d: &str, idx: &ObfIndex) -> String {
    let mut out = String::with_capacity(d.len());
    let bytes = d.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'L' {
            let end = match bytes[i..].iter().position(|&b| b == b';') {
                Some(p) => i + p,
                None => { out.push_str(&d[i..]); return out; }
            };
            let inner = &d[i + 1..end];
            let mapped = idx.lookup_class(inner).unwrap_or_else(|| inner.to_string());
            out.push('L');
            out.push_str(&mapped);
            out.push(';');
            i = end + 1;
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    out
}

/// Remap one classfile's bytes — class names, method names, field names, and
/// descriptors in the constant pool. Returns rewritten bytes; on parse
/// failure, passes through unchanged (logs warning).
///
/// # Approach chosen: multi-pass binary constant-pool walk (Option II — unified
/// slot-based substitution)
///
/// We do NOT use noak's `ClassWriter` for the round-trip because the writer
/// allocates a fresh pool from scratch. This means pool indices in the body
/// of the file (access flags, this_class, super_class, fields, methods,
/// attributes — all of which reference the pool by 1-based u16 index) would
/// be invalidated if the new pool orders entries differently.
///
/// ## Why slot-based and not content-based for class/member names?
///
/// Task 12 used content-based substitution for class names: any Utf8 entry
/// whose string content matched a class name in the index was rewritten.
/// That approach cannot extend to method/field names: two unrelated classes
/// can each declare a method named `run()V` that maps to DIFFERENT names.
/// The lookup key is `(owner_class, name, descriptor)`, not just `name`.
///
/// Switching to a fully slot-based approach also fixes the content-based
/// corner case documented in the original comment (string literals that
/// happen to match an obfuscated class name would have been falsely renamed).
///
/// ## Two-pass algorithm
///
/// **Pass 1 — index**: Walk the raw pool bytes once, recording for each slot:
///   - CONSTANT_Utf8 (tag 1): byte range of its content inside `input`
///   - CONSTANT_Class (tag 7): name_index (u16)
///   - CONSTANT_NameAndType (tag 12): name_index, descriptor_index
///   - CONSTANT_Fieldref/Methodref/InterfaceMethodref (tags 9/10/11):
///     class_index, name_and_type_index
///
/// **Pass 2 — build substitution table**: Walk the index entries; for each
/// ref entry (9/10/11), follow pointer chains to obtain the owner FQN (from
/// the ORIGINAL, pre-rewrite bytes), the member name, and the descriptor;
/// call `idx.lookup_method` / `idx.lookup_field`; if matched, record
/// `name_utf8_slot → new_name`. For each CONSTANT_Class entry, follow
/// name_index → Utf8; call `idx.lookup_class`; if matched, record
/// `name_utf8_slot → new_class_fqn`.
///
/// **Emission pass**: Walk the pool a third time (raw bytes); for each
/// CONSTANT_Utf8 slot, check the substitution table; if present, emit the
/// substituted bytes (with recomputed length prefix). Otherwise, still apply
/// `remap_descriptor` for descriptor-shaped strings (content-based, safe
/// because descriptor class-token remapping is not name-colliding). All other
/// entry types are copied verbatim.
///
/// Pool slot count and ordering are preserved, so all body references remain
/// valid.
///
/// The noak reader is still used for the initial validity check: if
/// `Class::new` fails we return the input unchanged.
#[allow(dead_code)]
pub fn remap_class_bytes(input: &[u8], idx: &ObfIndex) -> Vec<u8> {
    use noak::reader::Class;
    let class = match Class::new(input) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("specialsource: skip unparseable class ({e}) — passing through");
            return input.to_vec();
        }
    };
    let _ = class;

    // Fast path: if the index has nothing mapped, nothing to remap.
    if idx.class_count() == 0 && idx.field_count() == 0 && idx.method_count() == 0 {
        return input.to_vec();
    }

    // Classfile layout:
    //   magic(4) minor(2) major(2) pool_count(2) pool_items... <rest>
    // pool_count is 1-based: actual item count = pool_count - 1.
    // Long/Double entries consume two slots (second slot is implicit phantom).

    if input.len() < 10 {
        return input.to_vec();
    }

    let pool_count = u16::from_be_bytes([input[8], input[9]]) as usize;
    if pool_count == 0 {
        return input.to_vec();
    }
    let item_count = pool_count - 1;

    // -----------------------------------------------------------------------
    // Pass 1: build a lightweight index of every pool slot.
    // Slots are 1-based in the classfile; we store them 0-based (slot 1 → index 0).
    // -----------------------------------------------------------------------

    /// Lightweight descriptor of one pool entry, sufficient for pointer chasing.
    #[derive(Clone)]
    enum PoolSlot {
        /// CONSTANT_Utf8 — byte range of the UTF-8 content inside `input`.
        Utf8(std::ops::Range<usize>),
        /// CONSTANT_Class — index of the name Utf8 slot (1-based).
        Class { name_index: u16 },
        /// CONSTANT_NameAndType — name and descriptor Utf8 slot indices (1-based).
        NameAndType { name_index: u16, descriptor_index: u16 },
        /// CONSTANT_Fieldref / Methodref / InterfaceMethodref (tags 9/10/11).
        Ref { tag: u8, class_index: u16, nat_index: u16 },
        /// Long / Double high word (occupies two slots; next slot is Phantom).
        LongDoubleHigh,
        /// Phantom second slot of Long / Double.
        Phantom,
        /// Everything else (Integer, Float, String, MethodHandle, …).
        Other,
    }

    let mut slots: Vec<PoolSlot> = Vec::with_capacity(item_count);
    // body_start_pos: byte offset of the classfile body (access_flags onward),
    // i.e. the first byte after the constant pool.
    let body_start_pos: usize;
    {
        let mut pos = 10usize;
        let mut count = 0usize;
        while count < item_count {
            if pos >= input.len() {
                return input.to_vec();
            }
            let tag = input[pos];
            let entry = match tag {
                1 => {
                    if pos + 3 > input.len() { return input.to_vec(); }
                    let len = u16::from_be_bytes([input[pos + 1], input[pos + 2]]) as usize;
                    if pos + 3 + len > input.len() { return input.to_vec(); }
                    let range = pos + 3..pos + 3 + len;
                    pos += 3 + len;
                    count += 1;
                    PoolSlot::Utf8(range)
                }
                3 | 4 => {
                    if pos + 5 > input.len() { return input.to_vec(); }
                    pos += 5;
                    count += 1;
                    PoolSlot::Other
                }
                5 | 6 => {
                    // Long / Double: two slots, only one physical entry.
                    if pos + 9 > input.len() { return input.to_vec(); }
                    pos += 9;
                    count += 2;
                    slots.push(PoolSlot::LongDoubleHigh);
                    slots.push(PoolSlot::Phantom);
                    continue;
                }
                7 => {
                    if pos + 3 > input.len() { return input.to_vec(); }
                    let name_index = u16::from_be_bytes([input[pos + 1], input[pos + 2]]);
                    pos += 3;
                    count += 1;
                    PoolSlot::Class { name_index }
                }
                8 | 16 | 19 | 20 => {
                    if pos + 3 > input.len() { return input.to_vec(); }
                    pos += 3;
                    count += 1;
                    PoolSlot::Other
                }
                9 | 10 | 11 => {
                    if pos + 5 > input.len() { return input.to_vec(); }
                    let class_index = u16::from_be_bytes([input[pos + 1], input[pos + 2]]);
                    let nat_index   = u16::from_be_bytes([input[pos + 3], input[pos + 4]]);
                    pos += 5;
                    count += 1;
                    PoolSlot::Ref { tag, class_index, nat_index }
                }
                12 => {
                    if pos + 5 > input.len() { return input.to_vec(); }
                    let name_index       = u16::from_be_bytes([input[pos + 1], input[pos + 2]]);
                    let descriptor_index = u16::from_be_bytes([input[pos + 3], input[pos + 4]]);
                    pos += 5;
                    count += 1;
                    PoolSlot::NameAndType { name_index, descriptor_index }
                }
                15 => {
                    if pos + 4 > input.len() { return input.to_vec(); }
                    pos += 4;
                    count += 1;
                    PoolSlot::Other
                }
                17 | 18 => {
                    if pos + 5 > input.len() { return input.to_vec(); }
                    pos += 5;
                    count += 1;
                    PoolSlot::Other
                }
                _ => {
                    eprintln!("specialsource: unknown pool tag {tag} at byte {pos} — passing through");
                    return input.to_vec();
                }
            };
            slots.push(entry);
        }
        body_start_pos = pos;
    }

    // Helper: read a Utf8 slot as &str (slot is 1-based).
    let utf8_str = |slot_1based: u16| -> Option<&str> {
        let i = (slot_1based as usize).checked_sub(1)?;
        match slots.get(i) {
            Some(PoolSlot::Utf8(range)) => std::str::from_utf8(&input[range.clone()]).ok(),
            _ => None,
        }
    };

    // -----------------------------------------------------------------------
    // Pass 2: build substitution table  slot_1based → new_content (String).
    // -----------------------------------------------------------------------

    // Strategy: Option II (unified slot-based).
    // We populate this table for:
    //   • class-name Utf8 slots   (via CONSTANT_Class → name_index)
    //   • member-name Utf8 slots  (via Methodref/Fieldref → NameAndType → name_index)
    // Descriptor slots are NOT put in this table; they are handled content-based
    // in the emission pass (remap_descriptor is safe for descriptors because class
    // tokens inside descriptors don't have the "same-name-different-owner" problem).

    let mut subst: std::collections::HashMap<u16, String> = std::collections::HashMap::new();

    for entry in &slots {
        match entry {
            PoolSlot::Class { name_index } => {
                if let Some(class_fqn) = utf8_str(*name_index) {
                    if let Some(mapped) = idx.lookup_class(class_fqn) {
                        if mapped != class_fqn {
                            subst.insert(*name_index, mapped);
                        }
                    }
                }
            }
            PoolSlot::Ref { tag, class_index, nat_index } => {
                // Follow: Ref → Class → Utf8 (owner FQN, from ORIGINAL bytes)
                //         Ref → NameAndType → name Utf8 + descriptor Utf8
                // Indices are 1-based; subtract 1 to index into our 0-based vec.
                // Index 0 is invalid in the classfile spec, so checked_sub handles it.
                let owner_fqn = (*class_index as usize).checked_sub(1)
                    .and_then(|i| slots.get(i))
                    .and_then(|e| match e {
                        PoolSlot::Class { name_index: ni } => utf8_str(*ni),
                        _ => None,
                    });
                let (name_slot, name_str, desc_str) = (*nat_index as usize).checked_sub(1)
                    .and_then(|i| slots.get(i))
                    .map(|e| match e {
                        PoolSlot::NameAndType { name_index: ni, descriptor_index: di } => {
                            (Some(*ni), utf8_str(*ni), utf8_str(*di))
                        }
                        _ => (None, None, None),
                    })
                    .unwrap_or((None, None, None));

                if let (Some(owner), Some(name_slot), Some(name), Some(desc)) =
                    (owner_fqn, name_slot, name_str, desc_str)
                {
                    // Never remap constructors / static initialisers.
                    if name == "<init>" || name == "<clinit>" {
                        continue;
                    }
                    let mapped_opt = match *tag {
                        9 => idx.lookup_field(owner, name),
                        10 | 11 => idx.lookup_method(owner, name, desc),
                        _ => None,
                    };
                    if let Some(mapped) = mapped_opt {
                        if mapped != name {
                            subst.insert(name_slot, mapped);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // -----------------------------------------------------------------------
    // Pass 2b: scan declared fields and methods in the class body.
    //
    // Pool-based Ref entries only cover members that are REFERENCED (called or
    // accessed) inside this class. Members DECLARED in this class appear in the
    // fields[] / methods[] tables of the class body and use direct name_index /
    // descriptor_index pool references — no Ref entry is needed for a method the
    // class merely declares but never calls internally.
    //
    // Body layout (starting at body_start_pos):
    //   access_flags(2)  this_class(2)  super_class(2)
    //   interfaces_count(2)  [interface_index(2)]...
    //   fields_count(2)   [field_info...]
    //   methods_count(2)  [method_info...]
    //   attributes_count(2) [attribute_info...]
    //
    // field_info / method_info:
    //   access_flags(2)  name_index(2)  descriptor_index(2)
    //   attributes_count(2)  [attribute_info...]
    //
    // attribute_info:
    //   attribute_name_index(2)  attribute_length(4)  info[attribute_length]
    // -----------------------------------------------------------------------
    {
        // Helper: skip one attribute_info.
        let skip_attrs = |p: usize, n_attrs: u16, buf: &[u8]| -> Option<usize> {
            let mut pos = p;
            for _ in 0..n_attrs {
                if pos + 6 > buf.len() { return None; }
                let attr_len = u32::from_be_bytes([buf[pos+2], buf[pos+3], buf[pos+4], buf[pos+5]]) as usize;
                pos += 6 + attr_len;
            }
            Some(pos)
        };

        let mut bpos = body_start_pos;
        // Need at least: access(2) + this(2) + super(2) + ifaces_count(2) = 8 bytes.
        if bpos + 8 <= input.len() {
            let this_class_idx = u16::from_be_bytes([input[bpos + 2], input[bpos + 3]]);
            let this_fqn: Option<&str> = (this_class_idx as usize).checked_sub(1)
                .and_then(|i| slots.get(i))
                .and_then(|e| match e {
                    PoolSlot::Class { name_index: ni } => utf8_str(*ni),
                    _ => None,
                });

            let ifaces_count = u16::from_be_bytes([input[bpos + 6], input[bpos + 7]]) as usize;
            bpos += 8 + ifaces_count * 2;

            // Walk fields_count field_info entries.
            if let Some(fqn) = this_fqn {
                if bpos + 2 <= input.len() {
                    let fields_count = u16::from_be_bytes([input[bpos], input[bpos + 1]]);
                    bpos += 2;
                    for _ in 0..fields_count {
                        if bpos + 8 > input.len() { break; }
                        let name_idx = u16::from_be_bytes([input[bpos + 2], input[bpos + 3]]);
                        let n_attrs  = u16::from_be_bytes([input[bpos + 6], input[bpos + 7]]);
                        bpos += 8;
                        // Lookup field remap: (this_fqn, field_name) → mapped.
                        if let Some(name) = utf8_str(name_idx) {
                            if name != "<init>" && name != "<clinit>" {
                                if let Some(mapped) = idx.lookup_field(fqn, name) {
                                    if mapped != name {
                                        subst.insert(name_idx, mapped);
                                    }
                                }
                            }
                        }
                        match skip_attrs(bpos, n_attrs, input) {
                            Some(next) => bpos = next,
                            None => break,
                        }
                    }

                    // Walk methods_count method_info entries.
                    if bpos + 2 <= input.len() {
                        let methods_count = u16::from_be_bytes([input[bpos], input[bpos + 1]]);
                        bpos += 2;
                        for _ in 0..methods_count {
                            if bpos + 8 > input.len() { break; }
                            let name_idx = u16::from_be_bytes([input[bpos + 2], input[bpos + 3]]);
                            let desc_idx = u16::from_be_bytes([input[bpos + 4], input[bpos + 5]]);
                            let n_attrs  = u16::from_be_bytes([input[bpos + 6], input[bpos + 7]]);
                            bpos += 8;
                            // Lookup method remap: (this_fqn, method_name, desc) → mapped.
                            if let (Some(name), Some(desc)) = (utf8_str(name_idx), utf8_str(desc_idx)) {
                                if name != "<init>" && name != "<clinit>" {
                                    if let Some(mapped) = idx.lookup_method(fqn, name, desc) {
                                        if mapped != name {
                                            subst.insert(name_idx, mapped);
                                        }
                                    }
                                }
                            }
                            match skip_attrs(bpos, n_attrs, input) {
                                Some(next) => bpos = next,
                                None => break,
                            }
                        }
                    }
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Emission pass: walk raw bytes a second time, emit transformed pool.
    // -----------------------------------------------------------------------

    let mut out: Vec<u8> = Vec::with_capacity(input.len());
    // Copy magic + version + pool_count header (10 bytes) verbatim.
    out.extend_from_slice(&input[..10]);

    let mut pos = 10usize;
    let mut slot = 0usize; // 0-based, maps to 1-based slot via slot+1

    while slot < item_count {
        if pos >= input.len() {
            return input.to_vec();
        }
        let tag = input[pos];
        match tag {
            1 => {
                // CONSTANT_Utf8: tag(1) length(2) bytes(length)
                if pos + 3 > input.len() { return input.to_vec(); }
                let orig_len = u16::from_be_bytes([input[pos + 1], input[pos + 2]]) as usize;
                if pos + 3 + orig_len > input.len() { return input.to_vec(); }
                let content = &input[pos + 3..pos + 3 + orig_len];

                // slot+1 is the 1-based pool index for this entry.
                let slot_1based = (slot + 1) as u16;

                let rewritten: Option<Vec<u8>> = if let Some(new_str) = subst.get(&slot_1based) {
                    // Slot-based substitution wins (class name or member name).
                    Some(new_str.as_bytes().to_vec())
                } else {
                    // Fall back to content-based descriptor rewriting.
                    std::str::from_utf8(content).ok().and_then(|s| {
                        if s.contains('L') && s.contains(';') {
                            let remapped = remap_descriptor(s, idx);
                            if remapped != s {
                                return Some(remapped.into_bytes());
                            }
                        }
                        None
                    })
                };

                if let Some(new_bytes) = rewritten {
                    let new_len = new_bytes.len();
                    out.push(1u8);
                    out.push((new_len >> 8) as u8);
                    out.push(new_len as u8);
                    out.extend_from_slice(&new_bytes);
                } else {
                    out.extend_from_slice(&input[pos..pos + 3 + orig_len]);
                }

                pos += 3 + orig_len;
                slot += 1;
            }
            3 | 4 => {
                if pos + 5 > input.len() { return input.to_vec(); }
                out.extend_from_slice(&input[pos..pos + 5]);
                pos += 5;
                slot += 1;
            }
            5 | 6 => {
                // Long / Double: tag(1) + 8 bytes, occupies two pool slots.
                if pos + 9 > input.len() { return input.to_vec(); }
                out.extend_from_slice(&input[pos..pos + 9]);
                pos += 9;
                slot += 2;
            }
            7 | 8 | 16 | 19 | 20 => {
                // Class / String / MethodType / Module / Package: tag(1) + index(2)
                if pos + 3 > input.len() { return input.to_vec(); }
                out.extend_from_slice(&input[pos..pos + 3]);
                pos += 3;
                slot += 1;
            }
            9 | 10 | 11 | 12 => {
                // FieldRef / MethodRef / InterfaceMethodRef / NameAndType: tag(1) + index(2) + index(2)
                if pos + 5 > input.len() { return input.to_vec(); }
                out.extend_from_slice(&input[pos..pos + 5]);
                pos += 5;
                slot += 1;
            }
            15 => {
                // MethodHandle: tag(1) + kind(1) + index(2)
                if pos + 4 > input.len() { return input.to_vec(); }
                out.extend_from_slice(&input[pos..pos + 4]);
                pos += 4;
                slot += 1;
            }
            17 | 18 => {
                // Dynamic / InvokeDynamic: tag(1) + bootstrap_attr(2) + nat_index(2)
                if pos + 5 > input.len() { return input.to_vec(); }
                out.extend_from_slice(&input[pos..pos + 5]);
                pos += 5;
                slot += 1;
            }
            _ => {
                eprintln!("specialsource: unknown pool tag {tag} at byte {pos} — passing through");
                return input.to_vec();
            }
        }
    }

    // Append everything after the pool section (access flags, class refs,
    // fields, methods, attributes) unchanged. Pool indices in this region
    // are still valid because we preserved slot ordering and count.
    out.extend_from_slice(&input[pos..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_args_minimal() {
        let raw = vec![
            "--in-jar".into(), "/i".into(),
            "--out-jar".into(), "/o".into(),
            "--srg-in".into(), "/m".into(),
        ];
        let p = parse_args(&raw).unwrap();
        assert_eq!((p.in_jar.as_str(), p.out_jar.as_str(), p.srg_in.as_str()), ("/i", "/o", "/m"));
        assert!(!p.kill_lvt);
    }

    #[test]
    fn parse_args_kill_lvt_flag() {
        let raw = vec!["--in-jar".into(), "/i".into(), "--out-jar".into(), "/o".into(), "--srg-in".into(), "/m".into(), "--kill-lvt".into()];
        assert!(parse_args(&raw).unwrap().kill_lvt);
    }

    #[test]
    fn parse_args_rejects_live() {
        let raw = vec!["--in-jar".into(), "/i".into(), "--out-jar".into(), "/o".into(), "--srg-in".into(), "/m".into(), "--live".into()];
        assert!(parse_args(&raw).is_err());
    }

    #[test]
    fn parse_args_missing_required_errors() {
        assert!(parse_args(&vec!["--in-jar".into(), "/i".into()]).is_err());
    }
}
