package dev.virtuoso.agent;

import android.app.UiAutomation;
import android.os.Bundle;
import android.os.SystemClock;
import android.view.InputDevice;
import android.view.InputEvent;
import android.view.KeyCharacterMap;
import android.view.KeyEvent;
import android.view.MotionEvent;
import android.view.accessibility.AccessibilityNodeInfo;

import java.lang.reflect.Method;

/**
 * Touches and keys. As the shell user the agent hands events straight to the input manager
 * (no UiAutomation transaction syncs, ~1 ms an event); UiAutomation's injection is the fallback.
 *
 * A swipe plays out in real time, as a finger's does. A drag that should not fling is
 * backdated (its events carry the timestamps of a drag that ends now and go in at once) but for
 * its first two moves, each waited for. No gesture starts before the last one's events.
 */
final class Input {
    private static final int ASYNC = 0, WAIT_FOR_FINISH = 2;
    private static final int TAP_MS = 40;
    private static final int SWIPE_STEP_MS = 8;
    /** A drag that should not fling stays still this long before lifting. */
    private static final int HOLD_MS = 80;
    /**
     * A drag's end waits this long after its moves, in real time: over a frame, so the app
     * handles them in different frames, as a finger's. In one, a React Native list kept only
     * part of a cancelled drag, or none of it.
     */
    private static final int END_GAP_MS = 25;
    private static final int TYPE_CHUNK = 1;
    private static final int FOCUS_WAIT_MS = 2000;

    private final UiAutomation ua;
    private final Settle settle;
    private KeyCharacterMap keys;
    private Object manager;
    private Method inject;
    /** The latest event time sent: Android's input verifier refuses a gesture that starts earlier. */
    private long lastEventTime;
    /** Touch slop is 8 dp: the first move goes past it twice over, the second 4 dp further. */
    private final float firstMovePx = 16 * android.content.res.Resources.getSystem().getDisplayMetrics().density;
    private final float secondMovePx = 4 * android.content.res.Resources.getSystem().getDisplayMetrics().density;

    Input(UiAutomation ua, Settle settle) {
        this.ua = ua;
        this.settle = settle;
        try {
            Class<?> cls;
            try {
                cls = Class.forName("android.hardware.input.InputManagerGlobal");
            } catch (ClassNotFoundException e) {
                cls = android.hardware.input.InputManager.class;
            }
            manager = cls.getDeclaredMethod("getInstance").invoke(null);
            inject = cls.getMethod("injectInputEvent", InputEvent.class, int.class);
        } catch (Throwable t) {
            System.out.println("warning: input manager injection unavailable: " + Util.cause(t));
        }
    }

    private void send(InputEvent e, boolean sync) {
        if (inject != null) {
            try {
                inject.invoke(manager, e, sync ? WAIT_FOR_FINISH : ASYNC);
                return;
            } catch (Throwable t) {
                System.out.println("warning: input manager injection failed: " + Util.cause(t));
                inject = null;
            }
        }
        ua.injectInputEvent(e, sync);
    }

    private void touch(int action, long down, long at, float x, float y, boolean sync) {
        lastEventTime = Math.max(lastEventTime, at);
        MotionEvent e = MotionEvent.obtain(down, at, action, x, y, 0);
        e.setSource(InputDevice.SOURCE_TOUCHSCREEN);
        send(e, sync);
        e.recycle();
    }

    void tap(int x, int y) {
        settle.mark();
        long down = notBeforeLast(SystemClock.uptimeMillis() - TAP_MS);
        touch(MotionEvent.ACTION_DOWN, down, down, x, y, false);
        touch(MotionEvent.ACTION_UP, down, Math.max(down + TAP_MS, SystemClock.uptimeMillis()), x, y, false);
    }

    /** Held in real time: the app's long-press timer runs on the clock, not on timestamps. */
    void press(int x, int y, int ms) {
        settle.mark();
        long down = SystemClock.uptimeMillis();
        touch(MotionEvent.ACTION_DOWN, down, down, x, y, false);
        Util.sleep(ms);
        touch(MotionEvent.ACTION_UP, down, SystemClock.uptimeMillis(), x, y, false);
    }

