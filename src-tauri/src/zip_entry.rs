//! A zip entry's name, read one way everywhere (zip 9).
//!
//! zip 9 decodes a name when asked and returns it as a `Result`: the bytes as
//! UTF-8 when they are UTF-8, else as CP437. Its only error is CP437 refusing
//! all-ASCII bytes that are not UTF-8, which cannot happen (ASCII is UTF-8),
//! so the fallbacks below never run. The lossy one is what zip 2 gave a name
//! flagged UTF-8 that was not; neither invents a name.
//!
//! zip 9 finds a name (`by_name`, `index_for_name`) by its raw bytes, not by
//! the decoded text. A name taken from a listing is therefore read back by
//! its index ([`indexed_names`]), never by name: a CP437 name decoded to text
//! no longer matches its own bytes.

use std::borrow::Cow;
use std::io::{Read, Seek};

/// The entry's name.
pub fn name<'a, R: Read + ?Sized>(entry: &'a zip::read::ZipFile<'_, R>) -> Cow<'a, str> {
    entry
        .name()
        .unwrap_or_else(|_| String::from_utf8_lossy(entry.name_raw()))
}

/// Every entry's name, in central-directory order, without opening any entry.
pub fn names<R: Read + Seek>(archive: &zip::ZipArchive<R>) -> impl Iterator<Item = Cow<'_, str>> {
    indexed_names(archive).map(|(_, name)| name)
}

/// Every entry's index and name, in central-directory order, without opening
/// any entry. Read an entry from this list by its index.
pub fn indexed_names<R: Read + Seek>(
    archive: &zip::ZipArchive<R>,
) -> impl Iterator<Item = (usize, Cow<'_, str>)> {
    (0..archive.len()).filter_map(move |i| match archive.name_for_index(i)? {
        Ok(name) => Some((i, name)),
        Err(e) => {
            // Unreachable (module doc); said in the log if it ever is not.
            crate::diag!("zip: entry {i}'s name could not be decoded ({e}); left out");
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Write};

    use super::*;

    fn archive(names: &[&str]) -> zip::ZipArchive<Cursor<Vec<u8>>> {
        let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for n in names {
            w.start_file(*n, zip::write::SimpleFileOptions::default())
                .unwrap();
            w.write_all(n.as_bytes()).unwrap();
        }
        zip::ZipArchive::new(Cursor::new(w.finish().unwrap().into_inner())).unwrap()
    }

    #[test]
    fn names_come_in_central_directory_order_with_their_index() {
        let z = archive(&["pack.mcmeta", "data/a/функция.mcfunction"]);
        let got: Vec<(usize, String)> = indexed_names(&z)
            .map(|(i, n)| (i, n.into_owned()))
            .collect();
        assert_eq!(
            got,
            [
                (0, "pack.mcmeta".to_string()),
                (1, "data/a/функция.mcfunction".to_string())
            ]
        );
    }

    #[test]
    fn an_entry_name_reads_as_written() {
        let mut z = archive(&["assets/ru_ru.json"]);
        let entry = z.by_index(0).unwrap();
        assert_eq!(name(&entry), "assets/ru_ru.json");
    }
}
