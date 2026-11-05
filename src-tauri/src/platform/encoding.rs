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
//! The same conversion decides what a Java argument file must hold
//! ([`jvm_launcher_bytes`], used by `launch::argfile`): the launcher reads that
//! file's bytes as it reads its converted command line.
//!
//! The same code page is what a JVM writes its own console output in, so the
//! reverse conversion lives here too: [`decode_console_line`].

use std::path::Path;

// `CP_ACP` is the system default ANSI code page; `CP_UTF8` is where the "Beta:
// Use Unicode UTF-8 for worldwide language support" setting puts the ACP.
#[cfg(windows)]
use windows_sys::Win32::Globalization::{CP_ACP, CP_UTF8};

/// True if `path` survives a round trip through the system ANSI code page.
#[cfg(windows)]
pub fn path_launchable(path: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Globalization::GetACP;

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
    wide_in_code_page(&wide, CP_ACP).is_some()
}

/// `wide` in code page `code_page`, or `None` when Windows had to substitute
/// a character — no best-fit lookalikes, which would name another path — or
/// refused the conversion. Never call it with `CP_UTF8`: that code page takes
/// neither flag nor `lpUsedDefaultChar` (callers handle UTF-8 themselves).
#[cfg(windows)]
fn wide_in_code_page(wide: &[u16], code_page: u32) -> Option<Vec<u8>> {
    use windows_sys::Win32::Globalization::{WideCharToMultiByte, WC_NO_BEST_FIT_CHARS};

    if wide.is_empty() {
        return Some(Vec::new());
    }
    let len = i32::try_from(wide.len()).ok()?;
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
            code_page,
            0,
            wide.as_ptr(),
            len,
            std::ptr::null_mut(),
            0,
            std::ptr::null(),
            std::ptr::null_mut(),
        )
    };
    if needed <= 0 {
        return None;
    }

    let mut buf = vec![0u8; needed as usize];
    let mut used_default: i32 = 0;
    // SAFETY: `buf` is exactly `needed` bytes, the size the probe above asked
    // for; `used_default` is a live local for the duration of the call.
    let written = unsafe {
        WideCharToMultiByte(
            code_page,
            WC_NO_BEST_FIT_CHARS,
            wide.as_ptr(),
            len,
            buf.as_mut_ptr(),
            needed,
            std::ptr::null(),
            &mut used_default,
        )
    };
    if written <= 0 || used_default != 0 {
        return None;
    }
    buf.truncate(written as usize);
    Some(buf)
}

/// `text` in Windows code page `code_page`, exactly, or why not: the first
/// character the code page cannot hold. `CP_UTF8` (65001) is UTF-8 itself.
#[cfg(windows)]
pub(crate) fn encode_in_code_page(text: &str, code_page: u32) -> Result<Vec<u8>, String> {
    if code_page == CP_UTF8 {
        return Ok(text.as_bytes().to_vec());
    }
    let wide: Vec<u16> = text.encode_utf16().collect();
    if let Some(bytes) = wide_in_code_page(&wide, code_page) {
        return Ok(bytes);
    }
    // Only on the way out: name the character, so the error says what to fix.
    let mut unit = [0u16; 2];
    let culprit = text
        .chars()
        .find(|c| wide_in_code_page(c.encode_utf16(&mut unit), code_page).is_none());
    Err(match culprit {
        Some(c) => format!(
            "{c:?} (U+{:04X}) has no place in code page {code_page}",
            u32::from(c)
        ),
        None => format!("Windows could not convert it to code page {code_page}"),
    })
}

/// `text` as the bytes a Java launcher reads it as — what an argument file must
/// hold to mean what the same text on the command line means. On Windows the
/// launcher turns its UTF-16 command line into the ANSI code page without
/// best-fit (JDK 21u `launcher/main.c`) and hands those bytes, like an argument
/// file's bytes, to the JVM, which decodes both as `sun.jnu.encoding`: so the
/// system ANSI code page, or UTF-8 when the "Beta: Use Unicode UTF-8" option
/// made that the ANSI code page. A character it cannot hold is refused: on the
/// command line it would have reached Java as `?`.
#[cfg(windows)]
pub fn jvm_launcher_bytes(text: &str) -> Result<Vec<u8>, String> {
    use windows_sys::Win32::Globalization::GetACP;
    // SAFETY: GetACP takes no arguments and cannot fail.
    let acp = unsafe { GetACP() };
    encode_in_code_page(text, acp)
}

/// Non-Windows: an argument is its UTF-8 bytes on the command line, and the
/// launcher reads a file's bytes the same way.
#[cfg(not(windows))]
pub fn jvm_launcher_bytes(text: &str) -> Result<Vec<u8>, String> {
    Ok(text.as_bytes().to_vec())
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
pub(crate) fn decode_with_code_page(bytes: &[u8], code_page: u32) -> String {
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

    #[test]
    fn ascii_is_the_same_bytes_in_every_launcher_encoding() {
        assert_eq!(
            jvm_launcher_bytes(r"-cp C:\a b\x.jar;y.jar").unwrap(),
            br"-cp C:\a b\x.jar;y.jar"
        );
    }

    #[cfg(windows)]
    #[test]
    fn cyrillic_is_written_in_cp1251_where_that_is_the_code_page() {
        assert_eq!(
            encode_in_code_page("Привет", 1251).unwrap(),
            b"\xcf\xf0\xe8\xe2\xe5\xf2"
        );
    }

    #[cfg(windows)]
    #[test]
    fn a_character_the_code_page_lacks_is_refused_and_named() {
        let e = encode_in_code_page(r"C:\Пользователь", 1252).unwrap_err();
        assert!(e.contains("U+041F") && e.contains("1252"), "{e}");
        // Nor a best-fit lookalike: 1252 would best-fit Ł to L, another path.
        let e = encode_in_code_page("Łódź", 1252).unwrap_err();
        assert!(e.contains("U+0141"), "{e}");
    }

    // `GetACP` answers per process. Lucerna's equals java.exe's only while
    // neither process manifest sets `activeCodePage` (java.exe's has none;
    // tauri-build's default has none). A Lucerna manifest that set UTF-8 would
    // write every argument file in UTF-8 for a java.exe reading CP1251.
    #[test]
    fn lucerna_leaves_the_ansi_code_page_to_the_system() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut read = 0;
        for entry in std::fs::read_dir(root).expect("src-tauri") {
            let path = entry.expect("entry").path();
            if !path.is_file() {
                continue;
            }
            let bytes = std::fs::read(&path).expect("readable");
            read += 1;
            assert!(
                !String::from_utf8_lossy(&bytes).contains("activeCodePage"),
                "{} sets activeCodePage",
                path.display()
            );
        }
        assert!(read > 0 && root.join("build.rs").is_file());
    }

    #[cfg(windows)]
    #[test]
    fn the_utf8_code_page_is_plain_utf8() {
        assert_eq!(
            encode_in_code_page("Привет 中", 65001).unwrap(),
            "Привет 中".as_bytes()
        );
    }
}
