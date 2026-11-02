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
 * Gestures are backdated: their events carry the timestamps of a gesture that ends now and are
 * injected at once, so a 400 ms swipe flings like one but returns in a frame or two.
 */
final class Input {
    private static final int ASYNC = 0, WAIT_FOR_FINISH = 2;
    private static final int TAP_MS = 40;
    private static final int SWIPE_STEP_MS = 8;
    /** A drag that should not fling stays still this long before lifting. */
    private static final int HOLD_MS = 80;
    private static final int TYPE_CHUNK = 1;
    private static final int FOCUS_WAIT_MS = 2000;

    private final UiAutomation ua;
    private final Settle settle;
    private KeyCharacterMap keys;
    private Object manager;
    private Method inject;

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
        MotionEvent e = MotionEvent.obtain(down, at, action, x, y, 0);
        e.setSource(InputDevice.SOURCE_TOUCHSCREEN);
        send(e, sync);
        e.recycle();
    }

    void tap(int x, int y) {
        settle.mark();
        long now = SystemClock.uptimeMillis();
        long down = now - TAP_MS;
        touch(MotionEvent.ACTION_DOWN, down, down, x, y, false);
        touch(MotionEvent.ACTION_UP, down, now, x, y, false);
    }

    /** Held in real time: the app's long-press timer runs on the clock, not on timestamps. */
    void press(int x, int y, int ms) {
        settle.mark();
        long down = SystemClock.uptimeMillis();
        touch(MotionEvent.ACTION_DOWN, down, down, x, y, false);
        Util.sleep(ms);
        touch(MotionEvent.ACTION_UP, down, SystemClock.uptimeMillis(), x, y, false);
    }

    void swipe(int x1, int y1, int x2, int y2, int ms, boolean fling) {
        settle.mark();
        ms = Math.max(ms, SWIPE_STEP_MS * 2);
        int hold = fling ? 0 : HOLD_MS;
        long now = SystemClock.uptimeMillis();
        long down = now - ms - hold;
        touch(MotionEvent.ACTION_DOWN, down, down, x1, y1, false);
        int steps = Math.max(2, ms / SWIPE_STEP_MS);
        for (int i = 1; i <= steps; i++) {
            float t = (float) i / steps;
            touch(MotionEvent.ACTION_MOVE, down, down + (long) (ms * t),
                    x1 + (x2 - x1) * t, y1 + (y2 - y1) * t, i == 1);
        }
        if (hold > 0) touch(MotionEvent.ACTION_MOVE, down, now, x2, y2, false);
        touch(MotionEvent.ACTION_UP, down, now, x2, y2, true);
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

    void erase(int count) {
        settle.mark();
        // Erasing at least the whole text: clear the field in one action instead of a key each.
        AccessibilityNodeInfo focus = ua.findFocus(AccessibilityNodeInfo.FOCUS_INPUT);
        if (focus != null && focus.isEditable() && !focus.isPassword()) {
            CharSequence cur = focus.getText();
            int len = (cur == null || focus.isShowingHintText()) ? 0 : cur.length();
            if (len <= count) {
                Bundle args = new Bundle();
                args.putCharSequence(AccessibilityNodeInfo.ACTION_ARGUMENT_SET_TEXT_CHARSEQUENCE, "");
                if (focus.performAction(AccessibilityNodeInfo.ACTION_SET_TEXT, args)) return;
            }
        }
        keyEvent(KeyEvent.KEYCODE_MOVE_END, true);
        for (int i = 0; i < count; i++) {
            keyEvent(KeyEvent.KEYCODE_DEL, (i + 1) % TYPE_CHUNK == 0 || i == count - 1);
        }
    }
}
