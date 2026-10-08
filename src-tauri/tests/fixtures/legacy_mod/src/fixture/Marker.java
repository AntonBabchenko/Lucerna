package fixture;

import java.lang.annotation.ElementType;
import java.lang.annotation.Retention;
import java.lang.annotation.RetentionPolicy;
import java.lang.annotation.Target;
import net.minecraftforge.fml.common.Mod;

/** A second class-level annotation whose values use every element tag @Mod does not. */
@Retention(RetentionPolicy.RUNTIME)
@Target(ElementType.TYPE)
public @interface Marker {
    Side side();
    Class<?> type();
    int[] numbers();
    long big();
    double ratio();
    char letter();
    byte tiny();
    short small();
    float part();
    boolean flag();
    Mod.CustomProperty nested();
}
