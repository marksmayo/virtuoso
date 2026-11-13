package dev.virtuoso.agent;

import android.accessibilityservice.AccessibilityServiceInfo;
import android.app.UiAutomation;
import android.os.HandlerThread;
import android.os.Looper;
import android.view.accessibility.AccessibilityEvent;

/**
 * Started by the host as the shell user:
 *
 *   CLASSPATH=/data/local/tmp/virtuoso-agent-<hash>.jar app_process / dev.virtuoso.agent.Main <port> <token>
 *
 * The shell user may inject input straight into the input manager and read every window's
 * accessibility tree through a UiAutomation built the way `uiautomator dump` builds one. Only
 * one UiAutomation may exist on a device at a time: another test runner's agent must be stopped.
 */
public final class Main implements Runnable {
    /**
     * The events that change what the agent reads (Android's
     * AccessibilityCache.CACHE_CRITICAL_EVENTS_MASK). UiAutomation's default is every type,
     * which makes each app build and send clicks, hovers and announcements nobody reads.
     */
    static final int EVENT_TYPES = AccessibilityEvent.TYPE_VIEW_ACCESSIBILITY_FOCUSED
            | AccessibilityEvent.TYPE_VIEW_ACCESSIBILITY_FOCUS_CLEARED
            | AccessibilityEvent.TYPE_VIEW_FOCUSED
            | AccessibilityEvent.TYPE_VIEW_SELECTED
            | AccessibilityEvent.TYPE_VIEW_TEXT_CHANGED
            | AccessibilityEvent.TYPE_VIEW_TEXT_SELECTION_CHANGED
            | AccessibilityEvent.TYPE_WINDOW_CONTENT_CHANGED
            | AccessibilityEvent.TYPE_WINDOWS_CHANGED
            | AccessibilityEvent.TYPE_WINDOW_STATE_CHANGED
            | AccessibilityEvent.TYPE_VIEW_SCROLLED;

    /**
     * How long the system may take to grant the UiAutomation (Android 14+). UiAutomation.connect()
     * gives it 5 s, which a slow emulator (software rendering, a busy host) can miss right after
     * the last agent's UiAutomation went away.
     */
    static final long CONNECT_TIMEOUT_MS = 20_000;

    public static void main(String[] args) {
        if (args.length < 2 || args[1].length() < 16) {
            System.out.println("usage: Main <port> <token>");
            System.exit(2);
        }
        int port = Integer.parseInt(args[0]);
        String token = args[1];

        // The input manager and the display are looked up while the UiAutomation connects (which
        // mostly waits on system_server), not after it.
        Thread warm = new Thread(new Main(null, 0, null, null), "virtuoso-warm");
        warm.start();
        // app_process has no main looper; the accessibility client posts to it.
        Looper.prepareMainLooper();
        UiAutomation ua;
        try {
            ua = connect();
        } catch (Throwable t) {
            System.out.println("error: no UiAutomation (another test agent running?): " + Util.cause(t));
            System.out.flush();
            System.exit(3);
            return;
        }
        new Thread(new Main(ua, port, token, warm), "virtuoso-server").start();
        Looper.loop();
    }

    private final UiAutomation ua;
    private final int port;
    private final String token;
    private final Thread warm;

    /**
     * Main is the server thread's body itself (and the warm-up thread's, without a UiAutomation):
     * a lambda would be a class of its own in the dex.
     */
    private Main(UiAutomation ua, int port, String token, Thread warm) {
        this.ua = ua;
        this.port = port;
        this.token = token;
        this.warm = warm;
    }

    @Override
    public void run() {
        if (ua == null) {
            Input.warm();
            Screen.warm();
            return;
        }
        try {
            warm.join();
            new Server(ua, port, token).run();
        } catch (Throwable t) {
            System.out.println("error: " + Util.cause(t));
            System.out.flush();
        }
        System.exit(1);
    }

    private static UiAutomation connect() throws Exception {
        // Since Android 11 UiAutomation connects on a thread of its own and this looper only
        // hands events to the listener: the main thread, idle in Looper.loop(), does that. Before,
        // the connection's callbacks come through it too, and connect() here would wait forever.
        Looper looper = Looper.getMainLooper();
        if (android.os.Build.VERSION.SDK_INT < 30) {
            HandlerThread thread = new HandlerThread("virtuoso-uiautomation");
            thread.start();
            looper = thread.getLooper();
        }
        Object connection = Class.forName("android.app.UiAutomationConnection")
                .getDeclaredConstructor().newInstance();
        UiAutomation ua = UiAutomation.class
                .getConstructor(Looper.class, Class.forName("android.app.IUiAutomationConnection"))
                .newInstance(looper, connection);
        try {
            UiAutomation.class.getMethod("connectWithTimeout", int.class, long.class).invoke(ua, 0, CONNECT_TIMEOUT_MS);
        } catch (NoSuchMethodException e) {
            UiAutomation.class.getMethod("connect").invoke(ua);
        }
        AccessibilityServiceInfo info = ua.getServiceInfo();
        info.flags |= AccessibilityServiceInfo.FLAG_INCLUDE_NOT_IMPORTANT_VIEWS
                | AccessibilityServiceInfo.FLAG_REPORT_VIEW_IDS
                | AccessibilityServiceInfo.FLAG_RETRIEVE_INTERACTIVE_WINDOWS;
        info.eventTypes = EVENT_TYPES;
        ua.setServiceInfo(info);
        return ua;
    }
}
