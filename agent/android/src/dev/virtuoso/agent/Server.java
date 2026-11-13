package dev.virtuoso.agent;

import android.app.UiAutomation;

import java.io.BufferedInputStream;
import java.io.DataInputStream;
import java.io.IOException;
import java.io.OutputStream;
import java.net.InetAddress;
import java.net.ServerSocket;
import java.net.Socket;
import java.nio.charset.StandardCharsets;
import java.security.MessageDigest;
import java.util.List;
import java.util.Map;

/**
 * Serves one host at a time on 127.0.0.1:port. Frames are a big-endian u32 length and UTF-8
 * JSON; the first frame must be {"token": ...} within 5 s.
 */
final class Server {
    private final UiAutomation ua;
    private final int port;
    private final byte[] token;
    private final Screen screen;
    private final Input input;
    private final Settle settle;
    private Clipboard clipboard;
    /** Made on the first `location` (as the clipboard is): most runs never mock one. */
    private MockLocation location;
    /** One reply writer and one UTF-8 buffer for every reply: no garbage per request. */
    private final Json.Out reply = new Json.Out();
    private byte[] utf8 = new byte[1 << 16];
    /** The reply's chars, copied out of the builder in one go (charAt is a call per char). */
    private char[] chars = new char[1 << 14];
    private byte[] frame = new byte[1 << 10];
    /** Buffers past this size after a big reply are let go rather than kept. */
    private static final int MAX_KEPT = 1 << 20;
    /** Raw bytes that follow the reply in their own frame (a screenshot). */
    private java.io.ByteArrayOutputStream blob;

    Server(UiAutomation ua, int port, String token) {
        this.ua = ua;
        this.port = port;
        this.token = token.getBytes(StandardCharsets.UTF_8);
        this.settle = new Settle(ua);
        this.screen = new Screen(ua);
        this.input = new Input(ua, settle);
        this.settle.setScreen(screen);
    }

    void run() throws IOException {
        ServerSocket ss = new ServerSocket(port, 1, InetAddress.getByName("127.0.0.1"));
        // The display comes with the ready line, which saves the host an `info` round trip.
        // Written with the reply writer, which has nothing in it yet.
        Json.Out display = reply.open();
        try {
            screen.info(display, false);
        } catch (Exception e) {
            // the host asks for `info` itself
        }
        System.out.println("listening on 127.0.0.1:" + port + " " + display.close());
        System.out.flush();
        Thread keys = new Thread(input, "virtuoso-keys");
        keys.setDaemon(true);
        keys.start();
        while (true) {
            try (Socket s = ss.accept()) {
                serve(s);
            } catch (IOException e) {
                // the host went away; wait for the next one
            } finally {
                if (location != null) location.reset();
            }
        }
    }

    private void serve(Socket s) throws IOException {
        s.setTcpNoDelay(true);
        s.setSoTimeout(5000);
        DataInputStream in = new DataInputStream(new BufferedInputStream(s.getInputStream()));
        // Each reply frame goes out in one write, its length in front of it in the same buffer:
        // through a stream buffer, a reply past 8 KB went as two (and two adb packets).
        OutputStream out = s.getOutputStream();
        Object hello = Json.parse(readFrame(in));
        Object given = hello instanceof Map ? ((Map<?, ?>) hello).get("token") : null;
        if (!(given instanceof String)
                || !MessageDigest.isEqual(token, ((String) given).getBytes(StandardCharsets.UTF_8))) {
            return;
        }
        s.setSoTimeout(0);
        while (true) {
            @SuppressWarnings("unchecked")
            Map<String, Object> req = (Map<String, Object>) Json.parse(readFrame(in));
            // The host is done and does not wait for an answer. The UiAutomation is let go first:
            // the system then frees it at once, where it would take its time noticing the
            // process is gone (seconds on a slow emulator), and the next agent would wait.
            if ("quit".equals(req.get("op"))) {
                try {
                    UiAutomation.class.getMethod("disconnect").invoke(ua);
                } catch (Exception ignored) {
                }
                System.exit(0);
            }
            long start = System.nanoTime();
            Json.Out o = reply.reset().open();
            blob = null;
            try {
                o.put("ok", true);
                handle(req, o);
                // An action and the settle after it, in one round trip.
                Object then = req.get("then");
                if (then instanceof Map) {
                    @SuppressWarnings("unchecked")
                    Map<String, Object> next = (Map<String, Object>) then;
                    handle(next, o);
                }
            } catch (Throwable t) {
                blob = null;
                o = reply.reset().open().put("ok", false).put("error", String.valueOf(Util.cause(t)));
            }
            o.put("us", (System.nanoTime() - start) / 1000).close();
            int n = encode(o.b);
            putInt(0, n - 4);
            if (blob != null) {
                // The blob's frame header goes with the reply; the blob itself straight after.
                putInt(n, blob.size());
                out.write(utf8, 0, n + 4);
                blob.writeTo(out);
                blob = null;
            } else {
                out.write(utf8, 0, n);
            }
            reply.shrink();
            if (utf8.length > MAX_KEPT) utf8 = new byte[1 << 16];
            if (chars.length > MAX_KEPT) chars = new char[1 << 14];
        }
    }

