//! The on-disk cache of Forge/NeoForge installers: `<app_data>/forge/installers/<mc>-<fv>.jar`, and
//! beside it `<mc>-<fv>.jar.sha1`, the SHA-1 of the bytes Lucerna kept (spec
//! 2026-11-04-forge-installer-cache-recovery). For an installer downloaded from now on that is the
//! SHA-1 maven publishes, checked before the file was kept; for one an older Lucerna kept with no
//! record, it is recorded once the file has passed a whole-zip check.
//!
//! The record guards against accidents — an interrupted write, disk damage, another program
//! rewriting the file — not against someone who can write to the data root. Only file handling
//! lives here; `forge::meta` decides what to download.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use sha1::{Digest, Sha1};

use crate::error::{Error, Result};

/// What the cache holds for one installer.
#[derive(Debug)]
pub(crate) enum Cached {
    /// Nothing is cached.
    Absent,
    /// The bytes Lucerna kept.
    Intact(Vec<u8>),
    /// Present but not to be used: its SHA-1 differs from its record, the record is not a digest,
    /// a file with no record is not a whole zip, or the file or its record could not be read. Why,
    /// for the log.
    Unusable(String),
}

/// `<jar>.sha1`: where the jar's SHA-1 is recorded.
pub(crate) fn digest_path(jar: &Path) -> PathBuf {
    let mut name = jar.file_name().unwrap_or_default().to_os_string();
    name.push(".sha1");
    jar.with_file_name(name)
}

/// What the cache holds at `jar`. Only `NotFound` means absent; a read that fails any other way is
/// "could not tell", and its bytes are never used.
pub(crate) async fn load(jar: &Path) -> Cached {
    let bytes = match tokio::fs::read(jar).await {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == ErrorKind::NotFound => return Cached::Absent,
        Err(e) => return Cached::Unusable(format!("it could not be read: {e}")),
    };
    let record_path = digest_path(jar);
    let record = match tokio::fs::read_to_string(&record_path).await {
        Ok(text) => text,
        Err(e) if e.kind() == ErrorKind::NotFound => return adopt_unrecorded(jar, bytes).await,
        Err(e) => {
            return Cached::Unusable(format!(
                "its recorded SHA-1 {} could not be read: {e}",
                record_path.display()
            ))
        }
    };
    let Some(recorded) = super::meta::parse_maven_sha1(&record) else {
        return Cached::Unusable(format!(
            "its recorded SHA-1 {} is not a digest",
            record_path.display()
        ));
    };
    let actual = hex::encode(Sha1::digest(&bytes));
    if actual == recorded {
        Cached::Intact(bytes)
    } else {
        Cached::Unusable(format!("its SHA-1 is {actual}, {recorded} was recorded"))
    }
}

/// A jar an older Lucerna kept, written in place, with no record: used if it is a whole zip, and
/// its SHA-1 recorded so the next read is an ordinary one (spec §3.4).
async fn adopt_unrecorded(jar: &Path, bytes: Vec<u8>) -> Cached {
    if let Err(why) = zip_is_intact(&bytes) {
        return Cached::Unusable(format!("it has no recorded SHA-1 and {why}"));
    }
    record_digest(jar, &hex::encode(Sha1::digest(&bytes))).await;
    Cached::Intact(bytes)
}

/// Read every entry to its end, so zip 9 checks each one's CRC-32; a file cut short has no central
/// directory and fails to open.
fn zip_is_intact(bytes: &[u8]) -> std::result::Result<(), String> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|e| format!("it is not a whole zip ({e})"))?;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("its entry {i} cannot be opened ({e})"))?;
        if let Err(e) = std::io::copy(&mut entry, &mut std::io::sink()) {
            return Err(format!(
                "its entry {} is damaged ({e})",
                crate::zip_entry::name(&entry)
            ));
        }
    }
    Ok(())
}

/// Keep `bytes`, whose SHA-1 is `sha1`, as the cached installer at `jar`: the jar first, then its
/// record, so a record never vouches for bytes not yet in place (spec §3.5).
pub(crate) async fn keep(jar: &Path, bytes: &[u8], sha1: &str) -> Result<()> {
    write_replacing(jar, bytes).await?;
    record_digest(jar, sha1).await;
    Ok(())
}