    /** A gesture's start, after the last one's events (it waits when it has to). */
    private long notBeforeLast(long start) {
        long early = lastEventTime + 1 - start;
        if (early > 0) {
            long sleep = early - (SystemClock.uptimeMillis() - start);
            if (sleep > 0) Util.sleep((int) sleep);
            start += early;
        }
        return start;
    }

    /** cancel: a drag that ends in a CANCEL, not a lift, on which no list flings. */
    void swipe(int x1, int y1, int x2, int y2, int ms, boolean fling, boolean cancel) {
        settle.mark();
        ms = Math.max(ms, SWIPE_STEP_MS * 2);
        int steps = Math.max(2, ms / SWIPE_STEP_MS);
        float dx = x2 - x1, dy = y2 - y1;
        if (fling) {
            // In real time: backdated and sent at once, a React Native Gesture Handler pan
            // sometimes ended with no movement (speedrun found a swipe left read as one down, a
            // run in three).
            long down = notBeforeLast(SystemClock.uptimeMillis());
            touch(MotionEvent.ACTION_DOWN, down, down, x1, y1, false);
            for (int i = 1; i <= steps; i++) {
                float t = (float) i / steps;
                long at = down + (long) (ms * t);
                long wait = at - SystemClock.uptimeMillis();
                if (wait > 0) Util.sleep((int) wait);
                touch(MotionEvent.ACTION_MOVE, down, Math.max(at, SystemClock.uptimeMillis()), x1 + dx * t, y1 + dy * t, false);
            }
            touch(MotionEvent.ACTION_UP, down, SystemClock.uptimeMillis(), x2, y2, true);
            return;
        }
        long down = notBeforeLast(SystemClock.uptimeMillis() - ms - HOLD_MS);
        touch(MotionEvent.ACTION_DOWN, down, down, x1, y1, false);
        // Two moves just past touch slop, each handled before the next: a scrolling parent
        // intercepts the drag on the first, or on the second when the view under the finger
        // holds on to it until the finger moves (a text field). Sent together they reach the
        // app as one batched move, spent on intercepting, and nothing scrolls.
        float dist = (float) Math.hypot(dx, dy);
        float first = dist > 0 ? Math.min(1f, firstMovePx / dist) : 1f;
        float second = dist > 0 ? Math.min(1f, (firstMovePx + secondMovePx) / dist) : 1f;
        touch(MotionEvent.ACTION_MOVE, down, down + (long) (ms * first), x1 + dx * first, y1 + dy * first, true);
        if (second > first) touch(MotionEvent.ACTION_MOVE, down, down + (long) (ms * second), x1 + dx * second, y1 + dy * second, true);
        for (int i = 1; i <= steps; i++) {
            float t = (float) i / steps;
            if (t > second) touch(MotionEvent.ACTION_MOVE, down, down + (long) (ms * t), x1 + dx * t, y1 + dy * t, false);
        }
        // Still at the end, then still again past 40 ms: the velocity tracker takes the
        // pointer as stopped, so the lift does not fling. Not always in a React Native list
        // (two drags of 720 px carried one 1600 px to more than a screen): there, a CANCEL.
        touch(MotionEvent.ACTION_MOVE, down, down + ms + 10, x2, y2, false);
        touch(MotionEvent.ACTION_MOVE, down, down + ms + HOLD_MS - 5, x2, y2, false);
        Util.sleep(END_GAP_MS);
        int end = cancel ? MotionEvent.ACTION_CANCEL : MotionEvent.ACTION_UP;
        touch(end, down, Math.max(down + ms + HOLD_MS, SystemClock.uptimeMillis()), x2, y2, true);
    }

    /**
     * Drags in real time, as a finger does: drag-and-drop UIs lift the item only after a long
     * press and track the finger, so the events cannot be backdated like a swipe's.
     */
    void drag(int x1, int y1, int x2, int y2, int hold, int ms) {
        settle.mark();
        long down = SystemClock.uptimeMillis();
        touch(MotionEvent.ACTION_DOWN, down, down, x1, y1, false);
        Util.sleep(hold);
        int steps = Math.max(2, ms / SWIPE_STEP_MS);
        for (int i = 1; i <= steps; i++) {
            float t = (float) i / steps;
            Util.sleep(ms / steps);
            touch(MotionEvent.ACTION_MOVE, down, SystemClock.uptimeMillis(), x1 + (x2 - x1) * t, y1 + (y2 - y1) * t, false);
        }
        // Rest on the target so it registers the hover before the drop.
        Util.sleep(HOLD_MS);
        touch(MotionEvent.ACTION_MOVE, down, SystemClock.uptimeMillis(), x2, y2, false);
        touch(MotionEvent.ACTION_UP, down, SystemClock.uptimeMillis(), x2, y2, true);
    }

