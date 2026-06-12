//! GPU-preference OS divergence (Windows registry / Linux env / macOS none),
//! isolated behind the platform seam. Pure-fn cores (`classify`,
//! `gpu_pref_value`, `gpu_launch_env`) are unit-tested cross-platform; the
//! thin `#[cfg]` probes/appliers wrap them. See
//! docs/superpowers/specs/2026-06-12-gpu-selection-design.md.

use crate::instances::schema::GpuPreference;
use serde::Serialize;
use specta::Type;

/// One GPU as shown to the UI.
#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq)]
pub struct GpuInfo {
    pub name: String,
}

/// What the UI needs to decide whether/how to show the GPU control.
#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GpuCapability {
    /// OS has no per-launch GPU mechanism (macOS). Hide the control.
    Unsupported,
    /// Mechanism exists but only one GPU — nothing to choose. Hide.
    SingleGpu,
    /// Two or more GPUs — show the dropdown.
    Available {
        gpus: Vec<GpuInfo>,
        /// Name the "high performance" option resolves to, if known.
        high: Option<String>,
        /// Name the "power saving" option resolves to, if known.
        low: Option<String>,
    },
}

/// Internal probe result, fed to `classify`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuAdapter {
    pub name: String,
    /// True for the integrated GPU (iGPU). Drives high/low labelling.
    pub integrated: bool,
}

/// Pure classifier: adapter list → capability. <2 adapters → `SingleGpu`;
/// otherwise `Available`, labelling the first discrete adapter as `high`
/// and the first integrated as `low`. (Platforms with no mechanism return
/// `Unsupported` directly from `capability()` without calling this.)
pub fn classify(adapters: &[GpuAdapter]) -> GpuCapability {
    if adapters.len() < 2 {
        return GpuCapability::SingleGpu;
    }
    let high = adapters
        .iter()
        .find(|a| !a.integrated)
        .map(|a| a.name.clone());
    let low = adapters
        .iter()
        .find(|a| a.integrated)
        .map(|a| a.name.clone());
    GpuCapability::Available {
        gpus: adapters
            .iter()
            .map(|a| GpuInfo {
                name: a.name.clone(),
            })
            .collect(),
        high,
        low,
    }
}

/// The `UserGpuPreferences` value data for a preference, or `None` for
/// `Auto` (which means "delete our entry / let Windows decide").
pub fn gpu_pref_value(pref: GpuPreference) -> Option<&'static str> {
    match pref {
        GpuPreference::Auto => None,
        GpuPreference::HighPerformance => Some("GpuPreference=2;"),
        GpuPreference::PowerSaving => Some("GpuPreference=1;"),
    }
}

/// Apply `pref` to `exe` in `HKCU\…\UserGpuPreferences` (Windows). Idempotent;
/// `Auto` deletes our value. Best-effort — returns the IO error so callers can
/// log; never panics. No-op (`Ok`) off Windows.
pub fn sync_for_exe(exe: &std::path::Path, pref: GpuPreference) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        win::sync(exe, gpu_pref_value(pref))
    }
    #[cfg(not(windows))]
    {
        let _ = (exe, pref);
        Ok(())
    }
}

