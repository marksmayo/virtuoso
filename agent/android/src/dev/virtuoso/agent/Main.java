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
public final class Main {
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

    public static void main(String[] args) {
        if (args.length < 2 || args[1].length() < 16) {
            System.out.println("usage: Main <port> <token>");
            System.exit(2);
        }
        int port = Integer.parseInt(args[0]);
        String token = args[1];

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
        Thread server = new Thread(() -> {
            try {
                new Server(ua, port, token).run();
            } catch (Throwable t) {
                System.out.println("error: " + Util.cause(t));
                System.out.flush();
            }
            System.exit(1);
        }, "virtuoso-server");
        server.start();
        Looper.loop();
    }

    private static UiAutomation connect() throws Exception {
        HandlerThread thread = new HandlerThread("virtuoso-uiautomation");
        thread.start();
        Object connection = Class.forName("android.app.UiAutomationConnection")
                .getDeclaredConstructor().newInstance();
        UiAutomation ua = UiAutomation.class
                .getConstructor(Looper.class, Class.forName("android.app.IUiAutomationConnection"))
                .newInstance(thread.getLooper(), connection);
        UiAutomation.class.getMethod("connect").invoke(ua);
        AccessibilityServiceInfo info = ua.getServiceInfo();
        info.flags |= AccessibilityServiceInfo.FLAG_INCLUDE_NOT_IMPORTANT_VIEWS
                | AccessibilityServiceInfo.FLAG_REPORT_VIEW_IDS
                | AccessibilityServiceInfo.FLAG_RETRIEVE_INTERACTIVE_WINDOWS;
        info.eventTypes = EVENT_TYPES;
        ua.setServiceInfo(info);
        return ua;
    }
}
