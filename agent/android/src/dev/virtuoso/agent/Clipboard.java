package dev.virtuoso.agent;

import android.content.ClipData;
import android.os.IBinder;
import java.lang.reflect.Method;

/**
 * The device clipboard, through the clipboard service as the shell package. Its methods'
 * parameters change between Android versions, so they are filled by type: the clip, the
 * calling package, then nulls for further strings and 0 (user 0, the default device) for ints.
 */
final class Clipboard {
    private static final String PACKAGE = "com.android.shell";

    private final Object service;

    Clipboard() throws Exception {
        IBinder binder = (IBinder) Class.forName("android.os.ServiceManager").getMethod("getService", String.class).invoke(null, "clipboard");
        service = Class.forName("android.content.IClipboard$Stub").getMethod("asInterface", IBinder.class).invoke(null, binder);
    }

    void set(String text) throws Exception {
        call("setPrimaryClip", ClipData.newPlainText("virtuoso", text));
    }

    String get() throws Exception {
        ClipData clip = (ClipData) call("getPrimaryClip", null);
        if (clip == null || clip.getItemCount() == 0) return "";
        CharSequence t = clip.getItemAt(0).getText();
        return t == null ? "" : t.toString();
    }

    private Object call(String name, ClipData clip) throws Exception {
        for (Method m : service.getClass().getMethods()) {
            if (!m.getName().equals(name)) continue;
            Class<?>[] types = m.getParameterTypes();
            Object[] args = new Object[types.length];
            boolean packageGiven = false;
            for (int i = 0; i < types.length; i++) {
                if (types[i] == ClipData.class) {
                    args[i] = clip;
                } else if (types[i] == String.class) {
                    args[i] = packageGiven ? null : PACKAGE;
                    packageGiven = true;
                } else if (types[i] == int.class) {
                    args[i] = 0;
                }
            }
            return m.invoke(service, args);
        }
        throw new NoSuchMethodException("IClipboard." + name);
    }
}
