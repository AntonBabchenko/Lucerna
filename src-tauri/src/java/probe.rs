//! Parsing the answer of `java -XshowSettings:properties -version`.
//!
//! The JVM prints `Property settings:` followed by `    key = value` lines on
//! stderr, then its version banner. Multi-valued properties continue on
//! further-indented lines without ` = `; those are skipped. Only three keys
//! matter here: `java.version`, `os.arch`, and (optionally) `java.vendor`.

/// A JVM's architecture, normalised across the spellings JVMs use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Arch {
    X64,
    Aarch64,
    X86,
    Other(String),
}

impl Arch {
    pub fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "amd64" | "x86_64" | "x64" => Arch::X64,
            "aarch64" | "arm64" => Arch::Aarch64,
            "x86" | "i386" | "i486" | "i586" | "i686" => Arch::X86,
            other => Arch::Other(other.to_string()),
        }
    }

    /// A short label for the UI and the log ("x64", "aarch64", "x86").
    pub fn label(&self) -> String {
        match self {
            Arch::X64 => "x64".into(),
            Arch::Aarch64 => "aarch64".into(),
            Arch::X86 => "x86".into(),
            Arch::Other(s) => s.clone(),
        }
    }
}

/// The architecture of this launcher process — the one natives are extracted
/// for (`launch::spawn::current_arch`), so the one a JVM must match.
pub fn host_arch() -> Arch {
    if cfg!(target_arch = "x86_64") {
        Arch::X64
    } else if cfg!(target_arch = "aarch64") {
        Arch::Aarch64
    } else {
        Arch::X86
    }
}

/// The major of a `java.version` / `JAVA_VERSION` string: `1.8.0_402` → 8,
/// `17.0.9` → 17, `21` → 21, `25-ea` → 25. `None` for anything else.
pub fn parse_major(version: &str) -> Option<u32> {
    let v = version.trim().trim_matches('"');
    let leading = |s: &str| -> Option<u32> {
        let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
        digits.parse().ok()
    };
    match v.strip_prefix("1.") {
        Some(rest) => leading(rest),
        None => leading(v),
    }
    .filter(|m| *m > 0)
}

/// What a probe learned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JavaInfo {
    pub version: String,
    pub major: u32,
    pub vendor: Option<String>,
    pub arch: Arch,
}

/// Parse a probe's combined output. `Err` names what was missing, for the
/// "it did not say which Java it is" message.
pub fn parse_probe_output(text: &str) -> Result<JavaInfo, String> {
    let mut version = None;
    let mut arch = None;
    let mut vendor = None;
    for line in text.lines() {
        let Some((key, value)) = line.trim().split_once(" = ") else {
            continue;
        };
        match key.trim() {
            "java.version" => version = Some(value.trim().to_string()),
            "os.arch" => arch = Some(value.trim().to_string()),
            "java.vendor" => vendor = Some(value.trim().to_string()),
            _ => {}
        }
    }
    let version = version.ok_or_else(|| "no java.version in its answer".to_string())?;
    let major =
        parse_major(&version).ok_or_else(|| format!("unrecognised java.version \"{version}\""))?;
    let arch = arch.ok_or_else(|| "no os.arch in its answer".to_string())?;
    Ok(JavaInfo {
        version,
        major,
        vendor: vendor.filter(|v| !v.is_empty()),
        arch: Arch::parse(&arch),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const JAVA8: &str = "Property settings:\n    file.encoding = Cp1252\n    java.library.path = C:\\a\n        C:\\b\n    java.vendor = Temurin\n    java.version = 1.8.0_402\n    os.arch = amd64\n\nopenjdk version \"1.8.0_402\"\n";
    const JAVA21: &str = "Property settings:\n    java.vendor = Eclipse Adoptium\n    java.version = 21.0.4\n    os.arch = aarch64\n";

    #[test]
    fn majors_parse_across_the_old_and_new_schemes() {
        assert_eq!(parse_major("1.8.0_402"), Some(8));
        assert_eq!(parse_major("17.0.9"), Some(17));
        assert_eq!(parse_major("21"), Some(21));
        assert_eq!(parse_major("25-ea"), Some(25));
        assert_eq!(parse_major("\"11.0.20.1\""), Some(11));
        assert_eq!(parse_major("garbage"), None);
        assert_eq!(parse_major(""), None);
    }

    #[test]
    fn a_java_8_answer_parses() {
        let i = parse_probe_output(JAVA8).unwrap();
        assert_eq!(i.major, 8);
        assert_eq!(i.version, "1.8.0_402");
        assert_eq!(i.vendor.as_deref(), Some("Temurin"));
        assert_eq!(i.arch, Arch::X64);
    }

    #[test]
    fn a_java_21_answer_parses() {
        let i = parse_probe_output(JAVA21).unwrap();
        assert_eq!(i.major, 21);
        assert_eq!(i.arch, Arch::Aarch64);
    }

    #[test]
    fn an_answer_without_a_version_is_refused() {
        let e = parse_probe_output("Property settings:\n    os.arch = amd64\n").unwrap_err();
        assert!(e.contains("java.version"), "{e}");
    }

    #[test]
    fn an_answer_without_an_arch_is_refused() {
        let e = parse_probe_output("    java.version = 17.0.1\n").unwrap_err();
        assert!(e.contains("os.arch"), "{e}");
    }

    #[test]
    fn arches_normalise() {
        assert_eq!(Arch::parse("x86_64"), Arch::X64);
        assert_eq!(Arch::parse("arm64"), Arch::Aarch64);
        assert_eq!(Arch::parse("i386"), Arch::X86);
        assert_eq!(Arch::parse("ppc64le"), Arch::Other("ppc64le".into()));
    }
}
