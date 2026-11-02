package dev.virtuoso.agent;

import java.lang.reflect.InvocationTargetException;

final class Util {
    private Util() {}

    /** Reflection wraps the real error; logs and replies should show that one. */
    static Throwable cause(Throwable t) {
        while (t instanceof InvocationTargetException && t.getCause() != null) t = t.getCause();
        return t;
    }

    static void sleep(long ms) {
        if (ms <= 0) return;
        try {
            Thread.sleep(ms);
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
        }
    }
}