    /**
     * `s` as UTF-8 in {@link #utf8} after 4 bytes left for its length (and 4 after it for a
     * blob's), reused across replies: no String or byte[] copy of each reply. Returns where it
     * ends. A lone surrogate becomes '?', as String.getBytes does.
     */
    private int encode(StringBuilder sb) {
        int len = sb.length();
        if (utf8.length < len * 3 + 8) utf8 = new byte[len * 3 + 8];
        if (chars.length < len) chars = new char[len];
        char[] s = chars;
        sb.getChars(0, len, s, 0);
        byte[] b = utf8;
        int n = 4;
        for (int i = 0; i < len; i++) {
            char c = s[i];
            if (c < 0x80) {
                b[n++] = (byte) c;
            } else if (c < 0x800) {
                b[n++] = (byte) (0xc0 | (c >> 6));
                b[n++] = (byte) (0x80 | (c & 0x3f));
            } else if (Character.isHighSurrogate(c) && i + 1 < len && Character.isLowSurrogate(s[i + 1])) {
                int cp = Character.toCodePoint(c, s[++i]);
                b[n++] = (byte) (0xf0 | (cp >> 18));
                b[n++] = (byte) (0x80 | ((cp >> 12) & 0x3f));
                b[n++] = (byte) (0x80 | ((cp >> 6) & 0x3f));
                b[n++] = (byte) (0x80 | (cp & 0x3f));
            } else if (Character.isSurrogate(c)) {
                b[n++] = '?';
            } else {
                b[n++] = (byte) (0xe0 | (c >> 12));
                b[n++] = (byte) (0x80 | ((c >> 6) & 0x3f));
                b[n++] = (byte) (0x80 | (c & 0x3f));
            }
        }
        return n;
    }

    /** A big-endian u32 into {@link #utf8} at `at`. */
    private void putInt(int at, int v) {
        utf8[at] = (byte) (v >>> 24);
        utf8[at + 1] = (byte) (v >>> 16);
        utf8[at + 2] = (byte) (v >>> 8);
        utf8[at + 3] = (byte) v;
    }

    /** A request frame, read into one buffer kept across requests. */
    private String readFrame(DataInputStream in) throws IOException {
        int len = in.readInt();
        if (len < 0 || len > (16 << 20)) throw new IOException("bad frame length " + len);
        if (frame.length < len) frame = new byte[len];
        in.readFully(frame, 0, len);
        String s = new String(frame, 0, len, StandardCharsets.UTF_8);
        if (frame.length > MAX_KEPT) frame = new byte[1 << 10];
        return s;
    }

