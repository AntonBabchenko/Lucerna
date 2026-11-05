package coremodfixture;

import java.util.Map;
import net.minecraftforge.fml.relauncher.IFMLLoadingPlugin;

/** Shaped like TNTUtils' TntuPlugin: names its container by a string constant. */
public class CorePlugin implements IFMLLoadingPlugin {
    @Override
    public String[] getASMTransformerClass() {
        return new String[] {"coremodfixture.CoreTransformer"};
    }

    @Override
    public String getModContainerClass() {
        return "coremodfixture.CoreContainer";
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
