//! GPU-preference OS divergence (Windows registry / Linux env / macOS none),
//! isolated behind the platform seam. Pure-fn cores (`classify`,
//! `gpu_pref_field`, `fields`, `gpu_launch_env`) are unit-tested cross-platform; the
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
    /// The probe could not run — NOT "one GPU"; the UI says it could not tell.
    Unknown { details: String },
}

/// How this OS steers a spawned game's GPU. Lets the page describe the
/// mechanism truthfully — including for a stored choice on a system where it
/// does nothing.
#[derive(Debug, Clone, Copy, Serialize, Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GpuMechanism {
    WindowsRegistry,
    LinuxEnv,
    None,
}

#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq)]
pub struct GpuStatus {
    pub mechanism: GpuMechanism,
    pub capability: GpuCapability,
}

pub fn mechanism() -> GpuMechanism {
    #[cfg(windows)]
    {
        GpuMechanism::WindowsRegistry
    }
    #[cfg(target_os = "linux")]
    {
        GpuMechanism::LinuxEnv
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        GpuMechanism::None
    }
}

pub fn status() -> GpuStatus {
    GpuStatus {
        mechanism: mechanism(),
        capability: capability(),
    }
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

/// The `UserGpuPreferences` value is a `key=value;` list. Lucerna owns ONE
/// field of it (`GpuPreference`) and preserves every other — Windows 11 keeps
/// its "Optimizations for windowed games" toggle in the same value, as
/// `SwapEffectUpgradeEnable=1;`.
pub mod fields {
    /// The value of `key`, or `None` when the field is absent.
    pub fn get<'a>(value: &'a str, key: &str) -> Option<&'a str> {
        value
            .split(';')
            .filter_map(|pair| pair.split_once('='))
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v)
    }

    /// `value` with `key` set to `field`: replaced in place when present,
    /// appended otherwise. Every other field is kept byte for byte.
    pub fn set(value: &str, key: &str, field: &str) -> String {
        let mut out = String::new();
        let mut replaced = false;
        for pair in value.split(';').filter(|p| !p.is_empty()) {
            match pair.split_once('=') {
                Some((k, _)) if k == key => {
                    out.push_str(&format!("{key}={field};"));
                    replaced = true;
                }
                _ => {
                    out.push_str(pair);
                    out.push(';');
                }
            }
        }
        if !replaced {
            out.push_str(&format!("{key}={field};"));
        }
        out
    }

    /// `value` without `key`; empty when nothing is left.
    pub fn remove(value: &str, key: &str) -> String {
        let mut out = String::new();
        for pair in value.split(';').filter(|p| !p.is_empty()) {
            if matches!(pair.split_once('='), Some((k, _)) if k == key) {
                continue;
            }
            out.push_str(pair);
            out.push(';');
        }
        out
    }
}

/// The `GpuPreference` field Lucerna writes for a preference, or `None` for
/// `Auto` — which means "nothing to write", never "delete".
pub fn gpu_pref_field(pref: GpuPreference) -> Option<&'static str> {
    match pref {
        GpuPreference::Auto => None,
        GpuPreference::HighPerformance => Some("2"),
        GpuPreference::PowerSaving => Some("1"),
    }
}

/// The OS's per-exe GPU preference store, behind a trait so `gpu_pref` can be
/// exercised on every OS against a fake. `read` tells absent from unreadable;
/// nothing here creates the key except `write`.
pub trait GpuRegistry {
    fn read(&self, exe: &std::path::Path) -> std::io::Result<Option<String>>;
    fn write(&self, exe: &std::path::Path, value: &str) -> std::io::Result<()>;
    fn delete(&self, exe: &std::path::Path) -> std::io::Result<()>;
}

/// The real store: the per-user `UserGpuPreferences` key on Windows; reads
/// as absent and writes nothing anywhere else (Linux steers through the
/// child's environment, macOS has no mechanism).
pub struct OsRegistry;

impl GpuRegistry for OsRegistry {
    fn read(&self, exe: &std::path::Path) -> std::io::Result<Option<String>> {
        #[cfg(windows)]
        {
            win::read(exe)
        }
        #[cfg(not(windows))]
        {
            let _ = exe;
            Ok(None)
        }
    }

    fn write(&self, exe: &std::path::Path, value: &str) -> std::io::Result<()> {
        #[cfg(windows)]
        {
            win::write(exe, value)
        }
        #[cfg(not(windows))]
        {
            let _ = (exe, value);
            Ok(())
        }
    }

