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
//!
//! The same code page is what a JVM writes its own console output in, so the
//! reverse conversion lives here too: [`decode_console_line`].

use std::path::Path;

/// `CP_ACP` — the system default ANSI code page. Defined here because
/// windows-sys does not export the constant.
#[cfg(windows)]
const CP_ACP: u32 = 0;
/// Where the "Beta: Use Unicode UTF-8 for worldwide language support"
/// setting puts the ACP. Everything is expressible there.
#[cfg(windows)]
const CP_UTF8: u32 = 65001;

/// True if `path` survives a round trip through the system ANSI code page.
#[cfg(windows)]
pub fn path_launchable(path: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Globalization::{GetACP, WideCharToMultiByte, WC_NO_BEST_FIT_CHARS};

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

/// One line a child process wrote to its console pipe, as text. Never fails.
///
/// A JVM writes `System.out` in the platform charset — on Windows the ANSI code
/// page when its output is a pipe, e.g. CP1251 on a Russian system, so its own
/// messages (`FileNotFoundException … (Не удается найти указанный файл)`) are
/// not UTF-8 — while log4j layouts write UTF-8. One server console carries both,
/// line by line, so the decision is per line: valid UTF-8 as is; anything else
/// through the system ANSI code page on Windows, and as lossy UTF-8 elsewhere
/// (whose platform charset is UTF-8 in practice). A line nothing can decode
/// still arrives, with replacement characters.
pub fn decode_console_line(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_owned(),
        Err(_) => decode_platform_charset(bytes),
    }
}

#[cfg(windows)]
fn decode_platform_charset(bytes: &[u8]) -> String {
    use windows_sys::Win32::Globalization::GetACP;
    // SAFETY: GetACP takes no arguments and cannot fail.
    let acp = unsafe { GetACP() };
    // Not valid UTF-8 under a UTF-8 code page: nothing can decode it better.
    if acp == CP_UTF8 {
        return String::from_utf8_lossy(bytes).into_owned();
    }
    decode_with_code_page(bytes, CP_ACP)
}

#[cfg(not(windows))]
fn decode_platform_charset(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// `bytes` in Windows code page `code_page` (`CP_ACP` for the system's), as
/// text. Windows maps a byte the code page leaves undefined to a default
/// character; a conversion it refuses outright falls back to lossy UTF-8, so the
/// line is never dropped.
#[cfg(windows)]
fn decode_with_code_page(bytes: &[u8], code_page: u32) -> String {
    use windows_sys::Win32::Globalization::MultiByteToWideChar;

    let lossy = || String::from_utf8_lossy(bytes).into_owned();
    if bytes.is_empty() {
        return String::new();
    }
    let Ok(len) = i32::try_from(bytes.len()) else {
        return lossy();
    };
    // Size probe, then the real conversion.
    //
    // SAFETY: `bytes` is a valid initialised slice whose length is passed as
    // `cbMultiByte`; the output pointer is null with a zero length, which is the
    // documented way to ask for the required buffer size.
    let needed =
        unsafe { MultiByteToWideChar(code_page, 0, bytes.as_ptr(), len, std::ptr::null_mut(), 0) };
    if needed <= 0 {
        return lossy();
    }
    let mut wide = vec![0u16; needed as usize];
    // SAFETY: `wide` holds exactly `needed` UTF-16 units, the size the probe
    // above asked for, and stays alive for the call.
    let written = unsafe {
        MultiByteToWideChar(code_page, 0, bytes.as_ptr(), len, wide.as_mut_ptr(), needed)
    };
    if written <= 0 {
        return lossy();
    }
    wide.truncate(written as usize);
    String::from_utf16_lossy(&wide)
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

    /// `Не удается` in CP1251 — the start of the JVM's own "file not found"
    /// message on a Russian Windows, which is not UTF-8.
    const CP1251_LINE: &[u8] = b"\xcd\xe5 \xf3\xe4\xe0\xe5\xf2\xf1\xff";

    #[test]
    fn a_utf8_console_line_is_kept_as_is() {
        assert_eq!(
            decode_console_line("[Server thread/INFO]: Готово".as_bytes()),
            "[Server thread/INFO]: Готово"
        );
        assert_eq!(decode_console_line(b""), "");
    }

    #[test]
    fn a_console_line_that_is_not_utf8_still_arrives() {
        // Whatever this host's code page, the line is decoded, never dropped.
        // (What it reads as depends on the code page; see the CP1251 test.)
        let line = decode_console_line(CP1251_LINE);
        assert!(!line.is_empty(), "{line:?}");
        assert!(!line.contains('\n'), "{line:?}");
    }

    #[cfg(windows)]
    #[test]
    fn cp1251_bytes_decode_to_cyrillic() {
        assert_eq!(decode_with_code_page(CP1251_LINE, 1251), "Не удается");
    }
}
