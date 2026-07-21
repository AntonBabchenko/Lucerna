## What this changes

A short summary of the change and why.

## Related issue

Closes #

## Type

- [ ] feat
- [ ] fix
- [ ] refactor
- [ ] docs
- [ ] test
- [ ] chore / build / ci

## Checklist

- [ ] Branch is off `main`; commits follow Conventional Commits.
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml` passes.
- [ ] `pnpm typecheck`, `pnpm lint`, and `pnpm test` pass.
- [ ] No new outbound host except through `network::` + the allowlist (justified below if added).
- [ ] No new subprocess except through the `process::` module.
- [ ] No telemetry / analytics / client modification introduced.
- [ ] New dependency (if any) is justified: why, alternatives, dependency-tree impact.

## Notes for reviewers

Anything worth calling out — trade-offs, follow-ups, manual test steps.