    void key(int code) {
        settle.mark();
        keyEvent(code, false);
    }

    /** sync: return once the app has handled the key. */
    private void keyEvent(int code, boolean sync) {
        long now = SystemClock.uptimeMillis();
        KeyEvent d = new KeyEvent(now, now, KeyEvent.ACTION_DOWN, code, 0, 0,
                KeyCharacterMap.VIRTUAL_KEYBOARD, 0, 0, InputDevice.SOURCE_KEYBOARD);
        send(d, false);
        send(KeyEvent.changeAction(d, KeyEvent.ACTION_UP), sync);
    }

    /**
     * Types into the focused field with key events, which apps handle as a person typing. A
     * character no key produces (most non-ASCII) goes in through the focused node's text.
     * Returns how many characters could not be typed.
     */
    int type(String text) {
        awaitInputFocus();
        settle.mark();
        if (keys == null) keys = KeyCharacterMap.load(KeyCharacterMap.VIRTUAL_KEYBOARD);
        KeyCharacterMap map = keys;
        int skipped = 0;
        int typed = 0;
        for (int i = 0; i < text.length(); ) {
            int cp = text.codePointAt(i);
            String ch = new String(Character.toChars(cp));
            i += ch.length();
            KeyEvent[] events = map.getEvents(ch.toCharArray());
            if (events != null) {
                typed++;
                // Async, except the last event of each chunk: the app handles a chunk before
                // the next arrives, so a slow app drops nothing and a fast one waits little.
                boolean endOfChunk = typed % TYPE_CHUNK == 0 || i >= text.length();
                for (int k = 0; k < events.length; k++) {
                    send(stamped(events[k]), endOfChunk && k == events.length - 1);
                }
            } else if (!insert(ch)) {
                skipped++;
            }
        }
        return skipped;
    }

    /** getEvents stamps every event with the time the map was asked; restamp to now. */
    private static KeyEvent stamped(KeyEvent e) {
        long now = SystemClock.uptimeMillis();
        return KeyEvent.changeTimeRepeat(e, now, 0);
    }

    /**
     * A tap on a field focuses it a frame or two later, and a screen that opens on a tap focuses
     * its field after its window shows: keys sent before then go nowhere. Waits up to
     * FOCUS_WAIT_MS for an editable field with input focus; types anyway after that (a custom
     * view may take keys without being an editable node).
     */
    private void awaitInputFocus() {
        long deadline = SystemClock.uptimeMillis() + FOCUS_WAIT_MS;
        while (SystemClock.uptimeMillis() < deadline) {
            AccessibilityNodeInfo focus = ua.findFocus(AccessibilityNodeInfo.FOCUS_INPUT);
            if (focus != null && focus.isEditable()) return;
            Util.sleep(5);
        }
    }

    private boolean insert(String s) {
        AccessibilityNodeInfo focus = ua.findFocus(AccessibilityNodeInfo.FOCUS_INPUT);
        if (focus == null || !focus.isEditable()) return false;
        CharSequence cur = focus.getText();
        String now = (cur == null || focus.isShowingHintText()) ? "" : cur.toString();
        Bundle args = new Bundle();
        args.putCharSequence(AccessibilityNodeInfo.ACTION_ARGUMENT_SET_TEXT_CHARSEQUENCE, now + s);
        return focus.performAction(AccessibilityNodeInfo.ACTION_SET_TEXT, args);
    }

    /**
     * Deletes with keys, as a person would, the last waiting until the app has handled it. A
     * field cleared in one ACTION_SET_TEXT is not: React Native echoes the empty text back to the
     * field later, and that echo can take the first character typed next ("Jordan" read "ordan").
     */
    void erase(int count) {
        settle.mark();
        keyEvent(KeyEvent.KEYCODE_MOVE_END, true);
        for (int i = 0; i < count; i++) {
            keyEvent(KeyEvent.KEYCODE_DEL, (i + 1) % TYPE_CHUNK == 0 || i == count - 1);
        }
    }
}
