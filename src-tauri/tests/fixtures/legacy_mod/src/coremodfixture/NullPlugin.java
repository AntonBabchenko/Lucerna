package coremodfixture;

import java.util.Map;
import net.minecraftforge.fml.relauncher.IFMLLoadingPlugin;

/** Registers no container, though the jar holds one and mentions it elsewhere. */
public class NullPlugin implements IFMLLoadingPlugin {
    private static final String UNUSED = "coremodfixture.CoreContainer";

    @Override
    public String[] getASMTransformerClass() {
        return new String[] {UNUSED};
    }

    @Override
    public String getModContainerClass() {
        return null;
    }

    @Override
    public String getSetupClass() {
        return null;
    }

    @Override
    public void injectData(Map<String, Object> data) {}

    @Override
    public String getAccessTransformerClass() {
        return null;
    }
}
