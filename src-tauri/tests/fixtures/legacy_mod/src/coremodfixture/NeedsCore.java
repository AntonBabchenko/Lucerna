package coremodfixture;

import net.minecraftforge.fml.common.Mod;

/** Shaped like TNTUtils' @Mod: requires the id its own jar's coremod container provides. */
@Mod(modid = "needs_core", version = "1.0", dependencies = "required-after:test_core")
public class NeedsCore {}
