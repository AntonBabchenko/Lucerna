//! GPU-preference OS divergence (Windows registry / Linux env / macOS none),
//! isolated behind the platform seam. Pure-fn cores (`classify`,
//! `gpu_pref_value`, `gpu_launch_env`) are unit-tested cross-platform; the
//! thin `#[cfg]` probes/appliers wrap them. See
//! docs/superpowers/specs/2026-06-12-gpu-selection-design.md.

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
