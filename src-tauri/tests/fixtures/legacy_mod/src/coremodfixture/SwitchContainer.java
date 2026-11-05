package coremodfixture;

import net.minecraftforge.fml.common.DummyModContainer;
import net.minecraftforge.fml.common.ModMetadata;

/**
 * Its constructor runs a tableswitch and a lookupswitch before the assignment, so the
 * instruction walk must step over their padding and jump tables to reach it.
 */
public class SwitchContainer extends DummyModContainer {
    public SwitchContainer(int dense, int sparse) {
        super(new ModMetadata());
        String tag;
        switch (dense) {
            case 1: tag = "a"; break;
            case 2: tag = "b"; break;
            case 3: tag = "c"; break;
            case 4: tag = "d"; break;
            default: tag = "z";
        }
        switch (sparse) {
            case 10: tag += "x"; break;
            case 1000: tag += "y"; break;
            case 100000: tag += "w"; break;
            default: break;
        }
        ModMetadata md = getMetadata();
        md.name = tag;
        md.modId = "switch_core";
    }
}
