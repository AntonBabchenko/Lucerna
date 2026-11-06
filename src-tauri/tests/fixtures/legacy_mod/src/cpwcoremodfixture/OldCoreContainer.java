package cpwcoremodfixture;

import cpw.mods.fml.common.DummyModContainer;
import cpw.mods.fml.common.ModMetadata;

/** A 1.7.10 container: cpw.mods.fml's ModMetadata. */
public class OldCoreContainer extends DummyModContainer {
    public OldCoreContainer() {
        super(new ModMetadata());
        ModMetadata md = getMetadata();
        md.modId = "old_core";
        md.version = "0.9";
    }
}
