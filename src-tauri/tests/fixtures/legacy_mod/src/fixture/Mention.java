package fixture;

import net.minecraftforge.fml.common.Mod;

/** Not a mod: it only MENTIONS the descriptor and the dependency syntax, as a library inspecting
 *  mods might. It passes the reader's substring pre-filter and must still yield nothing. */
public class Mention {
    public static final String DESCRIPTOR = "Lnet/minecraftforge/fml/common/Mod;";
    public static final String CLAUSE = "required-after:creativecore";

    @Mod.EventHandler
    public void handler(Object event) {}
}
