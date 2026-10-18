# Mojang-mappings sanitize fixtures

`synthetic.proguard` is a made-up ProGuard mappings file. Every name is invented, and Mojang's mappings are not redistributed. It covers:
- comments, including an inline one;
- a few CRLF line ends;
- an inner class;
- constructors and `<clinit>`;
- primitive, array and object descriptors;
- a method repeated with two line ranges, plus inlined ranges (`:30:31`);
- overloads of one name whose `java.util.HashMap` bucket order differs from their file order.

`synthetic.tsrg` is what **srgutils 0.5.10** writes for it, the srgutils version Forge's `installertools` 1.4.3 uses for `DOWNLOAD_MOJMAPS --sanitize`. It was generated once with:

```java
import net.minecraftforge.srgutils.IMappingFile;
import java.io.FileInputStream;
import java.io.InputStream;
import java.nio.file.Paths;

public class GenTsrg {
    public static void main(String[] a) throws Exception {
        try (InputStream in = new FileInputStream(a[0])) {
            IMappingFile.load(in).write(Paths.get(a[1]), IMappingFile.Format.TSRG, false);
        }
    }
}
```

```text
javac -cp srgutils-0.5.10.jar GenTsrg.java
java -cp "srgutils-0.5.10.jar;." GenTsrg synthetic.proguard synthetic.tsrg
```

The srgutils jar comes from `https://maven.minecraftforge.net/net/minecraftforge/srgutils/0.5.10/srgutils-0.5.10.jar`.

`forge::patcher::mojmaps::proguard_to_tsrg` must reproduce `synthetic.tsrg` byte for byte (see `docs/superpowers/specs/2026-10-16-forge-classpath-and-processors-design.md` §5.2). If you change `synthetic.proguard`, regenerate `synthetic.tsrg` with srgutils. Never edit it by hand.
