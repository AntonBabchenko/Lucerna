package coremodfixture;

import net.minecraftforge.fml.common.DummyModContainer;
import net.minecraftforge.fml.common.ModMetadata;

/** Its id depends on a branch: only one of the two strings sits beside the store. */
public class TernaryContainer extends DummyModContainer {
    public TernaryContainer(boolean legacy) {
        super(new ModMetadata());
        getMetadata().modId = legacy ? "ternary_old" : "ternary_new";
    }
}
