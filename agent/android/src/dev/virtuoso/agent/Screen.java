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

    Screen(UiAutomation ua) {
        this.ua = ua;
    }

    void info(Json.Out o) throws Exception {
        Object dmg = Class.forName("android.hardware.display.DisplayManagerGlobal")
                .getMethod("getInstance").invoke(null);
        Display d = (Display) dmg.getClass().getMethod("getRealDisplay", int.class).invoke(dmg, 0);
        Point p = new Point();
        d.getRealSize(p);
        o.put("width", p.x).put("height", p.y).put("rotation", d.getRotation())
                .put("sdk", android.os.Build.VERSION.SDK_INT)
                .put("eventTypes", ua.getServiceInfo().eventTypes);
    }

    private void maybeClearCache() {
        if (!composeSeen) return;
        try {
            UiAutomation.class.getMethod("clearCache").invoke(ua);
        } catch (Throwable ignored) {
            // before API 34: the cache stays
        }
    }

    /** Whether the last read showed Compose, whose changes send no events. */
    boolean composeSeen() {
        return composeSeen;
    }

    void dump(Json.Out o, boolean all) {
        maybeClearCache();
        boolean compose = false;
        AccessibilityNodeInfo[] roots = windowList(o, ua.getWindows());
        o.openArray("nodes");
        for (int wi = 0; wi < roots.length; wi++) {
            if (roots[wi] == null) continue;
            compose |= walk(o, roots[wi], 0, wi, all);
        }
        o.closeArray();
        composeSeen = compose;
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

    private boolean walk(Json.Out o, AccessibilityNodeInfo n, int depth, int window, boolean all) {
        boolean visible = n.isVisibleToUser();
        if (!visible && !all) return false;
        String cls = str(n.getClassName());
        boolean compose = cls.startsWith("androidx.compose") || cls.contains("ComposeView");
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
        putIfAny(o, "t", n.getText());
        putIfAny(o, "c", n.getContentDescription());
        putIfAny(o, "i", n.getViewIdResourceName());
        putIfAny(o, "k", cls);
        CharSequence hint = n.getHintText();
        if (hint != null) o.put("h", hint.toString());
        CharSequence err = n.getError();
        if (err != null) o.put("e", err.toString());
        o.close();
        for (int i = 0, c = n.getChildCount(); i < c; i++) {
            AccessibilityNodeInfo child = n.getChild(i);
            if (child != null) compose |= walk(o, child, depth + 1, window, all);
        }
        return compose;
    }

    private static void putIfAny(Json.Out o, String key, CharSequence v) {
        if (v != null && v.length() > 0) o.put(key, v.toString());
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
        maybeClearCache();
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
