# SpecialSource bytecode fixtures

`Hello.class` and `HelloMethods.class` are minimal Java class files compiled
for the pure-Rust `remap_class_bytes` tests (Tasks 12-13).

These fixtures are unused since the 2026-05-17(b) shell-out pivot: the
`remap_class_bytes_*` and `remap_class_renames_a_method` tests were removed
because the pure-Rust bytecode walker is now `#[allow(dead_code)]` and cannot
achieve byte-fidelity against Java SpecialSource 1.11.0.

Retained here for future use if someone attempts a byte-faithful Rust port. To
revive: restore the tests from git history (commit before the shell-out pivot),
wire `remap_class_bytes` back into a pure-Rust `run()`, and compare output
against Java SpecialSource on `Hello.class` and `HelloMethods.class` to verify
byte identity.