    fn delete(&self, exe: &std::path::Path) -> std::io::Result<()> {
        #[cfg(windows)]
        {
            win::delete(exe)
        }
        #[cfg(not(windows))]
        {
            let _ = exe;
            Ok(())
        }
    }
}

/// Env vars to inject into the Minecraft child for `pref`, given whether the
/// proprietary NVIDIA stack is present. Pure → unit-tested on every OS. Only
/// `HighPerformance` offloads; `Auto`/`PowerSaving` keep the default (iGPU).
pub fn gpu_launch_env(pref: GpuPreference, nvidia_present: bool) -> Vec<(String, String)> {
    if pref != GpuPreference::HighPerformance {
        return Vec::new();
    }
    if nvidia_present {
        vec![
            ("__NV_PRIME_RENDER_OFFLOAD".into(), "1".into()),
            ("__GLX_VENDOR_LIBRARY_NAME".into(), "nvidia".into()),
            ("__VK_LAYER_NV_optimus".into(), "NVIDIA_only".into()),
        ]
    } else {
        vec![("DRI_PRIME".into(), "1".into())]
    }
}

/// OS wrapper the launch path calls. Linux computes `nvidia_present` from
/// `/proc/driver/nvidia`; every other OS returns no env.
pub fn launch_env(pref: GpuPreference) -> Vec<(String, String)> {
    #[cfg(target_os = "linux")]
    {
        let nvidia = std::path::Path::new("/proc/driver/nvidia").exists();
        gpu_launch_env(pref, nvidia)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = pref;
        Vec::new()
    }
}

/// Probe the machine and classify GPU-selection capability.
/// macOS → `Unsupported`; Linux reads `/sys/class/drm`; Windows enumerates the
/// display-adapter registry class key. A probe that cannot run is `Unknown`
/// — never folded into "one GPU" — and never a panic.
pub fn capability() -> GpuCapability {
    #[cfg(target_os = "macos")]
    {
        GpuCapability::Unsupported
    }
    #[cfg(target_os = "linux")]
    {
        capability_from_probe(linux_adapters())
    }
    #[cfg(target_os = "windows")]
    {
        capability_from_probe(win::adapters())
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        GpuCapability::Unsupported
    }
}

/// A probe that could not run is `Unknown`; an empty list is a real "one GPU".
pub fn capability_from_probe(probe: std::io::Result<Vec<GpuAdapter>>) -> GpuCapability {
    match probe {
        Ok(adapters) => classify(&adapters),
        Err(e) => GpuCapability::Unknown {
            details: e.to_string(),
        },
    }
}

/// Enumerate GPU adapters from `/sys/class/drm` on Linux.
/// Each `card<N>` directory represents one GPU; vendor IDs and `boot_vga`
/// determine whether it is integrated.
#[cfg(target_os = "linux")]
fn linux_adapters() -> std::io::Result<Vec<GpuAdapter>> {
    // Each /sys/class/drm/card<N>/device/{vendor,boot_vga}. vendor is the PCI
    // vendor id; boot_vga==1 marks the integrated/primary GPU. An unreadable
    // class dir is "could not tell", not "no GPU".
    let mut out = Vec::new();
    let entries = std::fs::read_dir("/sys/class/drm")?;
    for e in entries.flatten() {
        let name = e.file_name();
        let name = name.to_string_lossy();
        // cardN only (skip cardN-eDP-1 connector dirs).
        if !(name.starts_with("card")
            && name.len() > 4
            && name[4..].chars().all(|c| c.is_ascii_digit()))
        {
            continue;
        }
        let dev = e.path().join("device");
        // Display only: an unreadable vendor file lists the card as unknown,
        // not as nothing — the count still decides single vs. dual GPU.
        let vendor = std::fs::read_to_string(dev.join("vendor")).map(|s| s.trim().to_owned());
        let integrated = std::fs::read_to_string(dev.join("boot_vga"))
            .map(|s| s.trim() == "1")
            .unwrap_or(false)
            || vendor
                .as_deref()
                .is_ok_and(|v| v.eq_ignore_ascii_case("0x8086"));
        let label = match vendor.as_deref() {
            Ok("0x10de") => "NVIDIA GPU".to_owned(),
            Ok("0x1002") => "AMD GPU".to_owned(),
            Ok("0x8086") => "Intel GPU".to_owned(),
            Ok(other) => other.to_owned(),
            Err(_) => "Unknown GPU".to_owned(),
        };
        out.push(GpuAdapter {
            name: label,
            integrated,
        });
    }
    Ok(out)
}

