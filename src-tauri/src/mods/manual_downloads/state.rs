//! Pure decisions about `missing_mods` entries (spec §3.2–§3.4).

use crate::mods::modpack::schema::ModpackUnresolvable;

fn present(s: &Option<String>) -> Option<&str> {
    s.as_deref().map(str::trim).filter(|s| !s.is_empty())
}

/// One stable name per entry, the same before and after a pack update that
/// keeps the file: its CurseForge file id, else its md5, else its sha1, else
/// its name and file name (the pair `ResolvedMissing` already uses).
pub fn entry_key(e: &ModpackUnresolvable) -> String {
    if let Some(id) = present(&e.meta.file_id) {
        return format!("file:{id}");
    }
    if let Some(md5) = present(&e.meta.md5) {
        return format!("md5:{}", md5.to_ascii_lowercase());
    }
    if let Some(sha1) = present(&e.sha1) {
        return format!("sha1:{}", sha1.to_ascii_lowercase());
    }
    format!("name:{}|{}", e.mod_name, e.filename.to_ascii_lowercase())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::mods::modpack::schema::{ManualFileMeta, ModpackUnresolvable, UnresolvableReason};

    pub(crate) fn entry(
        file_id: Option<&str>,
        md5: Option<&str>,
        sha1: Option<&str>,
    ) -> ModpackUnresolvable {
        ModpackUnresolvable {
            reason: UnresolvableReason::DistributionDisabled,
            mod_name: "Scape and Run".into(),
            manual_action_url: "https://www.curseforge.com/projects/1".into(),
            filename: "SRP.jar".into(),
            size: 3.0,
            sha1: sha1.map(str::to_string),
            project_id: Some("1".into()),
            meta: ManualFileMeta {
                file_id: file_id.map(str::to_string),
                md5: md5.map(str::to_string),
                ..ManualFileMeta::default()
            },
        }
    }

    #[test]
    fn an_entry_is_keyed_by_file_id_then_md5_then_sha1_then_name() {
        assert_eq!(entry_key(&entry(Some("9"), Some("M"), Some("S"))), "file:9");
        assert_eq!(entry_key(&entry(None, Some("MM"), Some("S"))), "md5:mm");
        assert_eq!(entry_key(&entry(None, None, Some("SS"))), "sha1:ss");
        assert_eq!(
            entry_key(&entry(None, None, None)),
            "name:Scape and Run|srp.jar"
        );
        assert_eq!(
            entry_key(&entry(Some("  "), None, Some("S"))),
            "sha1:s",
            "blank is absent"
        );
    }
}
