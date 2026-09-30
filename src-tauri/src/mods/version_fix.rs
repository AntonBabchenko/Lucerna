//! Two-sided fix for one version conflict (spec §5.4, D8): update the DEPENDENT
//! to a build that accepts the installed provider, or change the PROVIDER to a
//! build the dependent accepts — either way naming every other mod that change
//! would break.
//!
//! Pure over injected fetchers (the `depgraph`/`dep_project` shape): the command
//! supplies `platform_for(..).versions` and a `fetch_to_cache` + descriptor read,
//! so the whole policy is tested without a network. A fetch failure is an error,
//! never a silent `None` — "could not check" must not read as "no version".
//!
//! Every verdict goes through the pre-flight's own admission test
//! ([`active_deps_for`]) and range check, so a build this module offers is one
//! the pre-flight will pass on the conflicting id (audit A-F4). `breaks` is the
//! pre-flight itself, run over the instance as the switch would leave it
//! ([`newly_failed`]): it names exactly the other mods that would gain a
//! violation, on whichever id of the switched jar. What a candidate itself
//! declares beyond the conflicting id is not judged here; the gate re-runs the
//! pre-flight after any fix.

use std::collections::HashSet;
use std::future::Future;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::mods::local::{DependencyKind, ManifestDeps};
use crate::mods::platform::{ModSource, ModVersion};
use crate::mods::preflight::{
    active_deps_for, canon_id, provided_version, LooseJar, ParsedInstance, ParsedMod, ParsedRow,
    Violation,
};
use crate::mods::version_range::{compare_numeric, satisfies, Cmp, Satisfaction};

/// Jars one side may download before it gives up (spec §5.4).
const MAX_CANDIDATES: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ChangeDirection {
    Upgrade,
    Downgrade,
    /// A qualifier (rc/beta/snapshot) decides: the UI uses a neutral verb.
    Unknown,
}

/// A newer build of the dependent that accepts the installed provider.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct PlannedVersion {
    pub version: ModVersion,
    /// Registry names of the other enabled mods this build would newly fail —
    /// a range or incompatibility on what the dependent provides, or a
    /// requirement on what it would stop providing (D8).
    pub breaks: Vec<String>,
}

/// A provider build the dependent accepts.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ProviderChange {
    pub version: ModVersion,
    pub direction: ChangeDirection,
    /// Registry names of the other enabled mods this build would newly fail.
    pub breaks: Vec<String>,
}

/// Both sides `None`: an honest dead end — no build either side offers fixes
/// the conflict (or the conflict is already gone).
#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
pub struct VersionFixPlan {
    pub update_dependent: Option<PlannedVersion>,
    pub change_provider: Option<ProviderChange>,
}

/// How one jar's active declarations on an id judge a provider version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fit {
    Accepts,
    Rejects,
    Unknown,
}

/// Read through the pre-flight's own admission test. `Rejects` is exactly what
/// the pre-flight would flag; `Accepts` needs every declaration PROVABLY met —
/// an `Unknown` comparison is not a fix. A jar that declares nothing on the id
/// accepts any version of it.
fn fit(manifest: &ManifestDeps, dep_id: &str, version: &str, inst: &ParsedInstance) -> Fit {
    let key = canon_id(dep_id);
    let mut unknown = false;
    for d in active_deps_for(manifest, inst.loader, inst.era).filter(|d| canon_id(&d.dep_id) == key)
    {
        let s = satisfies(version, &d.range, d.family);
        // An incompatibility is the inverted test (`ModSorter.java:286-288`).
        let (ok, bad) = match d.kind {
            DependencyKind::Incompatible => (Satisfaction::Violated, Satisfaction::Satisfied),
            _ => (Satisfaction::Satisfied, Satisfaction::Violated),
        };
        if s == bad {
            return Fit::Rejects;
        }
        if s != ok {
            unknown = true;
        }
    }
    if unknown {
        Fit::Unknown
    } else {
        Fit::Accepts
    }
}

fn direction(to: &str, from: &str) -> ChangeDirection {
    match compare_numeric(to, from) {
        Cmp::Greater => ChangeDirection::Upgrade,
        Cmp::Less => ChangeDirection::Downgrade,
        // Equal cannot fix what the installed build breaks; Unknown is a
        // qualifier in a decisive position. Neither licenses a verb.
        Cmp::Equal | Cmp::Unknown => ChangeDirection::Unknown,
    }
}

