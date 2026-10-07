package fixture;

import net.minecraftforge.fml.common.Mod;

/** OreLib's real @Mod shape, plus everything a reader must step over: long/double constants,
 *  a field, an annotated method, and @Marker BEFORE @Mod carrying every other element tag. */
@Marker(side = Side.CLIENT, type = String.class, numbers = {1, 2, 3}, big = 1234567890123L,
        ratio = 2.5, letter = 'x', tiny = 1, small = 2, part = 1.5f, flag = true,
        nested = @Mod.CustomProperty(k = "n", v = "m"))
@Mod(modid = "orelib", version = "3.6.0.1", useMetadata = true,
     dependencies = "required-after:forge@[14.23.5.2779,);",
     customProperties = {@Mod.CustomProperty(k = "a", v = "b")})
public class OreLib {
    public static final long BIG = 9876543210123L;
    public static final double PI = 3.14159;
    private int counter;

    @Mod.EventHandler
    public void init(Object event) {
        counter++;
    }
}