/// Record `sha1` as the SHA-1 of the jar at `jar`. A failure is logged, not returned: the jar is in
/// place, and without a record the next read checks it again (spec §3.4).
async fn record_digest(jar: &Path, sha1: &str) {
    let record = digest_path(jar);
    if let Err(e) = write_replacing(&record, format!("{sha1}\n").as_bytes()).await {
        crate::diag!(
            "forge: the installer {} is kept, but its SHA-1 could not be recorded ({e}); it will be checked again when next read",
            jar.display()
        );
    }
}

/// Replace `target` with `bytes` in one step: a temp file of its own beside it, flushed to disk,
/// then renamed over it. A crash leaves the old file or the new one, never part of one, and two
/// writers never share a temp. A temp left by a failed write or rename is removed; a failed
/// removal is logged.
async fn write_replacing(target: &Path, bytes: &[u8]) -> Result<()> {
    let parent = target
        .parent()
        .ok_or_else(|| Error::io(target.display().to_string(), "no parent folder"))?;
    tokio::fs::create_dir_all(parent)
        .await
        .map_err(|e| Error::io(parent.display().to_string(), e))?;
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp = parent.join(format!(".{name}.{}.tmp", crate::instances::ids::new_id()));
    if let Err(e) = write_flushed(&tmp, bytes).await {
        discard(&tmp).await;
        return Err(Error::io(tmp.display().to_string(), e));
    }
    if let Err(e) = tokio::fs::rename(&tmp, target).await {
        discard(&tmp).await;
        return Err(Error::io(target.display().to_string(), e));
    }
    Ok(())
}

async fn write_flushed(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use tokio::io::AsyncWriteExt;
    let mut file = tokio::fs::File::create(path).await?;
    file.write_all(bytes).await?;
    file.sync_all().await
}

