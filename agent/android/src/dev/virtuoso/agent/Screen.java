package dev.virtuoso.agent;

import android.app.UiAutomation;
import android.graphics.Bitmap;
import android.graphics.Point;
import android.graphics.Rect;
import android.view.Display;
import android.view.accessibility.AccessibilityNodeInfo;
import android.view.accessibility.AccessibilityWindowInfo;

import java.io.ByteArrayOutputStream;
import java.util.ArrayList;
import java.util.List;

/** Reads what is on screen: every window's accessibility tree, the display, screenshots. */
final class Screen {
    static final int CLICKABLE = 1, ENABLED = 2, FOCUSED = 4, SCROLLABLE = 8, CHECKED = 16,
            VISIBLE = 32, SELECTED = 64, EDITABLE = 128, CHECKABLE = 256, LONG_CLICKABLE = 512,
            PASSWORD = 1024;

    private final UiAutomation ua;
    private final Rect r = new Rect();
    /**
     * Compose sends no accessibility events to a UiAutomation client, so the node cache cannot
     * know a Compose node changed: while the last read showed Compose, each read clears it.
     */
    private volatile boolean composeSeen;
    /** Whether the read under way has met Compose yet: past the first node, no need to ask. */
    private boolean compose;

    Screen(UiAutomation ua) {
        this.ua = ua;
    }

    /** The default display, looked up at start ({@link #warm}) for the ready line's info. */
    private static Display warmDisplay;

    /** Looks up the display, on the warm-up thread while the UiAutomation connects. */
    static void warm() {
        try {
            warmDisplay = display();
        } catch (Throwable t) {
            // info looks again, and reports it
        }
    }

    private static Display display() throws Exception {
        Object dmg = Class.forName("android.hardware.display.DisplayManagerGlobal")
                .getMethod("getInstance").invoke(null);
        return (Display) dmg.getClass().getMethod("getRealDisplay", int.class).invoke(dmg, 0);
    }

    /** events: with the event types the agent listens to (a call to the system), for the `info` op. */
    void info(Json.Out o, boolean events) throws Exception {
        // The one looked up at start serves once; later calls look again, as the display turns.
        Display d = warmDisplay;
        warmDisplay = null;
        if (d == null) d = display();
        Point p = new Point();
        d.getRealSize(p);
        o.put("width", p.x).put("height", p.y).put("rotation", d.getRotation())
                .put("sdk", android.os.Build.VERSION.SDK_INT);
        if (events) o.put("eventTypes", ua.getServiceInfo().eventTypes);
    }

    /**
     * Drops the node cache, so the next read asks the app for every node. A layout change that
     * sends no event (a Reanimated height animation pushing content down) leaves cached bounds
     * stale otherwise.
     */
    private void clearCache() {
        // Before API 34 there is none to call: the cache stays.
        if (android.os.Build.VERSION.SDK_INT >= 34) ua.clearCache();
    }

    /** Whether the last read showed Compose, whose changes send no events. */
    boolean composeSeen() {
        return composeSeen;
    }

    /** Whether the read under way hashes its nodes, and the hash so far. */
    private boolean hashing;
    private long fp;

    /**
     * fresh: the cache is cleared first (as on a Compose screen, once). hash: returns the host's
     * fingerprint of the nodes written (ScreenTree::fingerprint), else nothing to go by.
     */
    long dump(Json.Out o, boolean all, boolean fresh, boolean hash) {
        if (fresh || composeSeen) clearCache();
        compose = false;
        hashing = hash;
        fp = 0xcbf29ce484222325L;
        List<AccessibilityWindowInfo> windows = ua.getWindows();
        AccessibilityNodeInfo[] roots = windowList(o, windows);
        o.openArray("nodes");
        for (int wi = 0; wi < roots.length; wi++) {
            if (roots[wi] == null) continue;
            // The host marks a keyboard's nodes with a flag of its own (KEYBOARD).
            int marks = windows.get(wi).getType() == AccessibilityWindowInfo.TYPE_INPUT_METHOD ? 2048 : 0;
            walk(o, roots[wi], 0, wi, all, marks);
        }
        o.closeArray();
        composeSeen = compose;
        return fp;
    }

    /** FNV-1a over UTF-8, as the host hashes. */
    private void mix(String s) {
        for (byte x : s.getBytes(java.nio.charset.StandardCharsets.UTF_8)) mix(x, 8);
    }

    /** The low `bits` of v, a byte at a time, little-endian. */
    private void mix(int v, int bits) {
        for (int k = 0; k < bits; k += 8) {
            fp ^= (v >>> k) & 0xff;
            fp *= 0x100000001b3L;
        }
    }

    /** The windows alone: enough to tell which app is in front or whether a keyboard shows. */
    void windows(Json.Out o) {
        windowList(o, ua.getWindows());
    }