/// A candidate's platform number turned into the jar's own version format, when
/// the installed build shows how the two relate: its number must contain its
/// descriptor version as whole tokens (`mc1.20.1-0.6.3` ⊃ `0.6.3`) and the
/// candidate's must carry the same prefix and suffix. `None` = no evidence, and
/// the caller reads the jar instead. Only ever used to SKIP a download, never to
/// pick a fix.
fn comparable_version(
    candidate: &str,
    installed_number: Option<&str>,
    installed_version: &str,
) -> Option<String> {
    let number = installed_number?;
    let version = installed_version.trim();
    if version.is_empty() {
        return None;
    }
    let at = number.rfind(version)?;
    let (prefix, suffix) = (&number[..at], &number[at + version.len()..]);
    // A digit or a dot against the match means it is a fragment of a longer
    // version (`1.0` inside `1.1.0`): no relation is proven, and a bogus guess
    // could skip the very build that fixes the conflict.
    let joins = |c: char| c.is_ascii_digit() || c == '.';
    if prefix.ends_with(joins) || suffix.starts_with(joins) {
        return None;
    }
    let middle = candidate.strip_prefix(prefix)?.strip_suffix(suffix)?;
    (!middle.is_empty()).then(|| middle.to_string())
}

/// A build the launcher could install — judged by the install pipeline's own
/// pre-I/O guard (safe filename, third-party distribution allowed, a SHA-1 to
/// verify by). Anything else could be neither verified nor installed, so it
/// costs no download and is never offered.
fn downloadable(v: &ModVersion) -> bool {
    crate::mods::install::guard_version(v).is_ok()
}

/// Only Modrinth and CurseForge list a mod's builds; a pack-managed row has
/// nothing to change to.
fn listed_identity(row: &ParsedRow) -> Option<(ModSource, String)> {
    match row.source? {
        s @ (ModSource::Modrinth | ModSource::Curseforge) => Some((s, row.project_id.clone()?)),
        _ => None,
    }
}

/// Plan both fixes for the conflict `dependent_sha1` has on `dep_id`
/// (`version_out_of_range`, `optional_out_of_range` or `incompatible_installed`).
///
/// `versions(source, project_id)` lists a project's builds newest-first, the
/// way `classify_update` reads them; `read_jar(build)` fetches one candidate and
/// reads its descriptors (`Ok(None)`: a zip that would not open). Either
/// failing fails the plan — including a platform-key error, which reaches the
/// caller typed.
pub async fn plan_version_fix<V, VFut, J, JFut>(
    inst: &ParsedInstance,
    dependent_sha1: &str,
    dep_id: &str,
    mut versions: V,
    mut read_jar: J,
) -> crate::error::Result<VersionFixPlan>
where
    V: FnMut(ModSource, String) -> VFut,
    VFut: Future<Output = crate::error::Result<Vec<ModVersion>>>,
    J: FnMut(ModVersion) -> JFut,
    JFut: Future<Output = crate::error::Result<Option<LooseJar>>>,
{
    let enabled = inst.registry_enabled();
    let key = canon_id(dep_id);
    // Re-derived from this parse, never trusted from the caller: the row the
    // user clicked may be stale. It is also what a switch's `breaks` is
    // measured against.
    let today = inst.resolve(&enabled);
    let installed = today.iter().find_map(|v| match v {
        Violation::VersionOutOfRange {
            dependent_sha1: d,
            dep_id: id,
            installed,
            ..
        }
        | Violation::OptionalOutOfRange {
            dependent_sha1: d,
            dep_id: id,
            installed,
            ..
        }
        | Violation::IncompatibleInstalled {
            dependent_sha1: d,
            dep_id: id,
            installed,
            ..
        } if d == dependent_sha1 && canon_id(id) == key => Some(installed.clone()),
        _ => None,
    });
    let (Some(installed), Some(dependent)) = (installed, inst.row(dependent_sha1)) else {
        return Ok(VersionFixPlan::default());
    };
    let update_dependent = dependent_side(
        inst,
        &today,
        dependent,
        dep_id,
        &installed,
        &mut versions,
        &mut read_jar,
    )
    .await?;
    let change_provider = provider_side(
        inst,
        &enabled,
        &today,
        dependent,
        dep_id,
        &installed,
        &mut versions,
        &mut read_jar,
    )
    .await?;
    Ok(VersionFixPlan {
        update_dependent,
        change_provider,
    })
}

/// The newest of at most [`MAX_CANDIDATES`] newer builds of the dependent whose
/// own declarations accept the installed provider version and that fails no
/// other enabled mod ([`newly_failed`]); failing that, the newest one that
/// accepts it, with `breaks` naming whom it would fail.
async fn dependent_side<V, VFut, J, JFut>(
    inst: &ParsedInstance,
    today: &[Violation],
    dependent: &ParsedRow,
    dep_id: &str,
    installed: &str,
    versions: &mut V,
    read_jar: &mut J,
) -> crate::error::Result<Option<PlannedVersion>>
where
    V: FnMut(ModSource, String) -> VFut,
    VFut: Future<Output = crate::error::Result<Vec<ModVersion>>>,
    J: FnMut(ModVersion) -> JFut,
    JFut: Future<Output = crate::error::Result<Option<LooseJar>>>,
{
    let (Some((source, project_id)), Some(current)) =
        (listed_identity(dependent), dependent.version_id.as_deref())
    else {
        return Ok(None);
    };
    let listed = versions(source, project_id).await?;
    // Newest-first: the builds above the installed one are the newer ones. An
    // installed build the listing lacks leaves "newer" unknowable, and an older
    // build is never offered as an update.
    let Some(at) = listed.iter().position(|v| v.version_id == current) else {
        return Ok(None);
    };
    let mut tried = 0;
    let mut best: Option<PlannedVersion> = None;
    for candidate in listed.into_iter().take(at).filter(downloadable) {
        if tried == MAX_CANDIDATES {
            break;
        }
        tried += 1;
        let Some(jar) = read_jar(candidate.clone()).await? else {
            continue; // an unreadable jar cannot be judged, so it is not offered
        };
        if fit(&jar.manifest, dep_id, installed, inst) != Fit::Accepts {
            continue;
        }
        let planned = PlannedVersion {
            breaks: newly_failed(inst, today, dependent, &jar),
            version: candidate,
        };
        if planned.breaks.is_empty() {
            return Ok(Some(planned));
        }
        // The newest verified build is the fallback; later ones never replace it.
        if best.is_none() {
            best = Some(planned);
        }
    }
    Ok(best)
}

