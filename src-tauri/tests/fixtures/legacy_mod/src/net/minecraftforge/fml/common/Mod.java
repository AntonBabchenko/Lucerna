package net.minecraftforge.fml.common;

import java.lang.annotation.ElementType;
import java.lang.annotation.Retention;
import java.lang.annotation.RetentionPolicy;
import java.lang.annotation.Target;

/** Test stub of Forge 1.12.2's @Mod: the element names and retention FML reads. */
@Retention(RetentionPolicy.RUNTIME)
@Target(ElementType.TYPE)
public @interface Mod {
    String modid();
    String name() default "";
    String version() default "";
    String dependencies() default "";
    boolean useMetadata() default false;
    String acceptedMinecraftVersions() default "";
    CustomProperty[] customProperties() default {};

    @Retention(RetentionPolicy.RUNTIME)
    @interface CustomProperty {
        String k();
        String v();
    }

    @Retention(RetentionPolicy.RUNTIME)
    @Target(ElementType.METHOD)
    @interface EventHandler {}
}
