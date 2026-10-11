//! What a user-supplied Java path means, and which sibling executable each
//! use needs.
//!
//! The UI stores the GUI executable (`javaw.exe` on Windows) so the client
//! never opens a console window. A probe needs the console one (`java.exe`),
//! which sits beside it in the same `bin/`.

use super::verdict::JavaUnusableReason;
use std::path::{Path, PathBuf};

/// Longest path accepted from the UI. Far beyond any real install path; it
/// only bounds what an IPC caller can make us stat.
const MAX_LEN: usize = 1024;

/// Whether the stored executable is there. `NotFound` and `Unreadable` are
/// kept apart (CLAUDE.md, Fallback discipline: discrimination).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Existence {
    Present,
    NotFound,
    Unreadable(String),
}

pub fn existence(p: &Path) -> Existence {
    match std::fs::metadata(p) {
        Ok(m) if m.is_file() => Existence::Present,
        Ok(_) => Existence::Unreadable("not a file".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Existence::NotFound,
        Err(e) => Existence::Unreadable(e.to_string()),
    }
}

fn is_java_exe_name(name: &str) -> bool {
    if cfg!(windows) {
        name.eq_ignore_ascii_case("java.exe") || name.eq_ignore_ascii_case("javaw.exe")
    } else {
        name == "java"
    }
}

fn file_name_is(p: &Path, name: &str) -> bool {
    p.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.eq_ignore_ascii_case(name))
}

/// A user-supplied path → the executable to store, or why it is not one.
/// Pure apart from one `is_file` check for the `javaw.exe` sibling.
pub fn normalize(raw: &str) -> Result<PathBuf, JavaUnusableReason> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_LEN || trimmed.chars().any(char::is_control) {
        return Err(JavaUnusableReason::NotJava);
    }
    let p = PathBuf::from(trimmed);
    if !p.is_absolute() {
        return Err(JavaUnusableReason::NotJava);
    }
    // macOS's /usr/bin/java is a stub: with no JDK installed, running it opens
    // a system "install Java" dialog, and with one it runs whichever JDK is the
    // system default today. Neither is a Java the user chose.
    if cfg!(target_os = "macos") && p == Path::new("/usr/bin/java") {
        return Err(JavaUnusableReason::NotJava);
    }
    // macOS: the file dialog shows a `.jdk` bundle as one item; the executable
    // lives inside it.
    if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("jdk")) {
        return Ok(p.join("Contents").join("Home").join("bin").join("java"));
    }
    let name = p
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or(JavaUnusableReason::NotJava)?;
    if !is_java_exe_name(name) {
        return Err(JavaUnusableReason::NotJava);
    }
    Ok(prefer_gui(&p))
}

/// On Windows, `javaw.exe` beside a `java.exe` when there is one; otherwise
/// the path itself.
pub fn prefer_gui(p: &Path) -> PathBuf {
    if cfg!(windows) && file_name_is(p, "java.exe") {
        let w = p.with_file_name("javaw.exe");
        if w.is_file() {
            return w;
        }
    }
    p.to_path_buf()
}

/// The console executable beside a stored one: `java.exe` for a `javaw.exe`
/// on Windows, the path itself elsewhere. Existence is the caller's business:
/// a missing sibling surfaces as a probe that could not start.
pub fn console_exe(p: &Path) -> PathBuf {
    if cfg!(windows) && file_name_is(p, "javaw.exe") {
        return p.with_file_name("java.exe");
    }
    p.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_empty_and_control_paths_are_refused() {
        assert_eq!(normalize(""), Err(JavaUnusableReason::NotJava));
        assert_eq!(normalize("bin/java"), Err(JavaUnusableReason::NotJava));
        assert_eq!(normalize("/a\u{7}/java"), Err(JavaUnusableReason::NotJava));
        assert_eq!(
            normalize(&format!("/{}", "a".repeat(MAX_LEN))),
            Err(JavaUnusableReason::NotJava)
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn unix_accepts_java_and_refuses_other_names() {
        assert_eq!(
            normalize("/opt/jdk/bin/java"),
            Ok(PathBuf::from("/opt/jdk/bin/java"))
        );
        assert_eq!(
            normalize("/opt/jdk/bin/javac"),
            Err(JavaUnusableReason::NotJava)
        );
        assert_eq!(
            normalize("/usr/bin/python3"),
            Err(JavaUnusableReason::NotJava)
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn a_jdk_bundle_resolves_to_its_executable() {
        assert_eq!(
            normalize("/Library/Java/JavaVirtualMachines/temurin-21.jdk"),
            Ok(PathBuf::from(
                "/Library/Java/JavaVirtualMachines/temurin-21.jdk/Contents/Home/bin/java"
            ))
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_prefers_javaw_beside_a_picked_java_exe() {
        let dir = tempfile::tempdir().unwrap();
        let java = dir.path().join("java.exe");
        let javaw = dir.path().join("javaw.exe");
        std::fs::write(&java, b"").unwrap();
        std::fs::write(&javaw, b"").unwrap();
        assert_eq!(normalize(java.to_str().unwrap()), Ok(javaw.clone()));
        assert_eq!(console_exe(&javaw), java);
    }

    #[cfg(windows)]
    #[test]
    fn windows_keeps_java_exe_when_there_is_no_javaw() {
        let dir = tempfile::tempdir().unwrap();
        let java = dir.path().join("JAVA.EXE");
        std::fs::write(&java, b"").unwrap();
        assert_eq!(normalize(java.to_str().unwrap()), Ok(java));
    }

    #[cfg(windows)]
    #[test]
    fn windows_refuses_other_executables() {
        assert_eq!(
            normalize(r"C:\Windows\notepad.exe"),
            Err(JavaUnusableReason::NotJava)
        );
    }

    #[test]
    fn existence_tells_missing_from_present() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("java");
        assert_eq!(existence(&f), Existence::NotFound);
        std::fs::write(&f, b"").unwrap();
        assert_eq!(existence(&f), Existence::Present);
        assert!(matches!(existence(dir.path()), Existence::Unreadable(_)));
    }
}