/// The newest provider build the dependent accepts and that fails no other
/// enabled mod ([`newly_failed`]); failing that, the newest one the dependent
/// accepts, with `breaks` naming whom it would fail. At most
/// [`MAX_CANDIDATES`] downloads.
#[allow(clippy::too_many_arguments)]
async fn provider_side<V, VFut, J, JFut>(
    inst: &ParsedInstance,
    enabled: &HashSet<String>,
    today: &[Violation],
    dependent: &ParsedRow,
    dep_id: &str,
    installed: &str,
    versions: &mut V,
    read_jar: &mut J,
) -> crate::error::Result<Option<ProviderChange>>
where
    V: FnMut(ModSource, String) -> VFut,
    VFut: Future<Output = crate::error::Result<Vec<ModVersion>>>,
    J: FnMut(ModVersion) -> JFut,
    JFut: Future<Output = crate::error::Result<Option<LooseJar>>>,
{
    let Some(provider) = inst.provider_row(enabled, dep_id) else {
        return Ok(None); // only an embedded copy provides it: nothing to change
    };
    let Some((source, project_id)) = listed_identity(provider) else {
        return Ok(None);
    };
    let listed = versions(source, project_id).await?;
    let mut tried = 0;
    let mut best: Option<ProviderChange> = None;
    for candidate in listed.into_iter().filter(downloadable) {
        if provider.version_id.as_deref() == Some(candidate.version_id.as_str()) {
            continue;
        }
        let guess = comparable_version(
            &candidate.version_number,
            provider.version_number.as_deref(),
            installed,
        );
        if guess.is_some_and(|g| fit(&dependent.parsed.manifest, dep_id, &g, inst) == Fit::Rejects)
        {
            continue; // provably outside the dependent's range: not worth a download
        }
        if tried == MAX_CANDIDATES {
            break;
        }
        tried += 1;
        let Some(jar) = read_jar(candidate.clone()).await? else {
            continue; // an unreadable jar cannot be judged, so it is not offered
        };
        let Some(provided) = provided_version(&jar, dep_id, inst.loader, inst.era) else {
            continue; // this build does not answer for the id with a known version
        };
        if fit(&dependent.parsed.manifest, dep_id, &provided, inst) != Fit::Accepts {
            continue;
        }
        let change = ProviderChange {
            direction: direction(&provided, installed),
            breaks: newly_failed(inst, today, provider, &jar),
            version: candidate,
        };
        if change.breaks.is_empty() {
            return Ok(Some(change));
        }
        // The newest verified build is the fallback; later ones never replace it.
        if best.is_none() {
            best = Some(change);
        }
    }
    Ok(best)
}

/// Registry names of the enabled mods, other than `switched` itself, that the
/// pre-flight would newly flag once `switched` became the build `jar` — in
/// registry order, each once. Both sides' `breaks`.
///
/// It is the pre-flight ([`ParsedInstance::resolve`]) over the instance as the
/// switch would leave it ([`switched_to`]), so whatever the switch changes is
/// judged the way the loader judges it (audit A-F4): a range or incompatibility
/// on a version the new build provides, a requirement on an id it no longer
/// provides — and an embedded library that a top-level jar outranks changes
/// nobody's verdict. Newly: a mod the pre-flight flags on an id `today` is not
/// one the switch breaks on it. What `switched` itself would then lack is not a
/// mod it breaks — the gate's re-run pre-flight reports that.
fn newly_failed(
    inst: &ParsedInstance,
    today: &[Violation],
    switched: &ParsedRow,
    jar: &LooseJar,
) -> Vec<String> {
    // (mod, id) — one mod may be flagged on several ids.
    let on = |v: &Violation| (v.dependent_sha1().to_string(), canon_id(v.dep_id()));
    let flagged: HashSet<(String, String)> = today.iter().map(on).collect();
    let after = switched_to(inst, switched, jar);
    let mut named: HashSet<String> = HashSet::new();
    let mut breaks = Vec::new();
    for v in after.resolve(&after.registry_enabled()) {
        if v.dependent_sha1() == switched.parsed.sha1 || flagged.contains(&on(&v)) {
            continue;
        }
        if named.insert(v.dependent_sha1().to_string()) {
            breaks.push(v.dependent_name().to_string());
        }
    }
    breaks
}

