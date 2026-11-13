package dev.virtuoso.agent;

import android.app.UiAutomation;
import android.os.SystemClock;
import android.view.accessibility.AccessibilityEvent;
import android.view.accessibility.AccessibilityNodeInfo;

import java.util.HashMap;
import java.util.HashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.concurrent.ConcurrentHashMap;

/**
 * Tells when the app has finished reacting to the last action ({@link #mark}).
 *
 * The UI is settled when no UI-change accessibility event has arrived for `quiet` ms and a
 * round trip to the app's main thread comes back fast (it is idle, not about to change more).
 * When nothing changed at all, `nochange` ms must pass first: the app may react late.
 *
 * An endless animation (a spinner, a ticker) sends events forever. Once events have gone on
 * for RESTLESS_START_MS, the active window's tree is read every READ_MS; nodes that changed in
 * either of the last two reads are left out, and the screen is settled when what is left
 * holds still for STABLE_READS reads.
 */
final class Settle implements UiAutomation.OnAccessibilityEventListener {
    private static final int POLL_MS = 5;
    private static final int MAX_SLEEP_MS = 20;
    private static final int PROBE_BUSY_MS = 12;
    private static final int RESTLESS_START_MS = 300;
    private static final int READ_MS = 80;
    private static final int STABLE_READS = 3;

    private final UiAutomation ua;
    private Screen screen;
    private volatile long lastEvent;
    private volatile long lastMark;
    private volatile int events;
    /** Every event heard, ever: what the screen shows can only have changed when this moved. */
    private volatile long seq;
    private final Object changed = new Object();
    /**
     * Requests waiting in {@link #awaitChange}: most events come with nobody waiting, and skip the
     * lock. Counted under the lock, after which the waiter reads seq: an event bumps seq before
     * it reads this, so either the waiter sees the new seq or the event sees the waiter.
     */
    private volatile int waiters;
    /** Which packages sent the events since the last mark, and how many each (for tuning). */
    private final Map<String, Integer> sources = new ConcurrentHashMap<>();
    /**
     * Counting sources and replying with them and the settle's other workings, on from the
     * first settle that asks (VIRTUOSO_DEBUG): the host reads only `status` and `changed`.
     */
    volatile boolean trace;

    Settle(UiAutomation ua) {
        this.ua = ua;
        lastMark = SystemClock.uptimeMillis();
        ua.setOnAccessibilityEventListener(this);
    }

    void setScreen(Screen s) {
        screen = s;
    }

    @Override
    public void onAccessibilityEvent(AccessibilityEvent e) {
        seq++;
        if (waiters > 0) {
            synchronized (changed) {
                changed.notifyAll();
            }
        }
        switch (e.getEventType()) {
            case AccessibilityEvent.TYPE_WINDOW_CONTENT_CHANGED:
            case AccessibilityEvent.TYPE_WINDOW_STATE_CHANGED:
            case AccessibilityEvent.TYPE_WINDOWS_CHANGED:
            case AccessibilityEvent.TYPE_VIEW_SCROLLED:
            case AccessibilityEvent.TYPE_VIEW_TEXT_CHANGED:
                lastEvent = SystemClock.uptimeMillis();
                events++;
                if (!trace) break;
                CharSequence pkg = e.getPackageName();
                String key = pkg == null ? "?" : pkg.toString();
                Integer n = sources.get(key);
                sources.put(key, n == null ? 1 : n + 1);
                break;
            default:
        }
    }

    long seq() {
        return seq;
    }

    /** Waits up to `ms` for an event after `after` (a {@link #seq} value). */
    void awaitChange(long after, int ms) {
        long deadline = SystemClock.uptimeMillis() + ms;
        synchronized (changed) {
            waiters++;
            long left;
            while (seq == after && (left = deadline - SystemClock.uptimeMillis()) > 0) {
                try {
                    changed.wait(left);
                } catch (InterruptedException e) {
                    break;
                }
            }
            waiters--;
        }
    }