#[cfg(windows)]
mod win {
    use std::io;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use windows_sys::Win32::Foundation::{
        ERROR_FILE_NOT_FOUND, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS, FILETIME,
    };
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyW, RegDeleteValueW, RegEnumKeyExW, RegOpenKeyExW,
        RegQueryValueExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ,
        KEY_WRITE, REG_SZ,
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
    /// Open the key with `access` WITHOUT creating it: `Ok(None)` when it
    /// does not exist. A read must never create.
    unsafe fn open(access: u32) -> io::Result<Option<HKEY>> {
        let subkey = wide(SUBKEY);
        let mut hkey: HKEY = std::ptr::null_mut();
        let rc = RegOpenKeyExW(HKEY_CURRENT_USER, subkey.as_ptr(), 0, access, &mut hkey);
        if rc == ERROR_SUCCESS {
            Ok(Some(hkey))
        } else if rc == ERROR_FILE_NOT_FOUND {
            Ok(None)
        } else {
            Err(io::Error::from_raw_os_error(rc as i32))
        }
    }

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

    /// A REG_SZ value by (wide) name: `Ok(None)` when absent, `Err` when it
    /// could not be read — the two are never folded together.
    unsafe fn query_sz(hkey: HKEY, name: &[u16]) -> io::Result<Option<String>> {
        let mut buf = [0u16; 512];
        let mut len = (buf.len() * 2) as u32; // bytes
        let rc = RegQueryValueExW(
            hkey,
            name.as_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            buf.as_mut_ptr() as *mut u8,
            &mut len,
        );
        if rc == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        if rc != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(rc as i32));
        }
        // `len` counts bytes including the NUL terminator; drop the terminator.
        let chars = (len as usize / 2).saturating_sub(1);
        Ok(Some(String::from_utf16_lossy(&buf[..chars])))
    }

    /// The whole value for `exe`; `None` when the key or the value is absent.
    pub fn read(exe: &Path) -> io::Result<Option<String>> {
        // SAFETY: standard Win32 registry FFI; pointers are to locals that
        // outlive the calls; the key is closed before returning.
        unsafe {
            let Some(hkey) = open(KEY_READ)? else {
                return Ok(None);
            };
            let name = wide_os(exe);
            let result = query_sz(hkey, &name);
            RegCloseKey(hkey);
            result
        }
    }

    /// Write the whole value for `exe` (creating the key if needed — the
    /// only primitive that does).
    pub fn write(exe: &Path, value: &str) -> io::Result<()> {
        // SAFETY: as in `read`; `data` outlives the call; the byte count
        // includes the NUL terminator `wide` appends.
        unsafe {
            let hkey = open_or_create()?;
            let name = wide_os(exe);
            let data = wide(value);
            let bytes = (data.len() * 2) as u32;
            let rc = RegSetValueExW(
                hkey,
                name.as_ptr(),
                0,
                REG_SZ,
                data.as_ptr() as *const u8,
                bytes,
            );
            RegCloseKey(hkey);
            if rc == ERROR_SUCCESS {
                Ok(())
            } else {
                Err(io::Error::from_raw_os_error(rc as i32))
            }
        }
    }

    /// Delete the value for `exe`; an absent key or value is already done.
    pub fn delete(exe: &Path) -> io::Result<()> {
        // SAFETY: as in `read`.
        unsafe {
            let Some(hkey) = open(KEY_WRITE)? else {
                return Ok(());
            };
            let name = wide_os(exe);
            let rc = RegDeleteValueW(hkey, name.as_ptr());
            RegCloseKey(hkey);
            if rc == ERROR_SUCCESS || rc == ERROR_FILE_NOT_FOUND {
                Ok(())
            } else {
                Err(io::Error::from_raw_os_error(rc as i32))
            }
        }
    }

    const DISPLAY_CLASS: &str =
        "SYSTEM\\CurrentControlSet\\Control\\Class\\{4d36e968-e325-11ce-bfc1-08002be10318}";

    /// Enumerate installed display adapters from the registry class key. Lists
    /// ALL adapters (including a display-less discrete GPU on Optimus), unlike
    /// EnumDisplayDevices. A class key or an adapter subkey that cannot be
    /// opened, or an enumeration that stops on anything but "no more items",
    /// is `Err` — one unreadable adapter on a dual-GPU box must not read as
    /// "single GPU".
    pub fn adapters() -> io::Result<Vec<super::GpuAdapter>> {
        let mut out: Vec<super::GpuAdapter> = Vec::new();
        // SAFETY: standard registry enumeration. Buffers are sized per the API
        // (char counts for key names); all pointers are to locals that outlive
        // each call; every opened key is closed before the function returns.
        unsafe {
            let class = wide(DISPLAY_CLASS);
            let mut hkey: HKEY = std::ptr::null_mut();
            let rc = RegOpenKeyExW(HKEY_LOCAL_MACHINE, class.as_ptr(), 0, KEY_READ, &mut hkey);
            if rc != ERROR_SUCCESS {
                return Err(io::Error::from_raw_os_error(rc as i32));
            }
            let mut idx = 0u32;
            loop {
                let mut name_buf = [0u16; 256];
                let mut name_len = name_buf.len() as u32; // in CHARS
                                                          // lpftLastWriteTime accepts NULL — pass a null pointer via FILETIME.
                let rc = RegEnumKeyExW(
                    hkey,
                    idx,
                    name_buf.as_mut_ptr(),
                    &mut name_len,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut::<FILETIME>(),
                );
                if rc == ERROR_NO_MORE_ITEMS {
                    break;
                }
                if rc != ERROR_SUCCESS {
                    RegCloseKey(hkey);
                    return Err(io::Error::from_raw_os_error(rc as i32));
                }
                idx += 1;
                let sub = String::from_utf16_lossy(&name_buf[..name_len as usize]);
                // Adapters are 4-digit numeric subkeys (0000, 0001, …)
                if sub.len() != 4 || !sub.chars().all(|c| c.is_ascii_digit()) {
                    continue;
                }
                let subpath = wide(&format!("{DISPLAY_CLASS}\\{sub}"));
                let mut subkey: HKEY = std::ptr::null_mut();
                let rc = RegOpenKeyExW(
                    HKEY_LOCAL_MACHINE,
                    subpath.as_ptr(),
                    0,
                    KEY_READ,
                    &mut subkey,
                );
                if rc != ERROR_SUCCESS {
                    RegCloseKey(hkey);
                    return Err(io::Error::from_raw_os_error(rc as i32));
                }
                let matching = read_sz(subkey, "MatchingDeviceId").unwrap_or_default();
                let desc = read_sz(subkey, "DriverDesc").unwrap_or_default();
                RegCloseKey(subkey);
                let mu = matching.to_ascii_uppercase();
                // Real PCI GPUs only (filters software/virtual/mirror adapters).
                if !mu.contains("PCI") || desc.is_empty() {
                    continue;
                }
                let integrated =
                    mu.contains("VEN_8086") || desc.to_ascii_lowercase().contains("intel");
                if !out.iter().any(|g| g.name == desc) {
                    out.push(super::GpuAdapter {
                        name: desc,
                        integrated,
                    });
                }
            }
            RegCloseKey(hkey);
        }
        Ok(out)
    }

    /// Read a REG_SZ value by name; `None` on any error (display-only callers).
    unsafe fn read_sz(hkey: HKEY, value: &str) -> Option<String> {
        let name = wide(value);
        query_sz(hkey, &name).ok().flatten()
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
    fn gpu_pref_field_is_the_windows_field_value_or_nothing() {
        assert_eq!(gpu_pref_field(GpuPreference::Auto), None);
        assert_eq!(gpu_pref_field(GpuPreference::HighPerformance), Some("2"));
        assert_eq!(gpu_pref_field(GpuPreference::PowerSaving), Some("1"));
    }

    #[test]
    fn fields_get_finds_one_field_among_others() {
        assert_eq!(
            fields::get(
                "GpuPreference=2;SwapEffectUpgradeEnable=1;",
                "GpuPreference"
            ),
            Some("2")
        );
        assert_eq!(
            fields::get("SwapEffectUpgradeEnable=1;", "GpuPreference"),
            None
        );
        assert_eq!(fields::get("", "GpuPreference"), None);
    }

    #[test]
    fn fields_set_replaces_in_place_and_keeps_every_other_field() {
        assert_eq!(
            fields::set(
                "GpuPreference=1;SwapEffectUpgradeEnable=1;",
                "GpuPreference",
                "2"
            ),
            "GpuPreference=2;SwapEffectUpgradeEnable=1;"
        );
        assert_eq!(
            fields::set("SwapEffectUpgradeEnable=1;", "GpuPreference", "2"),
            "SwapEffectUpgradeEnable=1;GpuPreference=2;"
        );
        assert_eq!(fields::set("", "GpuPreference", "2"), "GpuPreference=2;");
    }

    #[test]
    fn fields_remove_leaves_the_rest_or_nothing() {
        assert_eq!(
            fields::remove(
                "GpuPreference=2;SwapEffectUpgradeEnable=1;",
                "GpuPreference"
            ),
            "SwapEffectUpgradeEnable=1;"
        );
        assert_eq!(fields::remove("GpuPreference=2;", "GpuPreference"), "");
        assert_eq!(
            fields::remove("SwapEffectUpgradeEnable=1;", "GpuPreference"),
            "SwapEffectUpgradeEnable=1;"
        );
    }

    #[test]
    fn a_failed_probe_is_unknown_not_single_gpu() {
        let cap = capability_from_probe(Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "class key",
        )));
        assert!(
            matches!(cap, GpuCapability::Unknown { ref details } if details.contains("class key")),
            "got {cap:?}"
        );
        assert_eq!(capability_from_probe(Ok(vec![])), GpuCapability::SingleGpu);
    }

    #[test]
    fn gpu_status_serializes_the_mechanism_snake_case() {
        let json = serde_json::to_string(&GpuStatus {
            mechanism: GpuMechanism::WindowsRegistry,
            capability: GpuCapability::Unknown {
                details: "x".into(),
            },
        })
        .unwrap();
        assert_eq!(
            json,
            r#"{"mechanism":"windows_registry","capability":{"kind":"unknown","details":"x"}}"#
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_registry_primitives_on_a_throwaway_value() {
        use std::path::PathBuf;
        let fake = PathBuf::from(r"C:\lucerna-test\zzz-gpu-primitives\javaw.exe");
        let reg = OsRegistry;
        // Deleting what is absent is already done; reading it is None, not an error.
        reg.delete(&fake).unwrap();
        assert_eq!(reg.read(&fake).unwrap(), None);
        reg.write(&fake, "GpuPreference=1;SwapEffectUpgradeEnable=1;")
            .unwrap();
        assert_eq!(
            reg.read(&fake).unwrap().as_deref(),
            Some("GpuPreference=1;SwapEffectUpgradeEnable=1;")
        );
        reg.delete(&fake).unwrap();
        assert_eq!(reg.read(&fake).unwrap(), None);
    }

    #[test]
    fn launch_env_empty_unless_high_performance() {
        assert!(gpu_launch_env(GpuPreference::Auto, true).is_empty());
        assert!(gpu_launch_env(GpuPreference::PowerSaving, true).is_empty());
    }

    #[test]
    fn launch_env_high_nvidia_sets_prime_vars() {
        let env = gpu_launch_env(GpuPreference::HighPerformance, true);
        assert!(env
            .iter()
            .any(|(k, v)| k == "__NV_PRIME_RENDER_OFFLOAD" && v == "1"));
        assert!(env
            .iter()
            .any(|(k, v)| k == "__GLX_VENDOR_LIBRARY_NAME" && v == "nvidia"));
        assert!(env.iter().any(|(k, _)| k == "__VK_LAYER_NV_optimus"));
    }

    #[test]
    fn launch_env_high_mesa_sets_dri_prime() {
        let env = gpu_launch_env(GpuPreference::HighPerformance, false);
        assert_eq!(env, vec![("DRI_PRIME".to_string(), "1".to_string())]);
    }

    #[test]
    fn capability_returns_a_valid_variant() {
        let cap = capability();
        // Must be one of the known variants; on Windows/Linux it is Single/Available,
        // on macOS Unsupported. Just assert it doesn't panic and is constructible.
        match cap {
            GpuCapability::Unsupported
            | GpuCapability::SingleGpu
            | GpuCapability::Unknown { .. }
            | GpuCapability::Available { .. } => {}
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn capability_on_windows_is_not_unsupported() {
        assert!(!matches!(capability(), GpuCapability::Unsupported));
    }
}