/// The instance as switching `row` to another build would leave it: the same
/// registry rows, switches and order, with what the pre-flight reads of `row`
/// — its declarations and what it provides, embedded libraries included —
/// taken from `jar`. Every field is spelled out, so a field added to
/// `ParsedRow` has to be decided on here.
fn switched_to(inst: &ParsedInstance, row: &ParsedRow, jar: &LooseJar) -> ParsedInstance {
    let rows = inst
        .rows
        .iter()
        .map(|r| {
            if r.parsed.sha1 != row.parsed.sha1 {
                return r.clone();
            }
            ParsedRow {
                parsed: ParsedMod {
                    sha1: r.parsed.sha1.clone(),
                    name: r.parsed.name.clone(),
                    manifest: jar.manifest.clone(),
                },
                jij_provided: jar.jij_provided.clone(),
                enabled: r.enabled,
                source: r.source,
                project_id: r.project_id.clone(),
                version_id: r.version_id.clone(),
                version_number: r.version_number.clone(),
            }
        })
        .collect();
    ParsedInstance {
        rows,
        unreadable: inst.unreadable.clone(),
        loader: inst.loader,
        era: inst.era,
        mc: inst.mc.clone(),
        loader_version: inst.loader_version.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instances::schema::LoaderKind;
    use crate::mods::local::{DeclaredDep, DepSide, DescriptorEra, DescriptorSource, ProvidedMod};
    use crate::mods::platform::ModFile;
    use crate::mods::version_range::RangeFamily;
    use std::collections::HashMap;

    type Listed = std::future::Ready<crate::error::Result<Vec<ModVersion>>>;
    type Read = std::future::Ready<crate::error::Result<Option<LooseJar>>>;

    fn declares(id: &str, range: &str, kind: DependencyKind) -> DeclaredDep {
        DeclaredDep {
            dep_id: id.into(),
            range: range.into(),
            kind,
            side: DepSide::Both,
            family: RangeFamily::Maven,
            source: DescriptorSource::ModsToml,
        }
    }
    fn manifest(id: &str, version: &str, deps: Vec<DeclaredDep>) -> ManifestDeps {
        ManifestDeps {
            provided: vec![ProvidedMod {
                mod_id: id.into(),
                version: Some(version.into()),
                source: DescriptorSource::ModsToml,
            }],
            deps,
            sources_present: vec![DescriptorSource::ModsToml],
            platform: vec![],
        }
    }
    /// An enabled Modrinth row; its project id is `sha` upper-cased.
    fn row(sha: &str, vid: &str, number: &str, m: ManifestDeps) -> ParsedRow {
        ParsedRow {
            parsed: ParsedMod {
                sha1: sha.into(),
                name: sha.to_uppercase(),
                manifest: m,
            },
            jij_provided: vec![],
            enabled: true,
            source: Some(ModSource::Modrinth),
            project_id: Some(sha.to_uppercase()),
            version_id: Some(vid.into()),
            version_number: Some(number.into()),
        }
    }
    fn instance(rows: Vec<ParsedRow>) -> ParsedInstance {
        ParsedInstance {
            rows,
            unreadable: vec![],
            loader: LoaderKind::NeoForge,
            era: DescriptorEra::Modern,
            mc: "1.20.1".into(),
            loader_version: None,
        }
    }
    fn mv(project: &str, vid: &str, number: &str) -> ModVersion {
        ModVersion {
            source: ModSource::Modrinth,
            project_id: project.into(),
            version_id: vid.into(),
            name: number.into(),
            version_number: number.into(),
            mc_versions: vec!["1.20.1".into()],
            loaders: vec![LoaderKind::NeoForge],
            primary_file: ModFile {
                filename: format!("{vid}.jar"),
                url: format!("https://cdn.example/{vid}.jar"),
                sha1: Some("ab".repeat(20)),
                size: 1.0,
                distribution_allowed: true,
                sha256: None,
            },
            deps: vec![],
            published_at: None,
        }
    }
    fn jar(m: ManifestDeps) -> LooseJar {
        LooseJar {
            manifest: m,
            jij_provided: vec![],
        }
    }
    fn lister(
        map: HashMap<&'static str, Vec<ModVersion>>,
    ) -> impl FnMut(ModSource, String) -> Listed {
        move |_, pid| std::future::ready(Ok(map.get(pid.as_str()).cloned().unwrap_or_default()))
    }
    /// Indium needs Sodium in [0.5.0,0.6.0); 0.6.3 is installed. `other`: a
    /// third enabled mod's range on Sodium.
    fn sodium_conflict(other: Option<&str>) -> ParsedInstance {
        let req = |r: &str| vec![declares("sodium", r, DependencyKind::Required)];
        let mut rows = vec![
            row(
                "sod",
                "s063",
                "mc1.20.1-0.6.3",
                manifest("sodium", "0.6.3", vec![]),
            ),
            row(
                "ind",
                "i3",
                "1.0.3",
                manifest("indium", "1.0.3", req("[0.5.0,0.6.0)")),
            ),
        ];
        if let Some(r) = other {
            rows.push(row("oth", "o1", "1.0", manifest("other", "1.0", req(r))));
        }
        instance(rows)
    }
    fn needs_sodium(range: &str) -> LooseJar {
        jar(manifest(
            "indium",
            "x",
            vec![declares("sodium", range, DependencyKind::Required)],
        ))
    }
    fn sodium_builds() -> (
        HashMap<&'static str, Vec<ModVersion>>,
        HashMap<&'static str, LooseJar>,
    ) {
        let lists = HashMap::from([(
            "SOD",
            vec![
                mv("SOD", "s064", "mc1.20.1-0.6.4"),
                mv("SOD", "s063", "mc1.20.1-0.6.3"),
                mv("SOD", "s0512", "mc1.20.1-0.5.12"),
                mv("SOD", "s0511", "mc1.20.1-0.5.11"),
            ],
        )]);
        let jars = HashMap::from([
            ("s064", jar(manifest("sodium", "0.6.4", vec![]))),
            ("s0512", jar(manifest("sodium", "0.5.12", vec![]))),
            ("s0511", jar(manifest("sodium", "0.5.11", vec![]))),
        ]);
        (lists, jars)
    }

    #[tokio::test]
    async fn the_dependent_side_offers_the_first_newer_build_that_accepts_the_provider() {
        let inst = sodium_conflict(None);
        let lists = HashMap::from([(
            "IND",
            vec![
                mv("IND", "i5", "1.0.5"),
                mv("IND", "i4", "1.0.4"),
                mv("IND", "i3", "1.0.3"),
            ],
        )]);
        let jars = HashMap::from([
            ("i5", needs_sodium("[0.5.0,0.6.0)")),
            ("i4", needs_sodium("[0.6.0,)")),
        ]);
        let plan = plan_version_fix(&inst, "ind", "sodium", lister(lists), |v: ModVersion| {
            std::future::ready(Ok(jars.get(v.version_id.as_str()).cloned()))
        })
        .await
        .unwrap();
        assert_eq!(
            plan.update_dependent.map(|p| p.version.version_id),
            Some("i4".to_string())
        );
    }

    #[tokio::test]
    async fn the_dependent_side_never_offers_an_older_build_as_an_update() {
        let inst = sodium_conflict(None);
        let lists = HashMap::from([(
            "IND",
            vec![mv("IND", "i3", "1.0.3"), mv("IND", "i2", "1.0.2")],
        )]);
        let plan = plan_version_fix(
            &inst,
            "ind",
            "sodium",
            lister(lists),
            |v: ModVersion| -> Read {
                panic!("nothing newer exists, yet {} was downloaded", v.version_id)
            },
        )
        .await
        .unwrap();
        assert!(plan.update_dependent.is_none());
    }

    #[tokio::test]
    async fn the_dependent_side_downloads_at_most_three_candidates() {
        let inst = sodium_conflict(None);
        let ids = ["i9", "i8", "i7", "i6", "i3"];
        let lists = HashMap::from([("IND", ids.iter().map(|v| mv("IND", v, v)).collect())]);
        let mut downloaded: Vec<String> = Vec::new();
        let plan = plan_version_fix(&inst, "ind", "sodium", lister(lists), |v: ModVersion| {
            downloaded.push(v.version_id.clone());
            let fits = if v.version_id == "i6" {
                "[0.6.0,)"
            } else {
                "[0.5.0,0.6.0)"
            };
            std::future::ready(Ok(Some(needs_sodium(fits))))
        })
        .await
        .unwrap();
        assert!(
            plan.update_dependent.is_none(),
            "i6 fits but lies past the cap"
        );
        assert_eq!(downloaded, ["i9", "i8", "i7"]);
    }

    /// The install pipeline (`guard_version`) refuses a build with an unsafe
    /// filename, without a SHA-1, or with third-party distribution switched off,
    /// so the planner neither spends a download on one nor offers it.
    #[tokio::test]
    async fn a_build_the_launcher_could_not_install_is_neither_downloaded_nor_offered() {
        let inst = sodium_conflict(None);
        let mut unsafe_name = mv("IND", "i7", "1.0.7");
        unsafe_name.primary_file.filename = "../i7.jar".into();
        let mut no_digest = mv("IND", "i6", "1.0.6");
        no_digest.primary_file.sha1 = None;
        let mut not_distributable = mv("IND", "i5", "1.0.5");
        not_distributable.primary_file.distribution_allowed = false;
        let lists = HashMap::from([(
            "IND",
            vec![
                unsafe_name,
                no_digest,
                not_distributable,
                mv("IND", "i4", "1.0.4"),
                mv("IND", "i3", "1.0.3"),
            ],
        )]);
        let mut downloaded: Vec<String> = Vec::new();
        let plan = plan_version_fix(&inst, "ind", "sodium", lister(lists), |v: ModVersion| {
            downloaded.push(v.version_id.clone());
            std::future::ready(Ok(Some(needs_sodium("[0.6.0,)"))))
        })
        .await
        .unwrap();
        assert_eq!(downloaded, ["i4"]);
        assert_eq!(
            plan.update_dependent.map(|p| p.version.version_id),
            Some("i4".to_string())
        );
    }

    /// `sodium_conflict`, plus PIN: an enabled mod that requires Indium in
    /// `range`.
    fn indium_pinned(range: &str) -> ParsedInstance {
        let mut inst = sodium_conflict(None);
        let pin = manifest(
            "pin",
            "1.0",
            vec![declares("indium", range, DependencyKind::Required)],
        );
        inst.rows.push(row("pin", "p1", "1.0", pin));
        inst
    }
    /// An Indium build at `version` that accepts the installed Sodium 0.6.3.
    fn indium_build(version: &str) -> LooseJar {
        jar(manifest(
            "indium",
            version,
            vec![declares("sodium", "[0.6.0,)", DependencyKind::Required)],
        ))
    }
    /// A library embedded (Jar-in-Jar) in another jar.
    fn embedded(id: &str, version: &str) -> ProvidedMod {
        ProvidedMod {
            mod_id: id.into(),
            version: Some(version.into()),
            source: DescriptorSource::ModsToml,
        }
    }
    /// An enabled mod that requires the library `libx`.
    fn needs_libx(sha: &str) -> ParsedRow {
        let m = manifest(
            sha,
            "1.0",
            vec![declares("libx", "[1.0,)", DependencyKind::Required)],
        );
        row(sha, &format!("{sha}1"), "1.0", m)
    }

    /// D8: updating the dependent can push it out of ANOTHER mod's range on it —
    /// the offer says so, like a provider change does.
    #[tokio::test]
    async fn a_dependent_update_names_the_other_mod_it_would_fail() {
        // PIN accepts the installed Indium 1.0.3, not 1.0.5.
        let inst = indium_pinned("[1.0.0,1.0.5)");
        let lists = HashMap::from([(
            "IND",
            vec![mv("IND", "i5", "1.0.5"), mv("IND", "i3", "1.0.3")],
        )]);
        let jars = HashMap::from([("i5", indium_build("1.0.5"))]);
        let plan = plan_version_fix(&inst, "ind", "sodium", lister(lists), |v: ModVersion| {
            std::future::ready(Ok(jars.get(v.version_id.as_str()).cloned()))
        })
        .await
        .unwrap();
        let update = plan.update_dependent.expect("1.0.5 accepts Sodium 0.6.3");
        assert_eq!(update.version.version_id, "i5");
        assert_eq!(update.breaks, ["PIN"]);
    }

    /// Like the provider side: the newest build that fixes the conflict AND
    /// fails nobody wins over a newer one that fails someone.
    #[tokio::test]
    async fn the_dependent_side_prefers_a_newer_build_that_fails_no_other_mod() {
        let inst = indium_pinned("[1.0.0,1.0.5)");
        let lists = HashMap::from([(
            "IND",
            vec![
                mv("IND", "i5", "1.0.5"),
                mv("IND", "i4", "1.0.4"),
                mv("IND", "i3", "1.0.3"),
            ],
        )]);
        let jars = HashMap::from([("i5", indium_build("1.0.5")), ("i4", indium_build("1.0.4"))]);
        let plan = plan_version_fix(&inst, "ind", "sodium", lister(lists), |v: ModVersion| {
            std::future::ready(Ok(jars.get(v.version_id.as_str()).cloned()))
        })
        .await
        .unwrap();
        let update = plan
            .update_dependent
            .expect("1.0.4 fixes it and fails nobody");
        assert_eq!(update.version.version_id, "i4");
        assert!(update.breaks.is_empty(), "{:?}", update.breaks);
    }

    /// A mod the pre-flight flags on the dependent today is not one the update
    /// breaks: saying «breaks PIN» would blame the update for PIN's own state.
    #[tokio::test]
    async fn a_mod_already_flagged_on_the_dependent_is_not_one_its_update_breaks() {
        // PIN needs Indium 2.x: the installed 1.0.3 fails it already.
        let inst = indium_pinned("[2.0,)");
        let lists = HashMap::from([(
            "IND",
            vec![mv("IND", "i5", "1.0.5"), mv("IND", "i3", "1.0.3")],
        )]);
        let jars = HashMap::from([("i5", indium_build("1.0.5"))]);
        let plan = plan_version_fix(&inst, "ind", "sodium", lister(lists), |v: ModVersion| {
            std::future::ready(Ok(jars.get(v.version_id.as_str()).cloned()))
        })
        .await
        .unwrap();
        let update = plan.update_dependent.expect("1.0.5 accepts Sodium 0.6.3");
        assert!(update.breaks.is_empty(), "{:?}", update.breaks);
    }

    /// An id the new build stops providing breaks whoever requires it — here a
    /// library the installed Indium embeds and the new one does not.
    #[tokio::test]
    async fn a_dependent_update_that_drops_a_library_another_mod_needs_names_it() {
        let mut inst = sodium_conflict(None);
        inst.rows[1].jij_provided = vec![embedded("libx", "1.0")];
        inst.rows.push(needs_libx("pin"));
        let lists = HashMap::from([(
            "IND",
            vec![mv("IND", "i5", "1.0.5"), mv("IND", "i3", "1.0.3")],
        )]);
        let jars = HashMap::from([("i5", indium_build("1.0.5"))]);
        let plan = plan_version_fix(&inst, "ind", "sodium", lister(lists), |v: ModVersion| {
            std::future::ready(Ok(jars.get(v.version_id.as_str()).cloned()))
        })
        .await
        .unwrap();
        let update = plan.update_dependent.expect("1.0.5 accepts Sodium 0.6.3");
        assert_eq!(update.breaks, ["PIN"]);
    }

    #[tokio::test]
    async fn the_provider_side_picks_the_newest_harmless_build_and_skips_provable_misfits() {
        let inst = sodium_conflict(Some("[0.5.12,)"));
        let (lists, jars) = sodium_builds();
        let mut downloaded: Vec<String> = Vec::new();
        let plan = plan_version_fix(&inst, "ind", "sodium", lister(lists), |v: ModVersion| {
            downloaded.push(v.version_id.clone());
            std::future::ready(Ok(jars.get(v.version_id.as_str()).cloned()))
        })
        .await
        .unwrap();
        let change = plan
            .change_provider
            .expect("0.5.12 fits Indium and the other mod");
        assert_eq!(change.version.version_id, "s0512");
        assert_eq!(change.direction, ChangeDirection::Downgrade);
        assert!(change.breaks.is_empty());
        assert_eq!(
            downloaded,
            ["s0512"],
            "0.6.4 is provably outside Indium's range by number"
        );
    }

    #[tokio::test]
    async fn without_a_harmless_build_the_best_one_names_what_it_breaks() {
        let inst = sodium_conflict(Some("[0.5.13,)"));
        let (lists, jars) = sodium_builds();
        let plan = plan_version_fix(&inst, "ind", "sodium", lister(lists), |v: ModVersion| {
            std::future::ready(Ok(jars.get(v.version_id.as_str()).cloned()))
        })
        .await
        .unwrap();
        let change = plan
            .change_provider
            .expect("a build that fixes Indium exists");
        assert_eq!(change.version.version_id, "s0512");
        assert_eq!(change.breaks, ["OTH"]);
    }

    /// Not only the conflicting id: a provider build that no longer embeds a
    /// library another mod requires breaks that mod, and says so.
    #[tokio::test]
    async fn a_provider_change_that_drops_an_embedded_library_names_who_needs_it() {
        let mut inst = sodium_conflict(None);
        inst.rows[0].jij_provided = vec![embedded("libx", "1.0")];
        inst.rows.push(needs_libx("oth"));
        let (lists, jars) = sodium_builds();
        let plan = plan_version_fix(&inst, "ind", "sodium", lister(lists), |v: ModVersion| {
            std::future::ready(Ok(jars.get(v.version_id.as_str()).cloned()))
        })
        .await
        .unwrap();
        let change = plan.change_provider.expect("0.5.12 fixes Indium");
        assert_eq!(change.version.version_id, "s0512");
        assert_eq!(change.breaks, ["OTH"]);
    }

    fn create_clash(range: &str) -> ParsedInstance {
        let clash = vec![declares("create", range, DependencyKind::Incompatible)];
        instance(vec![
            row("cr", "c605", "6.0.5", manifest("create", "6.0.5", vec![])),
            row("ap", "a1", "1.0", manifest("asyncparticles", "1.0", clash)),
        ])
    }

    #[tokio::test]
    async fn an_incompatibility_is_fixed_against_the_negated_range() {
        let inst = create_clash("(,6.0.9]");
        let lists = HashMap::from([
            ("AP", vec![mv("AP", "a2", "1.1"), mv("AP", "a1", "1.0")]),
            (
                "CR",
                vec![
                    mv("CR", "c6010", "6.0.10"),
                    mv("CR", "c605", "6.0.5"),
                    mv("CR", "c603", "6.0.3"),
                ],
            ),
        ]);
        let narrower = vec![declares("create", "(,6.0.4]", DependencyKind::Incompatible)];
        let jars = HashMap::from([
            ("a2", jar(manifest("asyncparticles", "1.1", narrower))),
            ("c6010", jar(manifest("create", "6.0.10", vec![]))),
        ]);
        let plan = plan_version_fix(&inst, "ap", "create", lister(lists), |v: ModVersion| {
            std::future::ready(Ok(jars.get(v.version_id.as_str()).cloned()))
        })
        .await
        .unwrap();
        assert_eq!(
            plan.update_dependent.map(|p| p.version.version_id),
            Some("a2".to_string())
        );
        let change = plan.change_provider.expect("6.0.10 is outside (,6.0.9]");
        assert_eq!(
            (change.version.version_id.as_str(), change.direction),
            ("c6010", ChangeDirection::Upgrade)
        );
    }

    /// Audit A-F6: a bare Maven spec matches every version, so no provider build
    /// is "outside" it — only the dependent side could ever help.
    #[tokio::test]
    async fn a_bare_incompatible_range_has_no_provider_side_fix() {
        let inst = create_clash("6.0");
        let lists = HashMap::from([(
            "CR",
            vec![mv("CR", "c6010", "6.0.10"), mv("CR", "c605", "6.0.5")],
        )]);
        let jars = HashMap::from([("c6010", jar(manifest("create", "6.0.10", vec![])))]);
        let plan = plan_version_fix(&inst, "ap", "create", lister(lists), |v: ModVersion| {
            std::future::ready(Ok(jars.get(v.version_id.as_str()).cloned()))
        })
        .await
        .unwrap();
        assert!(plan.change_provider.is_none());
    }

    #[tokio::test]
    async fn no_current_violation_means_an_empty_plan_and_no_network() {
        let inst = sodium_conflict(None);
        let plan = plan_version_fix(
            &inst,
            "ind",
            "unrelated",
            |_: ModSource, _: String| -> Listed { panic!("no listing without a violation") },
            |_: ModVersion| -> Read { panic!("no download without a violation") },
        )
        .await
        .unwrap();
        assert!(plan.update_dependent.is_none() && plan.change_provider.is_none());
    }

    #[tokio::test]
    async fn a_failed_listing_is_an_error_not_a_dead_end() {
        let inst = sodium_conflict(None);
        let got = plan_version_fix(
            &inst,
            "ind",
            "sodium",
            |_: ModSource, _: String| -> Listed {
                std::future::ready(Err(crate::error::Error::io("<versions>", "offline")))
            },
            |_: ModVersion| -> Read { std::future::ready(Ok(None)) },
        )
        .await;
        assert!(
            got.is_err(),
            "\"could not check\" must not read as \"no version exists\""
        );
    }

    /// Spec §9: CurseForge without a key is the typed key error the UI already
    /// words — never "no version" — even when the other side had its answer.
    #[tokio::test]
    async fn a_missing_platform_key_reaches_the_caller_typed() {
        let inst = sodium_conflict(None);
        let got = plan_version_fix(
            &inst,
            "ind",
            "sodium",
            |_: ModSource, pid: String| -> Listed {
                std::future::ready(match pid.as_str() {
                    // The dependent side answers: nothing newer is listed.
                    "IND" => Ok(vec![mv("IND", "i3", "1.0.3")]),
                    _ => Err(crate::error::Error::ModsPlatformAuth {
                        kind: crate::error::ModsAuthKind::Missing,
                    }),
                })
            },
            |_: ModVersion| -> Read { std::future::ready(Ok(None)) },
        )
        .await;
        assert!(
            matches!(
                got,
                Err(crate::error::Error::ModsPlatformAuth {
                    kind: crate::error::ModsAuthKind::Missing
                })
            ),
            "{got:?}"
        );
    }

    #[test]
    fn direction_is_named_only_when_the_comparator_is_sure() {
        assert_eq!(direction("0.7.0", "0.6.3"), ChangeDirection::Upgrade);
        assert_eq!(direction("0.5.11", "0.6.3"), ChangeDirection::Downgrade);
        assert_eq!(
            direction("0.6.0-rc.1", "0.6.0-beta.2"),
            ChangeDirection::Unknown
        );
    }

    #[test]
    fn comparable_version_transfers_the_installed_builds_affixes() {
        let c = comparable_version;
        assert_eq!(
            c("mc1.20.1-0.5.11", Some("mc1.20.1-0.6.3"), "0.6.3").as_deref(),
            Some("0.5.11")
        );
        assert_eq!(
            c("1.20.1-1.21", Some("1.20.1-1.20"), "1.20").as_deref(),
            Some("1.21")
        );
        assert_eq!(
            c("0.5.12+mc1.20.1", Some("0.6.3+mc1.20.1"), "0.6.3").as_deref(),
            Some("0.5.12")
        );
        assert_eq!(c("0.5.11-fabric", Some("mc1.20.1-0.6.3"), "0.6.3"), None);
        assert_eq!(c("0.5.11", None, "0.6.3"), None);
    }

    /// The descriptor version must stand as whole tokens in the platform
    /// number. As a fragment of a longer version (`1.0` inside `1.1.0`, `0.6.3`
    /// inside `0.6.3.1`) it proves no relation, and a bogus guess could skip the
    /// very build that fixes the conflict.
    #[test]
    fn comparable_version_needs_the_descriptor_version_as_whole_tokens() {
        let c = comparable_version;
        assert_eq!(c("1.2.0", Some("1.1.0"), "1.0"), None);
        assert_eq!(c("0.6.4.1", Some("0.6.3.1"), "0.6.3"), None);
        assert_eq!(
            c("v0.5.11", Some("v0.6.3"), "0.6.3").as_deref(),
            Some("0.5.11")
        );
    }
}