    void mark() {
        events = 0;
        sources.clear();
        lastMark = SystemClock.uptimeMillis();
    }

    void waitSettled(int quiet, int nochange, int timeout, Json.Out o) {
        long start = SystemClock.uptimeMillis();
        long deadline = start + timeout;
        int status = 2;
        String method = "events";
        long nextRead = 0;
        Restless restless = null;
        int probes = 0;
        while (true) {
            long now = SystemClock.uptimeMillis();
            // The moment the outcome can next change: sleeping to it, not a poll period past it.
            long next;
            if (events == 0) {
                if (now - lastMark >= nochange && idle()) { status = 1; break; }
                next = lastMark + nochange;
            } else {
                if (now - lastEvent >= quiet) {
                    probes++;
                    if (idle() && now - lastEvent >= quiet) { status = 0; break; }
                }
                next = lastEvent + quiet;
                // Events that never stop (an endless animation), or a main thread kept busy by
                // work that changes nothing on screen: what is on screen holding still is enough.
                if (now - lastMark >= RESTLESS_START_MS && now >= nextRead) {
                    if (restless == null) restless = new Restless();
                    nextRead = now + READ_MS;
                    if (restless.stableAfter(screen.signature())) {
                        status = 0;
                        method = "stable";
                        break;
                    }
                }
                next = Math.min(next, Math.max(lastMark + RESTLESS_START_MS, nextRead));
            }
            if (now >= deadline) break;
            // A moment already past (the app was busy): wait a poll period before asking again.
            // Capped, as an event can come and end the no-change wait early.
            long wait = Math.min(next, deadline) - SystemClock.uptimeMillis();
            Util.sleep(wait <= 0 ? POLL_MS : Math.min(wait, MAX_SLEEP_MS));
        }
        o.put("status", status).put("changed", events > 0);
        if (!trace) return;
        o.put("events", events)
                .put("probes", probes)
                .put("method", method)
                .put("waited", SystemClock.uptimeMillis() - start)
                .put("lastEventMs", events > 0 ? lastEvent - lastMark : -1);
        o.key("sources").open();
        for (Map.Entry<String, Integer> e : sources.entrySet()) o.put(e.getKey(), e.getValue());
        o.close();
    }

    /** A round trip served on the app's main thread: slow while it is busy drawing a change. */
    private boolean idle() {
        AccessibilityNodeInfo root = ua.getRootInActiveWindow();
        if (root == null) return true;
        long t = SystemClock.uptimeMillis();
        root.refresh();
        return SystemClock.uptimeMillis() - t < PROBE_BUSY_MS;
    }

    /** Signatures of successive reads, with the nodes that keep changing left out. */
    private static final class Restless {
        private Map<String, String> last = new HashMap<>();
        private Set<String> changedBefore = new HashSet<>();
        private String lastStable;
        private int stableCount;

        boolean stableAfter(List<String> sig) {
            Map<String, String> cur = new HashMap<>();
            for (String line : sig) {
                int bar = line.indexOf('|');
                cur.put(line.substring(0, bar), line);
            }
            Set<String> changed = new HashSet<>();
            for (Map.Entry<String, String> e : cur.entrySet()) {
                String prev = last.get(e.getKey());
                if (prev != null && !prev.equals(e.getValue())) changed.add(e.getKey());
            }
            StringBuilder stable = new StringBuilder();
            for (String line : sig) {
                String key = line.substring(0, line.indexOf('|'));
                if (!changed.contains(key) && !changedBefore.contains(key)) stable.append(line).append('\n');
            }
            String s = stable.toString();
            stableCount = s.equals(lastStable) && changed.size() * 3 <= cur.size() ? stableCount + 1 : 0;
            lastStable = s;
            changedBefore = changed;
            last = cur;
            return stableCount >= STABLE_READS - 1;
        }
    }
}