    /** Writes the `windows` array; each window's root is read once, for this and the nodes. */
    private AccessibilityNodeInfo[] windowList(Json.Out o, List<AccessibilityWindowInfo> windows) {
        AccessibilityNodeInfo[] roots = new AccessibilityNodeInfo[windows.size()];
        o.openArray("windows");
        for (int wi = 0; wi < windows.size(); wi++) {
            AccessibilityWindowInfo w = windows.get(wi);
            AccessibilityNodeInfo root = w.getRoot();
            roots[wi] = root;
            o.item();
            CharSequence pkg = root != null ? root.getPackageName() : null;
            w.getBoundsInScreen(r);
            o.put("pkg", pkg == null ? "" : pkg.toString())
                    .put("type", w.getType())
                    .put("active", w.isActive());
            bounds(o);
            o.close();
        }
        o.closeArray();
        return roots;
    }

    private void walk(Json.Out o, AccessibilityNodeInfo n, int depth, int window, boolean all, int marks) {
        boolean visible = n.isVisibleToUser();
        if (!visible && !all) return;
        String cls = str(n.getClassName());
        if (!compose) compose = cls.startsWith("androidx.compose") || cls.contains("ComposeView");
        n.getBoundsInScreen(r);
        int f = 0;
        if (n.isClickable()) f |= CLICKABLE;
        if (n.isEnabled()) f |= ENABLED;
        if (n.isFocused()) f |= FOCUSED;
        if (n.isScrollable()) f |= SCROLLABLE;
        if (n.isChecked()) f |= CHECKED;
        if (visible) f |= VISIBLE;
        if (n.isSelected()) f |= SELECTED;
        if (n.isEditable()) f |= EDITABLE;
        if (n.isCheckable()) f |= CHECKABLE;
        if (n.isLongClickable()) f |= LONG_CLICKABLE;
        if (n.isPassword()) f |= PASSWORD;
        o.item().put("d", depth).put("w", window);
        bounds(o);
        o.put("f", f);
        String text = str(n.getText()), desc = str(n.getContentDescription()), id = str(n.getViewIdResourceName());
        putIfAny(o, "t", text);
        putIfAny(o, "c", desc);
        putIfAny(o, "i", id);
        putIfAny(o, "k", cls);
        putIfAny(o, "h", str(n.getHintText()));
        putIfAny(o, "e", str(n.getError()));
        o.close();
        if (hashing) {
            mix(text);
            mix(desc);
            mix(id);
            mix(f | marks, 32);
            mix(r.left, 32);
            mix(r.top, 32);
            mix(r.right, 32);
            mix(r.bottom, 32);
        }
        for (int i = 0, c = n.getChildCount(); i < c; i++) {
            AccessibilityNodeInfo child = n.getChild(i);
            if (child != null) walk(o, child, depth + 1, window, all, marks);
        }
    }

    private static void putIfAny(Json.Out o, String key, String v) {
        if (!v.isEmpty()) o.put(key, v);
    }

    private void bounds(Json.Out o) {
        o.key("b").b.append('[').append(r.left).append(',').append(r.top).append(',')
                .append(r.right).append(',').append(r.bottom).append(']');
    }

    private static String str(CharSequence s) {
        return s == null ? "" : s.toString();
    }

    /** One line per visible node of the active window, for settle's endless-animation check. */
    List<String> signature() {
        if (composeSeen) clearCache();
        List<String> out = new ArrayList<>();
        AccessibilityNodeInfo root = ua.getRootInActiveWindow();
        if (root != null) sign(root, "0", out);
        return out;
    }

    private void sign(AccessibilityNodeInfo n, String path, List<String> out) {
        if (!n.isVisibleToUser()) return;
        n.getBoundsInScreen(r);
        out.add(path + "|" + n.getClassName() + "|" + n.getViewIdResourceName() + "|" + n.getText()
                + "|" + n.getContentDescription() + "|" + r.toShortString());
        for (int i = 0, c = n.getChildCount(); i < c; i++) {
            AccessibilityNodeInfo child = n.getChild(i);
            if (child != null) sign(child, path + "." + i, out);
        }
    }

    /** The screen as a PNG, sent raw after the reply (`blob`): no base64 or string copies. */
    ByteArrayOutputStream screenshot(Json.Out o) {
        Bitmap b = ua.takeScreenshot();
        if (b == null) throw new IllegalStateException("the screenshot failed");
        ByteArrayOutputStream png = new ByteArrayOutputStream(1 << 20);
        try {
            b.compress(Bitmap.CompressFormat.PNG, 100, png);
        } finally {
            // ~10 MB of pixels, freed now rather than whenever a GC runs the bitmap's cleaner.
            b.recycle();
        }
        o.put("blob", png.size());
        return png;
    }
}