#[cfg(windows)]
mod win {
    use std::io;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyW, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW,
        RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_SZ,
    };

    const SUBKEY: &str = "Software\\Microsoft\\DirectX\\UserGpuPreferences";

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn wide_os(p: &Path) -> Vec<u16> {
        p.as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    /// Open (or create) `HKCU\…\UserGpuPreferences`. Returns the open key or
    /// an IO error. The caller is responsible for calling `RegCloseKey`.
    unsafe fn open_or_create() -> io::Result<HKEY> {
        let subkey = wide(SUBKEY);
        let mut hkey: HKEY = std::ptr::null_mut();

        // Try to open first (no Win32_Security feature needed for RegOpenKeyExW).
        let rc = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            0,
            KEY_READ | KEY_WRITE,
            &mut hkey,
        );
        if rc == ERROR_SUCCESS {
            return Ok(hkey);
        }

        // Key does not exist yet — create it. RegCreateKeyW creates
        // all missing intermediate keys, opens with default access (KEY_ALL_ACCESS),
        // and does not require the Win32_Security feature.
        let rc = RegCreateKeyW(HKEY_CURRENT_USER, subkey.as_ptr(), &mut hkey);
        if rc == ERROR_SUCCESS {
            Ok(hkey)
        } else {
            Err(io::Error::from_raw_os_error(rc as i32))
        }
    }

    /// Write/delete the `UserGpuPreferences` value for `exe`. `value=None` → delete.
    pub fn sync(exe: &Path, value: Option<&str>) -> io::Result<()> {
        // SAFETY: standard Win32 registry FFI. All pointers are to locals that
        // outlive the calls; buffer sizes are passed in bytes as the API wants.
        // `open_or_create` returns a valid non-null HKEY on success.
        unsafe {
            let hkey = open_or_create()?;
            let name = wide_os(exe);
            let result = match value {
                None => {
                    let rc = RegDeleteValueW(hkey, name.as_ptr());
                    if rc == ERROR_SUCCESS || rc == ERROR_FILE_NOT_FOUND {
                        // Treating "value not found" as success: Auto = delete,
                        // and if it's already absent we're already in the desired state.
                        Ok(())
                    } else {
                        Err(io::Error::from_raw_os_error(rc as i32))
                    }
                }
                Some(v) => {
                    if current_equals(hkey, &name, v) {
                        // Already set to the correct value — skip the write (idempotent).
                        Ok(())
                    } else {
                        let data = wide(v);
                        // REG_SZ byte count includes the NUL terminator (already appended
                        // by `wide`), and the API wants the length in bytes (u16 * 2).
                        let bytes = (data.len() * 2) as u32;
                        let rc = RegSetValueExW(
                            hkey,
                            name.as_ptr(),
                            0,
                            REG_SZ,
                            data.as_ptr() as *const u8,
                            bytes,
                        );
                        if rc == ERROR_SUCCESS {
                            Ok(())
                        } else {
                            Err(io::Error::from_raw_os_error(rc as i32))
                        }
                    }
                }
            };
            RegCloseKey(hkey);
            result
        }
    }

    /// True iff the existing REG_SZ value for `name` already equals `v`.
    /// Returns `false` on any error (missing value, wrong type, buffer overflow).
    unsafe fn current_equals(hkey: HKEY, name: &[u16], v: &str) -> bool {
        let mut buf = [0u16; 256];
        let mut len = (buf.len() * 2) as u32;
        let rc = RegQueryValueExW(
            hkey,
            name.as_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            buf.as_mut_ptr() as *mut u8,
            &mut len,
        );
        if rc != ERROR_SUCCESS {
            return false;
        }
        // `len` is byte count of the returned data including the NUL terminator.
        // Subtract the terminator to get the string chars.
        let chars = (len as usize / 2).saturating_sub(1);
        let s = String::from_utf16_lossy(&buf[..chars]);
        s == v
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instances::schema::GpuPreference;

    fn a(name: &str, integrated: bool) -> GpuAdapter {
        GpuAdapter {
            name: name.into(),
            integrated,
        }
    }

    #[test]
    fn classify_zero_or_one_is_single_gpu() {
        assert_eq!(classify(&[]), GpuCapability::SingleGpu);
        assert_eq!(classify(&[a("Intel UHD", true)]), GpuCapability::SingleGpu);
    }

    #[test]
    fn classify_hybrid_pairs_high_and_low() {
        let cap = classify(&[a("NVIDIA RTX 3050 Ti", false), a("Intel UHD", true)]);
        match cap {
            GpuCapability::Available { gpus, high, low } => {
                assert_eq!(gpus.len(), 2);
                assert_eq!(high.as_deref(), Some("NVIDIA RTX 3050 Ti"));
                assert_eq!(low.as_deref(), Some("Intel UHD"));
            }
            other => panic!("expected Available, got {other:?}"),
        }
    }

    #[test]
    fn classify_two_discrete_has_high_no_low() {
        let cap = classify(&[a("RTX A", false), a("RTX B", false)]);
        match cap {
            GpuCapability::Available { high, low, .. } => {
                assert_eq!(high.as_deref(), Some("RTX A"));
                assert_eq!(low, None);
            }
            other => panic!("expected Available, got {other:?}"),
        }
    }

    #[test]
    fn gpu_capability_serializes_with_kind_tag() {
        let json = serde_json::to_string(&GpuCapability::Unsupported).unwrap();
        assert_eq!(json, r#"{"kind":"unsupported"}"#);
    }

    #[test]
    fn gpu_pref_value_maps_to_registry_string() {
        assert_eq!(gpu_pref_value(GpuPreference::Auto), None);
        assert_eq!(
            gpu_pref_value(GpuPreference::HighPerformance),
            Some("GpuPreference=2;")
        );
        assert_eq!(
            gpu_pref_value(GpuPreference::PowerSaving),
            Some("GpuPreference=1;")
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_registry_roundtrip_on_throwaway_value() {
        use std::path::PathBuf;
        let fake = PathBuf::from(r"C:\lucerna-test\zzz-gpu-roundtrip\javaw.exe");
        // Write HighPerformance → should set "GpuPreference=2;"
        super::sync_for_exe(&fake, GpuPreference::HighPerformance).unwrap();
        // Writing again (idempotent) must also succeed.
        super::win::sync(&fake, Some("GpuPreference=2;")).unwrap();
        // Auto = delete → must succeed (value exists).
        super::sync_for_exe(&fake, GpuPreference::Auto).unwrap();
        // Deleting again when already absent must also succeed (idempotent).
        super::sync_for_exe(&fake, GpuPreference::Auto).unwrap();
    }
}
