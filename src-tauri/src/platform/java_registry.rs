//! Java installs recorded in the Windows registry — read only. Covers the
//! installers that record a Java home: Oracle/JavaSoft, Eclipse Adoptium (and
//! its AdoptOpenJDK / Eclipse Foundation predecessors), Microsoft, Azul Zulu
//! and BellSoft Liberica, machine-wide and per-user, in both registry views.
//! Nothing here writes, so `structural_platform_chokepoint.rs` (which polices
//! registry WRITES) is not involved; it lives behind `platform::` all the same.

use std::path::PathBuf;

/// Every Java home the registry names. Unreadable keys are skipped (and
/// logged): this feeds a list the user picks from, and a pick is probed.
#[cfg(windows)]
pub fn java_homes() -> Vec<PathBuf> {
    win::java_homes()
}

#[cfg(not(windows))]
pub fn java_homes() -> Vec<PathBuf> {
    Vec::new()
}

#[cfg(windows)]
mod win {
    use std::path::PathBuf;
    use windows_sys::Win32::Foundation::{
        ERROR_FILE_NOT_FOUND, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS, FILETIME,
    };
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER,
        HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY,
    };

    /// (base key, path under each version subkey, value naming the home).
    const SOURCES: &[(&str, &str, &str)] = &[
        (
            r"SOFTWARE\JavaSoft\Java Runtime Environment",
            "",
            "JavaHome",
        ),
        (r"SOFTWARE\JavaSoft\Java Development Kit", "", "JavaHome"),
        (r"SOFTWARE\JavaSoft\JRE", "", "JavaHome"),
        (r"SOFTWARE\JavaSoft\JDK", "", "JavaHome"),
        (r"SOFTWARE\Eclipse Adoptium\JDK", r"hotspot\MSI", "Path"),
        (r"SOFTWARE\Eclipse Adoptium\JRE", r"hotspot\MSI", "Path"),
        (r"SOFTWARE\Eclipse Foundation\JDK", r"hotspot\MSI", "Path"),
        (r"SOFTWARE\AdoptOpenJDK\JDK", r"hotspot\MSI", "Path"),
        (r"SOFTWARE\AdoptOpenJDK\JRE", r"hotspot\MSI", "Path"),
        (r"SOFTWARE\Microsoft\JDK", r"hotspot\MSI", "Path"),
        (r"SOFTWARE\Azul Systems\Zulu", "", "InstallationPath"),
        (r"SOFTWARE\BellSoft\Liberica", "", "InstallationPath"),
    ];

    /// An open key, closed on drop.
    struct Key(HKEY);

    impl Drop for Key {
        fn drop(&mut self) {
            // SAFETY: the handle came from a successful RegOpenKeyExW and is
            // closed exactly once, here.
            unsafe {
                RegCloseKey(self.0);
            }
        }
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn open(root: HKEY, path: &str, view: u32) -> Option<Key> {
        let p = wide(path);
        let mut h: HKEY = std::ptr::null_mut();
        // SAFETY: standard registry FFI; `p` and `h` are locals that outlive
        // the call.
        let rc = unsafe { RegOpenKeyExW(root, p.as_ptr(), 0, KEY_READ | view, &mut h) };
        if rc == ERROR_SUCCESS {
            return Some(Key(h));
        }
        if rc != ERROR_FILE_NOT_FOUND {
            crate::diag!("java registry: cannot open {path}: error {rc}");
        }
        None
    }

    fn subkeys(key: &Key) -> Vec<String> {
        let mut out = Vec::new();
        let mut idx = 0u32;
        loop {
            let mut buf = [0u16; 256];
            let mut len = buf.len() as u32; // in CHARS
                                            // SAFETY: `buf`/`len` are locals sized for the call; the key is
                                            // open for the life of `key`.
            let rc = unsafe {
                RegEnumKeyExW(
                    key.0,
                    idx,
                    buf.as_mut_ptr(),
                    &mut len,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut::<FILETIME>(),
                )
            };
            if rc == ERROR_NO_MORE_ITEMS {
                break;
            }
            if rc != ERROR_SUCCESS {
                crate::diag!("java registry: subkey {idx} unreadable: error {rc}");
                break;
            }
            out.push(String::from_utf16_lossy(&buf[..len as usize]));
            idx += 1;
        }
        out
    }

    fn query_sz(key: &Key, name: &str) -> Option<String> {
        let n = wide(name);
        let mut buf = [0u16; 1024];
        let mut len = (buf.len() * 2) as u32; // in BYTES
                                              // SAFETY: `buf`/`len` are locals sized for the call; the key is open.
        let rc = unsafe {
            RegQueryValueExW(
                key.0,
                n.as_ptr(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                buf.as_mut_ptr() as *mut u8,
                &mut len,
            )
        };
        if rc != ERROR_SUCCESS {
            if rc != ERROR_FILE_NOT_FOUND {
                crate::diag!("java registry: value {name} unreadable: error {rc}");
            }
            return None;
        }
        let chars = (len as usize / 2).min(buf.len());
        let s = String::from_utf16_lossy(&buf[..chars]);
        let s = s.trim_end_matches('\0').trim().to_string();
        (!s.is_empty()).then_some(s)
    }

    pub fn java_homes() -> Vec<PathBuf> {
        let mut out = Vec::new();
        for root in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
            for view in [KEY_WOW64_64KEY, KEY_WOW64_32KEY] {
                for (base, suffix, value) in SOURCES {
                    let Some(b) = open(root, base, view) else {
                        continue;
                    };
                    for sub in subkeys(&b) {
                        let path = if suffix.is_empty() {
                            format!(r"{base}\{sub}")
                        } else {
                            format!(r"{base}\{sub}\{suffix}")
                        };
                        if let Some(home) =
                            open(root, &path, view).and_then(|k| query_sz(&k, value))
                        {
                            out.push(PathBuf::from(home));
                        }
                    }
                }
            }
        }
        out
    }
}
