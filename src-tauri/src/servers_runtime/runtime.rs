//! Долгоживущий серверный процесс: состояние, консоль-стрим, команды, стоп.
//! (Состояние/события/старт добавляются в следующих задачах Плана 2.)

use std::path::{Path, PathBuf};

/// Console JVM: on Windows `jre::java_executable_path` returns `javaw.exe`
/// (no console → no stdout). Servers stream stdout, so swap to `java.exe`.
pub(crate) fn console_java_path(javaw: &Path) -> PathBuf {
    match javaw.file_name().and_then(|n| n.to_str()) {
        Some("javaw.exe") => javaw.with_file_name("java.exe"),
        _ => javaw.to_path_buf(),
    }
}

/// MC version JSON `java_version.component`, else Mojang's legacy default.
pub(crate) fn java_component_or_legacy(component: Option<&str>) -> String {
    component
        .unwrap_or(crate::jre::DEFAULT_LEGACY_COMPONENT)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn console_java_swaps_javaw_to_java_on_windows() {
        assert_eq!(
            console_java_path(Path::new("/x/bin/javaw.exe")),
            Path::new("/x/bin/java.exe")
        );
        assert_eq!(
            console_java_path(Path::new("/x/bin/java")),
            Path::new("/x/bin/java")
        );
    }

    #[test]
    fn java_component_defaults_to_legacy_when_absent() {
        assert_eq!(java_component_or_legacy(None), "jre-legacy");
        assert_eq!(
            java_component_or_legacy(Some("java-runtime-delta")),
            "java-runtime-delta"
        );
    }
}
