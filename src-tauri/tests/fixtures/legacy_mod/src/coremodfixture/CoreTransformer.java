package coremodfixture;

/** A class the plugin names that is NOT a container: it assigns no metadata. */
public class CoreTransformer {
    public byte[] transform(String name, String transformedName, byte[] basicClass) {
        return basicClass;
    }
}
