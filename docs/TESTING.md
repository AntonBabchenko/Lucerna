# Testing

Lucerna uses a layered test strategy. Each layer targets a different class of failure and runs in a different context. Knowing which layer to run when matters — the cross-loader matrix takes 1.5+ hours; the unit suite takes 100ms.

## Layers at a glance

| Layer | Location | `cargo test` includes by default | Run-time | Catches |
|---|---|---|---|---|
| Unit | `#[cfg(test)] mod tests` in every `src-tauri/src/**/*.rs` | yes | <1 s | logic bugs in pure functions, parsers, sort keys, dedup invariants |
| Integration | `src-tauri/tests/*_integration.rs` | yes | a few seconds | module-boundary regressions, wiremock'd network paths, fixture-driven parser checks |
| **Single-MC e2e** (`#[ignore]`) | `src-tauri/tests/forge_*_era_e2e.rs` | **no** | ~40 s + ~50 MB download | install-pipeline correctness for one era against the real reference Forge installer |
| **Loader matrix e2e** (`#[ignore]`) | `src-tauri/tests/loader_matrix_e2e.rs` | **no** | 17 min cached / ~3 h cold | cross-product regressions across MC versions × loaders; production library/launch path bugs that only surface for specific combos |
| UI typecheck + lint | `pnpm typecheck` / `pnpm lint` | n/a | ~10 s | TS errors, Svelte a11y warnings, no-network-call audit |
| Manual UI | `pnpm tauri dev` | n/a | minutes | UI plumbing (modal flows, error banner state, dropdown rendering) |

`cargo test` runs the first two layers fully. Everything else is on-demand.

## Running each layer

### Unit + integration (default sweep)

```powershell
cargo test --manifest-path src-tauri/Cargo.toml
```

Expected: 800+ lib tests + integration tests, all pass. Anything failing here is a regression — investigate before continuing.

### UI checks

```powershell
pnpm typecheck   # svelte-kit sync + svelte-check
pnpm lint        # Biome + Prettier (Svelte) + no-network audit
```

Pre-existing lint warnings exist in three files inherited from v0.1.0 (`PhaseStatusRow.svelte`, `ManageInstancesModal.svelte`, `+page.svelte`). If those are the *only* warnings, the lint baseline is intact.

### Single-MC e2e (on-demand)

Pinned reference installer per era. Validates the full install pipeline (forge processors + library fetch + JRE resolution) against the actual upstream artifact.

```powershell
# Legacy era (Forge 1.7.10) — no dedicated single-MC e2e target.
# The legacy installer path is covered by forge_legacy_era_integration.rs
# and forge_legacy_pipeline_integration.rs (both run by default) plus the
# loader matrix below.

# Transitional era (Forge 1.16.5-36.2.42)
cargo test --manifest-path src-tauri/Cargo.toml --test forge_transitional_era_e2e -- --ignored --nocapture

# Modern era (Forge 1.20.4-49.0.49)
cargo test --manifest-path src-tauri/Cargo.toml --test forge_modern_era_e2e -- --ignored --nocapture
```

Each prints `--- INSTALL SUCCEEDED ---` on success. On failure: read the `eprintln!` log + the per-combo log under `target/`. Phase 2/3 hot-fixes were predominantly caught here — `#[ignore]` doesn't mean optional, it means deliberate (slow, network-heavy).

Prerequisites: `pwsh src-tauri/tests/fixtures/forge/fetch.ps1` to download the SHA-pinned reference installers.

### Loader matrix e2e (on-demand, heavy)

The cross-product runner. Installs and launches every (MC, loader) combination, watches MC's `latest.log` for `Sound engine started`, kills on success, moves on.

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --test loader_matrix_e2e -- --ignored --nocapture
```

Override the default MC list to focus on one row (faster when debugging):

```powershell
$env:LUCERNA_MATRIX_MC = "1.20.4,1.21.11"
cargo test --manifest-path src-tauri/Cargo.toml --test loader_matrix_e2e -- --ignored --nocapture
```

Default matrix: 11 MC versions × applicable loaders (`MC_VERSIONS_DEFAULT` / `loaders_for` in `loader_matrix_e2e.rs`) ≈ 46 combos. Cached run ~17 min; cold ~3 h + ~10 GB disk + network.

Outputs:
- Console: progress line per combo + summary table at the end.
- `target/loader-matrix-logs/<combo>.log` — `INSTALL FAILED` text or `argv` summary.
- `target/loader-matrix-logs/<combo>.stdio.log` — child JVM stdout/stderr (useful when MC crashes early before `latest.log` is written).
- `<TEMP>/lucerna-matrix-data/` — sandbox data root. Persistent across runs so the second pass is mostly cache hits.

### Manual UI

Run the real launcher and click around. Reserve for changes affecting UI plumbing — backend correctness is covered by the matrix.

```powershell
pnpm tauri dev
```

## When to run what

| Change | Run before commit |
|---|---|
| Pure-Rust function or parser | unit |
| Module boundary or wiremock'd path | unit + relevant integration |
| Loader meta layer (Fabric/Quilt/Forge) | unit + integration + matrix subset for that loader |
| Install pipeline (forge installer, processors) | unit + integration + single-era e2e (the era you touched) |
| Anything that changes `versions::resolve`, `versions::libraries`, or `launch::args` | unit + integration + **matrix** (these touch every combo) |
| UI only | typecheck + lint + manual UI smoke |

## The `#[ignore]` gate pattern

Several tests are gated behind `#[ignore]` because they:
- Need network access.
- Need ~50 MB to ~10 GB disk.
- Need an installed JRE on PATH or system Java.
- Take minutes to hours.

CI doesn't run them by default. The maintainer runs them locally before merges that touch install/launch paths. Phase 2 and Phase 3 had a strict pre-merge protocol of "run the era-specific `_e2e` test green before squash". Phase 3 raised the bar to "the cross-loader matrix green".

## Test fixture rules

- **Forge installer fixtures** live in `src-tauri/tests/fixtures/forge/installers/` (gitignored). SHA-pinned in `SHA1SUMS`. Download via `pwsh fetch.ps1` from the same directory.
- **Bytecode golden fixtures** (small, checked-in) live under `src-tauri/tests/fixtures/specialsource/`. They're optional — tests skip cleanly if absent.

Adding a new fixture: append a SHA1SUMS row and verify `fetch.ps1` finds the upstream URL. The `<mc>-<fv>-<mc>` legacy quirk (1.7.10 et al) is hardcoded in `fetch.ps1`'s allowlist.
