//! jarsplitter — split a jar into "slim" (classes named in a TSRG
//! file) and "extra" (everything else). Used by Forge 1.13+ to
//! separate the parts of vanilla MC that get deobfuscated from the
//! parts that don't.

use crate::error::Result;
use crate::forge::patcher::{patcher_fail, ProcessorContext};
use std::io::{Read, Write};

pub async fn run(args: Vec<String>, _ctx: &ProcessorContext) -> Result<()> {
    let input = read_flag(&args, "--input").ok_or_else(|| {
        patcher_fail("jarsplitter", &"missing --input")
    })?;
    let slim = read_flag(&args, "--slim").ok_or_else(|| {
        patcher_fail("jarsplitter", &"missing --slim")
    })?;
    let extra = read_flag(&args, "--extra").ok_or_else(|| {
        patcher_fail("jarsplitter", &"missing --extra")
    })?;
    let srg_path = read_flag(&args, "--srg").ok_or_else(|| {
        patcher_fail("jarsplitter", &"missing --srg")
    })?;

    let srg_text = tokio::fs::read_to_string(&srg_path).await.map_err(|e| {
        patcher_fail("jarsplitter", &format!("read {srg_path}: {e}"))
    })?;
    let known = parse_class_set(&srg_text);

    let bytes = tokio::fs::read(&input).await.map_err(|e| {
        patcher_fail("jarsplitter", &format!("read {input}: {e}"))
    })?;
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| {
        patcher_fail("jarsplitter", &format!("zip open {input}: {e}"))
    })?;

    if let Some(parent) = std::path::Path::new(&slim).parent() {
        tokio::fs::create_dir_all(parent).await.ok();
    }
    if let Some(parent) = std::path::Path::new(&extra).parent() {
        tokio::fs::create_dir_all(parent).await.ok();
    }
    let slim_file = std::fs::File::create(&slim).map_err(|e| {
        patcher_fail("jarsplitter", &format!("create {slim}: {e}"))
    })?;
    let extra_file = std::fs::File::create(&extra).map_err(|e| {
        patcher_fail("jarsplitter", &format!("create {extra}: {e}"))
    })?;
    let mut slim_zip = zip::ZipWriter::new(slim_file);
    let mut extra_zip = zip::ZipWriter::new(extra_file);
    let opts: zip::write::FileOptions<()> = zip::write::FileOptions::default();

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| {
            patcher_fail("jarsplitter", &format!("zip index {i}: {e}"))
        })?;
        if entry.is_dir() { continue; }
        let name = entry.name().to_string();
        let mut buf = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut buf).map_err(|e| {
            patcher_fail("jarsplitter", &format!("read entry {name}: {e}"))
        })?;

        let goes_to_slim = name.ends_with(".class") && {
            let class_fqn = name.strip_suffix(".class").unwrap_or(&name);
            known.contains(class_fqn)
        };
        let target = if goes_to_slim { &mut slim_zip } else { &mut extra_zip };
        target.start_file::<_, ()>(name.clone(), opts).map_err(|e| {
            patcher_fail("jarsplitter", &format!("zip start {name}: {e}"))
        })?;
        target.write_all(&buf).map_err(|e| {
            patcher_fail("jarsplitter", &format!("zip write {name}: {e}"))
        })?;
    }
    slim_zip.finish().map_err(|e| patcher_fail("jarsplitter", &format!("slim finish: {e}")))?;
    extra_zip.finish().map_err(|e| patcher_fail("jarsplitter", &format!("extra finish: {e}")))?;
    Ok(())
}

fn read_flag(args: &[String], name: &str) -> Option<String> {
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == name { return it.next().cloned(); }
    }
    None
}

/// Extract the set of obf-FQNs (class names with `/` separators) from
/// a TSRG v1 file. Only class headers — lines without leading tab.
fn parse_class_set(body: &str) -> std::collections::HashSet<String> {
    let mut out = std::collections::HashSet::new();
    for line in body.lines() {
        let line = line.trim_end();
        if line.is_empty() || line.starts_with('\t') || line.starts_with('#') { continue; }
        if let Some(first) = line.split_whitespace().next() {
            out.insert(first.to_string());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_class_set_picks_only_headers() {
        let body = "net/a net/a\n\tfield_x name\nnet/b net/b\n";
        let s = parse_class_set(body);
        assert!(s.contains("net/a"));
        assert!(s.contains("net/b"));
        assert!(!s.contains("field_x"));
        assert_eq!(s.len(), 2);
    }

    #[tokio::test]
    async fn jarsplitter_routes_classes_correctly() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let in_path = dir.path().join("in.jar");
        let slim_path = dir.path().join("slim.jar");
        let extra_path = dir.path().join("extra.jar");
        let srg_path = dir.path().join("m.tsrg");
        std::fs::write(&srg_path, "net/known/A net/known/A\n").unwrap();
        {
            let f = std::fs::File::create(&in_path).unwrap();
            let mut w = zip::ZipWriter::new(f);
            let opts: zip::write::FileOptions<()> = zip::write::FileOptions::default();
            w.start_file::<_, ()>("net/known/A.class", opts).unwrap();
            w.write_all(b"a-bytes").unwrap();
            w.start_file::<_, ()>("net/other/B.class", opts).unwrap();
            w.write_all(b"b-bytes").unwrap();
            w.start_file::<_, ()>("assets/icon.png", opts).unwrap();
            w.write_all(b"png").unwrap();
            w.finish().unwrap();
        }
        let ctx = ProcessorContext { classpath: vec![], cache_dir: dir.path().to_path_buf(), java_bin: None };
        let args = vec![
            "--input".into(), in_path.display().to_string(),
            "--slim".into(), slim_path.display().to_string(),
            "--extra".into(), extra_path.display().to_string(),
            "--srg".into(), srg_path.display().to_string(),
        ];
        run(args, &ctx).await.expect("jarsplitter");

        let read_jar = |p: &std::path::Path| -> Vec<String> {
            let f = std::fs::File::open(p).unwrap();
            let mut z = zip::ZipArchive::new(f).unwrap();
            (0..z.len()).map(|i| z.by_index(i).unwrap().name().to_string()).collect()
        };
        let slim_names = read_jar(&slim_path);
        let extra_names = read_jar(&extra_path);
        assert_eq!(slim_names, vec!["net/known/A.class"]);
        assert!(extra_names.contains(&"net/other/B.class".to_string()));
        assert!(extra_names.contains(&"assets/icon.png".to_string()));
    }
}