/// Remove a temp file a failed write left; a failure other than "already gone" is logged.
async fn discard(tmp: &Path) {
    match tokio::fs::remove_file(tmp).await {
        Ok(()) => {}
        Err(e) if e.kind() == ErrorKind::NotFound => {}
        Err(e) => crate::diag!(
            "forge: could not remove the temp file {} ({e})",
            tmp.display()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    fn sha(b: &[u8]) -> String {
        hex::encode(Sha1::digest(b))
    }

    fn jar_in(td: &TempDir) -> PathBuf {
        td.path()
            .join("forge")
            .join("installers")
            .join("1.21.1-21.1.0.jar")
    }

    fn put(jar: &Path, bytes: &[u8], record: Option<&str>) {
        std::fs::create_dir_all(jar.parent().unwrap()).unwrap();
        std::fs::write(jar, bytes).unwrap();
        if let Some(r) = record {
            std::fs::write(digest_path(jar), r).unwrap();
        }
    }

    /// The payload of the zip's one entry, stored uncompressed so a test can find and change it.
    const PAYLOAD: &[u8] = b"install_profile payload, stored as is";

    fn whole_zip() -> Vec<u8> {
        let mut zw = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zw.start_file("install_profile.json", opts).unwrap();
        zw.write_all(PAYLOAD).unwrap();
        zw.finish().unwrap().into_inner()
    }

    fn names_in(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn the_record_sits_beside_the_jar() {
        let jar = Path::new("forge")
            .join("installers")
            .join("1.21.1-21.1.0.jar");
        assert_eq!(
            digest_path(&jar),
            Path::new("forge")
                .join("installers")
                .join("1.21.1-21.1.0.jar.sha1")
        );
    }

    #[tokio::test]
    async fn a_jar_matching_its_record_is_intact() {
        let td = TempDir::new().unwrap();
        let jar = jar_in(&td);
        put(
            &jar,
            b"installer",
            Some(&format!("{}\n", sha(b"installer"))),
        );
        let got = load(&jar).await;
        assert!(
            matches!(got, Cached::Intact(ref b) if b == b"installer"),
            "got {got:?}"
        );
    }

    #[tokio::test]
    async fn a_truncated_jar_is_unusable() {
        let td = TempDir::new().unwrap();
        let jar = jar_in(&td);
        // What an interrupted in-place write left: the first part of the kept bytes.
        put(&jar, b"instal", Some(&sha(b"installer")));
        let got = load(&jar).await;
        assert!(matches!(got, Cached::Unusable(_)), "got {got:?}");
    }

    #[tokio::test]
    async fn a_record_that_is_not_a_digest_vouches_for_nothing() {
        let td = TempDir::new().unwrap();
        let jar = jar_in(&td);
        put(&jar, &whole_zip(), Some("not a digest"));
        let got = load(&jar).await;
        assert!(matches!(got, Cached::Unusable(_)), "got {got:?}");
    }

    #[tokio::test]
    async fn a_record_that_cannot_be_read_vouches_for_nothing() {
        let td = TempDir::new().unwrap();
        let jar = jar_in(&td);
        put(&jar, &whole_zip(), None);
        // A directory where the record should be: reading it fails, and not with NotFound.
        std::fs::create_dir_all(digest_path(&jar)).unwrap();
        let got = load(&jar).await;
        assert!(matches!(got, Cached::Unusable(_)), "got {got:?}");
    }

    #[tokio::test]
    async fn an_unrecorded_whole_zip_is_intact_and_recorded() {
        let td = TempDir::new().unwrap();
        let jar = jar_in(&td);
        let zip = whole_zip();
        put(&jar, &zip, None);
        let got = load(&jar).await;
        assert!(
            matches!(got, Cached::Intact(ref b) if *b == zip),
            "got {got:?}"
        );
        assert_eq!(
            std::fs::read_to_string(digest_path(&jar)).unwrap(),
            format!("{}\n", sha(&zip))
        );
    }

    #[tokio::test]
    async fn an_unrecorded_zip_cut_short_is_unusable() {
        let td = TempDir::new().unwrap();
        let jar = jar_in(&td);
        let zip = whole_zip();
        put(&jar, &zip[..zip.len() - 10], None);
        let got = load(&jar).await;
        assert!(matches!(got, Cached::Unusable(_)), "got {got:?}");
        assert!(!digest_path(&jar).exists(), "nothing may vouch for it");
    }

    #[tokio::test]
    async fn an_unrecorded_zip_with_a_changed_byte_is_unusable() {
        let td = TempDir::new().unwrap();
        let jar = jar_in(&td);
        let mut zip = whole_zip();
        // The structure is whole; only the entry's CRC-32 can tell its data changed.
        let at = zip
            .windows(PAYLOAD.len())
            .position(|w| w == PAYLOAD)
            .expect("a stored entry keeps its bytes");
        zip[at] ^= 0x01;
        put(&jar, &zip, None);
        let got = load(&jar).await;
        assert!(matches!(got, Cached::Unusable(_)), "got {got:?}");
        assert!(!digest_path(&jar).exists(), "nothing may vouch for it");
    }

    #[tokio::test]
    async fn no_jar_is_absent() {
        let td = TempDir::new().unwrap();
        assert!(matches!(load(&jar_in(&td)).await, Cached::Absent));
    }

    #[tokio::test]
    async fn a_jar_that_cannot_be_read_is_unusable_not_absent() {
        let td = TempDir::new().unwrap();
        let jar = jar_in(&td);
        // A directory in the jar's place: the read fails, and "could not tell" is not "absent".
        std::fs::create_dir_all(&jar).unwrap();
        let got = load(&jar).await;
        assert!(matches!(got, Cached::Unusable(_)), "got {got:?}");
    }

    #[tokio::test]
    async fn keep_replaces_the_jar_records_it_and_leaves_nothing_else() {
        let td = TempDir::new().unwrap();
        let jar = jar_in(&td);
        put(&jar, b"old bytes", Some("old record"));
        keep(&jar, b"new", &sha(b"new")).await.unwrap();
        assert_eq!(std::fs::read(&jar).unwrap(), b"new");
        assert!(matches!(load(&jar).await, Cached::Intact(ref b) if b == b"new"));
        assert_eq!(
            names_in(jar.parent().unwrap()),
            vec![
                "1.21.1-21.1.0.jar".to_string(),
                "1.21.1-21.1.0.jar.sha1".to_string()
            ]
        );
    }

    #[tokio::test]
    async fn a_replacement_that_cannot_land_is_an_error_and_leaves_no_temp() {
        let td = TempDir::new().unwrap();
        let jar = jar_in(&td);
        // A non-empty directory in the target's place: nothing can be renamed over it.
        std::fs::create_dir_all(jar.join("inside")).unwrap();
        assert!(keep(&jar, b"new", &sha(b"new")).await.is_err());
        assert_eq!(
            names_in(jar.parent().unwrap()),
            vec!["1.21.1-21.1.0.jar".to_string()]
        );
    }
}
