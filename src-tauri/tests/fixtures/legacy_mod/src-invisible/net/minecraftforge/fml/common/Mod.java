package net.minecraftforge.fml.common;

import java.lang.annotation.ElementType;
import java.lang.annotation.Retention;
import java.lang.annotation.RetentionPolicy;
import java.lang.annotation.Target;

/** CLASS retention: javac writes it to RuntimeInvisibleAnnotations, which FML's ASM visitor reads too. */
@Retention(RetentionPolicy.CLASS)
@Target(ElementType.TYPE)
public @interface Mod {
    String modid();
    String version() default "";
}
