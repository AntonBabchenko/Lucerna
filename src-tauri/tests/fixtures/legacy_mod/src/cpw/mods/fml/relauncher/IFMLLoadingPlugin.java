package cpw.mods.fml.relauncher;

import java.util.Map;

/** Stub of Forge 1.7.10's coremod entry point. */
public interface IFMLLoadingPlugin {
    String[] getASMTransformerClass();

    String getModContainerClass();

    String getSetupClass();

    void injectData(Map<String, Object> data);

    String getAccessTransformerClass();
}
