package net.minecraftforge.fml.common;

/** Stub of Forge 1.12.2's DummyModContainer, the base of a coremod's mod container. */
public class DummyModContainer {
    private final ModMetadata md;

    public DummyModContainer(ModMetadata md) {
        this.md = md;
    }

    public ModMetadata getMetadata() {
        return md;
    }
}
