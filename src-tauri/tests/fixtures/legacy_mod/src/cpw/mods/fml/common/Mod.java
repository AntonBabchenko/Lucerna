package cpw.mods.fml.common;

import java.lang.annotation.ElementType;
import java.lang.annotation.Retention;
import java.lang.annotation.RetentionPolicy;
import java.lang.annotation.Target;

/** Test stub of Forge 1.7.10's @Mod: the element names and retention FML reads. */
@Retention(RetentionPolicy.RUNTIME)
@Target(ElementType.TYPE)
public @interface Mod {
    String modid();
    String name() default "";
    String version() default "";
    String dependencies() default "";
    boolean useMetadata() default false;
}
