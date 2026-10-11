//! Javas installed on this machine (spec §5). Reads directories, the PATH,
//! JAVA_HOME and — on Windows — the registry; spawns nothing. Each candidate
//! is described from the `release` file in its Java home. Picking one runs
//! the real probe, so a wrong `release` file costs a refusal, not a launch.

use super::probe::parse_major;
use serde::Serialize;
use specta::Type;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum DetectSource {
    JavaHome,
    Path,
    Registry,
    KnownDir,
    UserDir,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct DetectedJava {
    /// The executable that would be stored if picked.
    pub path: String,
    pub home: String,
    pub major: Option<u32>,
    pub version: Option<String>,
    pub vendor: Option<String>,
    pub arch: Option<String>,
    pub source: DetectSource,
}

/// Where to look. Built by `default_roots` for this machine, by tests for a
/// temp tree.
#[derive(Debug, Default)]
pub struct Roots {
    /// Java homes named outright (JAVA_HOME, registry).
    pub homes: Vec<(PathBuf, DetectSource)>,
    /// Directories that may hold a `java` executable (PATH entries).
    pub bin_dirs: Vec<PathBuf>,
    /// Directories whose children are Java homes, macOS bundles, or Homebrew
    /// kegs.
    pub parents: Vec<(PathBuf, DetectSource)>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ReleaseInfo {
    pub version: Option<String>,
    pub vendor: Option<String>,
    pub arch: Option<String>,
}

/// `KEY="value"` lines of a Java home's `release` file.
pub fn parse_release(text: &str) -> ReleaseInfo {
    let mut r = ReleaseInfo::default();
    for line in text.lines() {
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let v = v.trim().trim_matches('"').to_string();
        if v.is_empty() {
            continue;
        }
        match k.trim() {
            "JAVA_VERSION" => r.version = Some(v),
            "IMPLEMENTOR" => r.vendor = Some(v),
            "OS_ARCH" => r.arch = Some(v),
            _ => {}
        }
    }
    r
}

#[cfg(windows)]
const EXE_NAMES: &[&str] = &["javaw.exe", "java.exe"];
#[cfg(not(windows))]
const EXE_NAMES: &[&str] = &["java"];

/// The executable a home would be stored as: the first of `EXE_NAMES` present.
/// A home also needs its `lib/`: that is what tells a Java home from a
/// directory that merely holds a `bin/java` — the launcher stubs Oracle puts
/// in `Common Files\Oracle\Java\javapath`, or a wrapper script under `/opt`.
fn stored_exe(home: &Path) -> Option<PathBuf> {
    if !home.join("lib").is_dir() {
        return None;
    }
    EXE_NAMES
        .iter()
        .map(|n| home.join("bin").join(n))
        .find(|p| p.is_file())
}

/// The candidate homes a child of a parent directory may hold: itself, a
/// macOS `.jdk` bundle's home, or a Homebrew keg's.
fn homes_in(child: &Path) -> [PathBuf; 3] {
    [
        child.to_path_buf(),
        child.join("Contents").join("Home"),
        child
            .join("libexec")
            .join("openjdk.jdk")
            .join("Contents")
            .join("Home"),
    ]
}

fn home_of_bin_dir(dir: &Path) -> Option<PathBuf> {
    // macOS's /usr/bin/java is a stub that opens a system "install Java"
    // dialog when no JDK is installed; real JDKs are found under
    // /Library/Java/JavaVirtualMachines instead.
    if cfg!(target_os = "macos") && dir == Path::new("/usr/bin") {
        return None;
    }
    let exe = EXE_NAMES
        .iter()
        .map(|n| dir.join(n))
        .find(|p| p.is_file())?;
    // Canonicalise the EXECUTABLE so a symlink chain (/usr/bin/java →
    // /etc/alternatives/java → /usr/lib/jvm/…/bin/java) lands on its home.
    let real = dunce::canonicalize(&exe).ok()?;
    Some(real.parent()?.parent()?.to_path_buf())
}

fn describe(home: &Path, source: DetectSource) -> Option<DetectedJava> {
    let exe = stored_exe(home)?;
    let release = match std::fs::read_to_string(home.join("release")) {
        Ok(text) => parse_release(&text),
        // A display hint only: the version shows as unknown in the list, and
        // the probe decides when the entry is picked.
        Err(e) => {
            crate::diag!("java detect: no release file in {}: {e}", home.display());
            ReleaseInfo::default()
        }
    };
    Some(DetectedJava {
        path: exe.to_string_lossy().into_owned(),
        home: home.to_string_lossy().into_owned(),
        major: release.version.as_deref().and_then(parse_major),
        version: release.version,
        vendor: release.vendor,
        arch: release.arch,
        source,
    })
}

/// Every Java under `roots`, each home once, none under `exclude` (Lucerna's
/// own `jres/`, which the UI offers as "Lucerna's Java").
pub fn collect(roots: &Roots, exclude: &Path) -> Vec<DetectedJava> {
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut out = Vec::new();
    let mut consider = |home: &Path, source: DetectSource| {
        let Ok(home) = dunce::canonicalize(home) else {
            return;
        };
        if home.starts_with(exclude) || !seen.insert(home.clone()) {
            return;
        }
        if let Some(found) = describe(&home, source) {
            out.push(found);
        }
    };
    for (home, source) in &roots.homes {
        consider(home, *source);
    }
    for dir in &roots.bin_dirs {
        if let Some(home) = home_of_bin_dir(dir) {
            consider(&home, DetectSource::Path);
        }
    }
    for (parent, source) in &roots.parents {
        let entries = match std::fs::read_dir(parent) {
            Ok(entries) => entries,
            // Most known folders simply do not exist on a given machine.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                // Said, so an empty list can be told from a list that could
                // not be read; Browse still works either way.
                crate::diag!("java detect: cannot read {}: {e}", parent.display());
                continue;
            }
        };
        for entry in entries.flatten() {
            if let Some(home) = homes_in(&entry.path())
                .into_iter()
                .find(|h| stored_exe(h).is_some())
            {
                consider(&home, *source);
            }
        }
    }
    out
}

fn env_dir(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// Where Javas live on this machine.
pub fn default_roots() -> Roots {
    let mut r = Roots::default();
    if let Some(h) = env_dir("JAVA_HOME") {
        r.homes.push((h, DetectSource::JavaHome));
    }
    if let Some(p) = std::env::var_os("PATH") {
        r.bin_dirs.extend(std::env::split_paths(&p));
    }
    let user_home = env_dir(if cfg!(windows) { "USERPROFILE" } else { "HOME" });
    if let Some(home) = &user_home {
        r.parents.push((home.join(".jdks"), DetectSource::UserDir));
        r.parents.push((
            home.join(".sdkman").join("candidates").join("java"),
            DetectSource::UserDir,
        ));
    }
    #[cfg(windows)]
    {
        for h in crate::platform::java_registry::java_homes() {
            r.homes.push((h, DetectSource::Registry));
        }
        let bases: Vec<PathBuf> = [
            env_dir("ProgramFiles"),
            env_dir("ProgramFiles(x86)"),
            env_dir("LOCALAPPDATA").map(|l| l.join("Programs")),
        ]
        .into_iter()
        .flatten()
        .collect();
        for base in bases {
            for vendor in [
                "Java",
                "Eclipse Adoptium",
                "Eclipse Foundation",
                "AdoptOpenJDK",
                "Zulu",
                "Microsoft",
                "BellSoft",
                "Amazon Corretto",
            ] {
                r.parents.push((base.join(vendor), DetectSource::KnownDir));
            }
        }
    }
    #[cfg(target_os = "linux")]
    for d in ["/usr/lib/jvm", "/usr/lib64/jvm", "/usr/java", "/opt"] {
        r.parents.push((PathBuf::from(d), DetectSource::KnownDir));
    }
    #[cfg(target_os = "macos")]
    {
        // Oracle's Java 8 JRE installs as a browser plug-in, not a .jdk bundle.
        r.homes.push((
            PathBuf::from("/Library/Internet Plug-Ins/JavaAppletPlugin.plugin/Contents/Home"),
            DetectSource::KnownDir,
        ));
        r.parents.push((
            PathBuf::from("/Library/Java/JavaVirtualMachines"),
            DetectSource::KnownDir,
        ));
        if let Some(home) = &user_home {
            r.parents.push((
                home.join("Library")
                    .join("Java")
                    .join("JavaVirtualMachines"),
                DetectSource::UserDir,
            ));
        }
        r.parents
            .push((PathBuf::from("/opt/homebrew/opt"), DetectSource::KnownDir));
        r.parents
            .push((PathBuf::from("/usr/local/opt"), DetectSource::KnownDir));
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_home(root: &Path, name: &str, release: Option<&str>) -> PathBuf {
        let home = root.join(name);
        std::fs::create_dir_all(home.join("bin")).unwrap();
        std::fs::create_dir_all(home.join("lib")).unwrap();
        std::fs::write(home.join("bin").join(EXE_NAMES[0]), b"").unwrap();
        if let Some(r) = release {
            std::fs::write(home.join("release"), r).unwrap();
        }
        home
    }

    #[test]
    fn release_files_parse() {
        let r = parse_release(
            "IMPLEMENTOR=\"Eclipse Adoptium\"\nJAVA_VERSION=\"17.0.9\"\nOS_ARCH=\"amd64\"\nBAD LINE\n",
        );
        assert_eq!(r.version.as_deref(), Some("17.0.9"));
        assert_eq!(r.vendor.as_deref(), Some("Eclipse Adoptium"));
        assert_eq!(r.arch.as_deref(), Some("amd64"));
    }

    #[test]
    fn a_parent_dir_yields_each_home_once_with_its_release() {
        let t = tempfile::tempdir().unwrap();
        let jvm = t.path().join("jvm");
        let a = fake_home(
            &jvm,
            "jdk-21",
            Some("JAVA_VERSION=\"21.0.4\"\nIMPLEMENTOR=\"Eclipse Adoptium\"\n"),
        );
        fake_home(&jvm, "jre8", None);
        std::fs::create_dir_all(jvm.join("not-a-java")).unwrap();
        let roots = Roots {
            homes: vec![(a.clone(), DetectSource::JavaHome)],
            bin_dirs: vec![],
            parents: vec![(jvm.clone(), DetectSource::KnownDir)],
        };
        let found = collect(&roots, &t.path().join("jres"));
        assert_eq!(found.len(), 2, "{found:?}");
        let jdk21 = found.iter().find(|j| j.major == Some(21)).unwrap();
        assert_eq!(
            jdk21.source,
            DetectSource::JavaHome,
            "the first sighting wins"
        );
        let jre8 = found.iter().find(|j| j.major.is_none()).unwrap();
        assert_eq!(
            jre8.version, None,
            "no release file: version unknown, still listed"
        );
    }

    #[test]
    fn a_bin_java_without_a_java_home_around_it_is_not_listed() {
        // Oracle's `javapath` launcher stubs: an executable with no `lib/`.
        let t = tempfile::tempdir().unwrap();
        let stub = t.path().join("javapath");
        std::fs::create_dir_all(stub.join("bin")).unwrap();
        std::fs::write(stub.join("bin").join(EXE_NAMES[0]), b"").unwrap();
        let roots = Roots {
            homes: vec![(stub.clone(), DetectSource::JavaHome)],
            bin_dirs: vec![stub.join("bin")],
            parents: vec![(t.path().to_path_buf(), DetectSource::KnownDir)],
        };
        assert!(collect(&roots, &t.path().join("jres")).is_empty());
    }

    #[test]
    fn lucernas_own_runtimes_are_excluded() {
        let t = tempfile::tempdir().unwrap();
        let jres = t.path().join("jres");
        fake_home(
            &jres,
            "java-runtime-delta",
            Some("JAVA_VERSION=\"21.0.3\"\n"),
        );
        let roots = Roots {
            parents: vec![(jres.clone(), DetectSource::KnownDir)],
            ..Roots::default()
        };
        let exclude = dunce::canonicalize(&jres).unwrap();
        assert!(collect(&roots, &exclude).is_empty());
    }

    #[test]
    fn a_path_entry_finds_its_home() {
        let t = tempfile::tempdir().unwrap();
        let home = fake_home(t.path(), "jdk-17", Some("JAVA_VERSION=\"17.0.9\"\n"));
        let roots = Roots {
            bin_dirs: vec![home.join("bin"), t.path().join("nothing-here")],
            ..Roots::default()
        };
        let found = collect(&roots, &t.path().join("jres"));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].source, DetectSource::Path);
        assert_eq!(found[0].major, Some(17));
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_java_is_the_same_home() {
        let t = tempfile::tempdir().unwrap();
        let home = fake_home(t.path(), "jdk-21", Some("JAVA_VERSION=\"21\"\n"));
        let bin = t.path().join("usrbin");
        std::fs::create_dir_all(&bin).unwrap();
        std::os::unix::fs::symlink(home.join("bin/java"), bin.join("java")).unwrap();
        let roots = Roots {
            homes: vec![(home.clone(), DetectSource::JavaHome)],
            bin_dirs: vec![bin],
            parents: vec![],
        };
        assert_eq!(collect(&roots, &t.path().join("jres")).len(), 1);
    }
}
