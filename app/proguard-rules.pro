-dontwarn org.bouncycastle.jsse.BCSSLParameters
-dontwarn org.bouncycastle.jsse.BCSSLSocket
-dontwarn org.bouncycastle.jsse.provider.BouncyCastleJsseProvider
-dontwarn org.conscrypt.Conscrypt$Version
-dontwarn org.conscrypt.Conscrypt
-dontwarn org.conscrypt.ConscryptHostnameVerifier
-dontwarn org.openjsse.javax.net.ssl.SSLParameters
-dontwarn org.openjsse.javax.net.ssl.SSLSocket
-dontwarn org.openjsse.net.ssl.OpenJSSE
-dontwarn java.beans.Introspector
-dontwarn java.beans.VetoableChangeListener
-dontwarn java.beans.VetoableChangeSupport
-dontwarn java.beans.BeanInfo
-dontwarn java.beans.IntrospectionException
-dontwarn java.beans.PropertyDescriptor

# Keep ini4j Service Provider Interface
-keep,allowobfuscation,allowoptimization class org.ini4j.spi.** { *; }

# Kotlin
-assumenosideeffects class kotlin.jvm.internal.Intrinsics {
    public static void check*(...);
    public static void throw*(...);
}

-repackageclasses
-allowaccessmodification
-overloadaggressively
-renamesourcefileattribute SourceFile

# JNI RegisterNatives looks up this class by name. R8 renaming it crashes onLoad.
-keep class me.ethereal.app.Natives { *; }
-keep class me.ethereal.app.Natives$Profile { *; }
-keep class me.ethereal.app.EtherealApplication { *; }
-keep class me.ethereal.app.ui.MainActivity { *; }
-keepclasseswithmembernames class * { native <methods>; }
-keep class androidx.core.app.CoreComponentFactory { *; }
-keep class androidx.core.app.CoreComponentFactory$Wrapper { *; }
