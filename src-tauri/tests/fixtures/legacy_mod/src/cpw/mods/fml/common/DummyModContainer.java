package cpw.mods.fml.common;

/** Stub of Forge 1.7.10's DummyModContainer. */
public class DummyModContainer {
    private final ModMetadata md;

    public DummyModContainer(ModMetadata md) {
        this.md = md;
    }

    public ModMetadata getMetadata() {
        return md;
    }
}
