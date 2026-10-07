# Legacy `@Mod` class fixtures

Real class files for `mods::mod_annotation` and `tests/dependency_preflight.rs`. They are compiled,
not hand-written: a hand-made byte layout mirrors the reader's own assumptions, which is how the
printable-run bug survived (a `CONSTANT_Utf8`'s length byte is printable for a 33–126 byte string).

- `src/`: stub `@Mod` annotations with Forge's element names and retention — 1.12.2's
  `net.minecraftforge.fml.common.Mod` and 1.7.10's `cpw.mods.fml.common.Mod` — and the fixtures.
- `src-invisible/`: a CLASS-retention variant of the 1.12.2 stub, compiled separately, so its fixture
  carries `RuntimeInvisibleAnnotations`.
- `classes/`: the compiled fixtures. The stubs themselves are not kept.

Regenerate (Git Bash, from this directory, JDK 9+ on `PATH`):

```bash
rm -rf build classes && mkdir -p build/main build/invisible classes
javac --release 8 -d build/main $(find src -name '*.java')
javac --release 8 -d build/invisible $(find src-invisible -name '*.java')
cp -r build/main/fixture build/main/cpwfixture build/invisible/invisible classes/
rm -rf build
```

`--release 8` emits class-file major version 52, the version 1.12.2 mods ship.
