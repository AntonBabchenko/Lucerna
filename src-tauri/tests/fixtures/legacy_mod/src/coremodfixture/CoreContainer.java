package coremodfixture;

import net.minecraftforge.fml.common.DummyModContainer;
import net.minecraftforge.fml.common.ModMetadata;

/** Shaped like TNTUtils' TntuCoremodContainer: assigns its metadata in the constructor. */
public class CoreContainer extends DummyModContainer {
    public CoreContainer() {
        super(new ModMetadata());
        ModMetadata md = getMetadata();
        md.modId = "test_core";
        md.name = "Test Core";
        md.version = "1.2.3";
        md.description = "The core mod belonging to the test mod";
    }
}
