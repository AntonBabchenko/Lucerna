package coremodfixture;

import net.minecraftforge.fml.common.DummyModContainer;
import net.minecraftforge.fml.common.ModMetadata;

/** A container in the jar the plugin never names: FML never builds it, so it provides nothing. */
public class DecoyContainer extends DummyModContainer {
    public DecoyContainer() {
        super(new ModMetadata());
        getMetadata().modId = "decoy_core";
    }
}