    private void handle(Map<String, Object> r, Json.Out o) throws Exception {
        String op = (String) r.get("op");
        switch (op) {
            case "ping":
                break;
            case "info":
                screen.info(o, true);
                break;
            case "screen": {
                // `after`: the host holds the read at that event count. Until an event comes
                // (or `waitMs` passes) nothing can have changed: say so instead of reading again.
                long after = r.get("after") instanceof Number ? ((Number) r.get("after")).longValue() : -1;
                if (after >= 0 && !screen.composeSeen()) {
                    settle.awaitChange(after, num(r, "waitMs"));
                    if (settle.seq() == after) {
                        o.put("unchanged", true).put("seq", after);
                        break;
                    }
                } else if (after >= 0) {
                    // Compose changes send no events: read again after a short pause, as before.
                    Util.sleep(Math.min(num(r, "waitMs"), 10));
                }
                // tapIf: tap at (x, y) if the read is the host's lookup (by its fingerprint), and
                // leave the read out; else send it, untapped, and skip `then` (the tap's settle).
                Object tapIf = r.get("tapIf");
                int at = o.b.length();
                o.put("seq", settle.seq());
                long fp = screen.dump(o, bool(r, "all"), bool(r, "fresh"), tapIf instanceof Map);
                if (tapIf instanceof Map) {
                    @SuppressWarnings("unchecked")
                    Map<String, Object> t = (Map<String, Object>) tapIf;
                    if (Long.toHexString(fp).equals(t.get("fp"))) {
                        o.b.setLength(at);
                        input.tap(num(t, "x"), num(t, "y"));
                        o.put("tapped", true);
                    } else {
                        r.remove("then");
                    }
                }
                break;
            }
            case "tap":
                input.tap(num(r, "x"), num(r, "y"));
                break;
            case "press":
                input.press(num(r, "x"), num(r, "y"), num(r, "ms"));
                break;
            case "swipe":
                input.swipe(num(r, "x1"), num(r, "y1"), num(r, "x2"), num(r, "y2"), num(r, "ms"), bool(r, "fling"), bool(r, "cancel"));
                break;
            case "drag":
                input.drag(num(r, "x1"), num(r, "y1"), num(r, "x2"), num(r, "y2"), num(r, "hold"), num(r, "ms"));
                break;
            case "key":
                input.key(num(r, "code"));
                break;
            case "type":
                o.put("skipped", input.type((String) r.get("text")));
                break;
            case "erase":
                o.put("deleted", input.erase(num(r, "count")));
                break;
            case "mark":
                settle.mark();
                break;
            case "settle":
                if (bool(r, "trace")) settle.trace = true;
                settle.waitSettled(num(r, "quiet"), num(r, "nochange"), num(r, "timeout"), o);
                // read: the screen it settled on, for the host's next lookup (a request saved).
                if (bool(r, "read")) {
                    o.put("seq", settle.seq());
                    screen.dump(o, false, false, false);
                }
                break;
            case "windows":
                // `after`: wait (up to `waitMs`) for an event after that count first.
                if (r.get("after") instanceof Number) settle.awaitChange(((Number) r.get("after")).longValue(), num(r, "waitMs"));
                o.put("seq", settle.seq());
                screen.windows(o);
                break;
            case "exec": {
                // mark: the settle measurement starts here (an app launch), saving a round trip.
                if (bool(r, "mark")) settle.mark();
                @SuppressWarnings("unchecked")
                List<Object> argv = (List<Object>) r.get("argv");
                Exec.run(argv, o);
                break;
            }
            case "location":
                if (location == null) location = new MockLocation();
                location.set(dbl(r, "lat"), dbl(r, "lon"));
                break;
            case "clipboard":
                if (clipboard == null) clipboard = new Clipboard();
                if (r.containsKey("text")) clipboard.set((String) r.get("text"));
                else o.put("text", clipboard.get());
                break;
            case "screenshot":
                blob = screen.screenshot(o);
                break;
            default:
                throw new IllegalArgumentException("unknown op " + op);
        }
    }

    static int num(Map<String, Object> r, String k) {
        Object v = r.get(k);
        return v instanceof Number ? ((Number) v).intValue() : 0;
    }

    static double dbl(Map<String, Object> r, String k) {
        Object v = r.get(k);
        if (!(v instanceof Number)) throw new IllegalArgumentException(k + " must be a number");
        return ((Number) v).doubleValue();
    }

    static boolean bool(Map<String, Object> r, String k) {
        return Boolean.TRUE.equals(r.get(k));
    }
}
