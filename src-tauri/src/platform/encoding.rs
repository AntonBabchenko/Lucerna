//! Is a path expressible in the system ANSI code page?
//!
//! The JVM launcher on Windows reads its command line through the ANSI API and
//! converts it via the ACP. A path holding characters the ACP cannot express
//! reaches `java.exe` with `?` substituted — and `?` is an illegal Windows path
//! character, so the game dies with `InvalidPathException` before Minecraft
//! starts. We pass absolute paths as `--gameDir` and `-Djava.library.path`
//! (`launch::args`), which makes this a launch blocker, not a cosmetic issue.
//!
//! Deliberately a *measurement*, not a heuristic: we ask Windows to perform the
//! conversion and report whether it had to substitute anything. No code-page
//! tables of our own, no guessing which scripts are "safe".

use std::path::Path;

/// True if `path` survives a round trip through the system ANSI code page.
#[cfg(windows)]
pub fn path_launchable(path: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Globalization::{GetACP, WideCharToMultiByte, WC_NO_BEST_FIT_CHARS};

    /// `CP_ACP` — the system default ANSI code page. Defined here because
    /// windows-sys does not export the constant.
    const CP_ACP: u32 = 0;
    /// Where the "Beta: Use Unicode UTF-8 for worldwide language support"
    /// setting puts the ACP. Everything is expressible there.
    const CP_UTF8: u32 = 65001;

    let wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    if wide.is_empty() {
        return true;
    }

    // Load-bearing early return, not an optimisation: for CP_UTF8 the API
    // REQUIRES lpUsedDefaultChar to be null and fails with ERROR_INVALID_PARAMETER
    // otherwise. Without this branch, every path would report as unlaunchable on
    // a UTF-8-beta machine.
    //
    // SAFETY: GetACP takes no arguments and cannot fail.
    if unsafe { GetACP() } == CP_UTF8 {
        return true;
    }

    // Size probe, then the real conversion. The probe cannot carry
    // WC_NO_BEST_FIT_CHARS together with a non-null lpUsedDefaultChar, and the
    // documentation does not promise that flag is written on a zero-length call
    // — so the substitution check rides on the second pass, which has a real
    // buffer.
    //
    // SAFETY: `wide` is a valid initialised slice whose length is passed as
    // `cchWideChar`; the output pointer is null with a zero length, which is the
    // documented way to ask for the required buffer size.
    let needed = unsafe {
        WideCharToMultiByte(
            CP_ACP,
            0,
            wide.as_ptr(),
            wide.len() as i32,
            std::ptr::null_mut(),
            0,
            std::ptr::null(),
            std::ptr::null_mut(),
        )
    };
    if needed <= 0 {
        return false;
    }

    let mut buf = vec![0u8; needed as usize];
    let mut used_default: i32 = 0;
    // SAFETY: `buf` is exactly `needed` bytes, the size the probe above asked
    // for; `used_default` is a live local for the duration of the call.
    let written = unsafe {
        WideCharToMultiByte(
            CP_ACP,
            WC_NO_BEST_FIT_CHARS,
            wide.as_ptr(),
            wide.len() as i32,
            buf.as_mut_ptr(),
            needed,
            std::ptr::null(),
            &mut used_default,
        )
    };
    written > 0 && used_default == 0
}

/// Non-Windows: paths are UTF-8 bytes and the JVM reads `sun.jnu.encoding` from
/// the locale, so there is no lossy argv conversion to guard against.
#[cfg(not(windows))]
pub fn path_launchable(_path: &Path) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_path_is_always_launchable() {
        assert!(path_launchable(Path::new(
            r"C:\Lucerna\instances\My-Pack\.minecraft"
        )));
    }

    #[test]
    fn empty_path_is_launchable() {
        assert!(path_launchable(Path::new("")));
    }

    #[cfg(not(windows))]
    #[test]
    fn non_windows_accepts_any_unicode() {
        assert!(path_launchable(Path::new(
            "/home/u/instances/红石生电优化/.minecraft"
        )));
    }
}
