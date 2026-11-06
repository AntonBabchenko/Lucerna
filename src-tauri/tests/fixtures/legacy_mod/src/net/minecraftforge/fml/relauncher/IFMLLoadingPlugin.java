package net.minecraftforge.fml.relauncher;

import java.util.Map;

/** Stub of Forge 1.12.2's coremod entry point (named by the jar's FMLCorePlugin attribute). */
public interface IFMLLoadingPlugin {
    String[] getASMTransformerClass();

    String getModContainerClass();

    String getSetupClass();

    void injectData(Map<String, Object> data);

    String getAccessTransformerClass();
}
