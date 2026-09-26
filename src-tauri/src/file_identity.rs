//! Whether two paths name the same directory entry, as the file system itself
//! sees it.
//!
//! It lives outside `mods/`, `datapacks/` and `worlds/` on purpose. The Windows
//! identity read opens the entry through `fs::OpenOptions` with no data access
//! at all, only to ask NTFS for its file index. `structural_no_inplace_mods_write`
//! rightly refuses any `OpenOptions` under those trees. Nothing here reads or
//! writes a byte of any file.

use std::fs;
use std::io;
use std::path::Path;

/// Whether two paths are the same directory entry. Neither is followed.
/// Unix: the same `(dev, ino)`. Windows: the same volume serial number and
/// file index (`GetFileInformationByHandle`), which is NTFS's own identity —
/// two spellings of one entry, or two hard links, agree exactly. Elsewhere:
/// the same type, length and timestamps, a heuristic. `Err` = either could
/// not be stated (`NotFound`: that name is not there).
pub fn same_entry(a: &Path, b: &Path) -> io::Result<bool> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let (ma, mb) = (fs::symlink_metadata(a)?, fs::symlink_metadata(b)?);
        Ok(ma.dev() == mb.dev() && ma.ino() == mb.ino())
    }
    #[cfg(windows)]
    {
        Ok(windows_file_id(a)? == windows_file_id(b)?)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let (ma, mb) = (fs::symlink_metadata(a)?, fs::symlink_metadata(b)?);
        Ok(ma.file_type() == mb.file_type()
            && ma.len() == mb.len()
            && ma.modified()? == mb.modified()?
            && ma.created()? == mb.created()?)
    }
}

/// The volume serial number and file index of the entry at `path`, read from
/// a handle that asks for no data access and does not follow a link (the
/// `symlink_metadata` semantics), so it opens files and folders alike.
#[cfg(windows)]
fn windows_file_id(path: &Path) -> io::Result<(u32, u64)> {
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, FILE_FLAG_BACKUP_SEMANTICS,
        FILE_FLAG_OPEN_REPARSE_POINT,
    };
    let file = fs::OpenOptions::new()
        .access_mode(0)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    // SAFETY: BY_HANDLE_FILE_INFORMATION is plain integer data, for which
    // all-zero bytes are a valid value.
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    // SAFETY: `file` keeps the handle open for the whole call, and `info` is a
    // valid, writable BY_HANDLE_FILE_INFORMATION the call fills in.
    let ok = unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    let index = (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow);
    Ok((info.dwVolumeSerialNumber, index))
}
