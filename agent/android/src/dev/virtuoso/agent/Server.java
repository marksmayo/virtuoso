package dev.virtuoso.agent;

import android.app.UiAutomation;

import java.io.BufferedInputStream;
import java.io.BufferedOutputStream;
import java.io.DataInputStream;
import java.io.DataOutputStream;
import java.io.IOException;
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
    /** One reply writer and one UTF-8 buffer for every reply: no garbage per request. */
    private final Json.Out reply = new Json.Out();
    private byte[] utf8 = new byte[1 << 16];
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
        Json.Out display = new Json.Out().open();
        try {
            screen.info(display);
        } catch (Exception e) {
            // the host asks for `info` itself
        }
        System.out.println("listening on 127.0.0.1:" + port + " " + display.close());
        System.out.flush();
        while (true) {
            try (Socket s = ss.accept()) {
                serve(s);
            } catch (IOException e) {
                // the host went away; wait for the next one
            }
        }
    }

    private void serve(Socket s) throws IOException {
        s.setTcpNoDelay(true);
        s.setSoTimeout(5000);
        DataInputStream in = new DataInputStream(new BufferedInputStream(s.getInputStream()));
        // Replies are written in one piece from a reused buffer: a small stream buffer is enough.
        DataOutputStream out = new DataOutputStream(new BufferedOutputStream(s.getOutputStream(), 8192));
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
            // The host is done and does not wait for an answer.
            if ("quit".equals(req.get("op"))) System.exit(0);
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
            out.writeInt(n);
            out.write(utf8, 0, n);
            if (blob != null) {
                out.writeInt(blob.size());
                blob.writeTo(out);
                blob = null;
            }
            out.flush();
            reply.shrink();
            if (utf8.length > MAX_KEPT) utf8 = new byte[1 << 16];
        }
    }

    /**
     * `s` as UTF-8 in {@link #utf8}, reused across replies: no String or byte[] copy of each
     * reply. A lone surrogate becomes '?', as String.getBytes does.
     */
    private int encode(StringBuilder s) {
        int len = s.length();
        if (utf8.length < len * 3) utf8 = new byte[len * 3];
        byte[] b = utf8;
        int n = 0;
        for (int i = 0; i < len; i++) {
            char c = s.charAt(i);
            if (c < 0x80) {
                b[n++] = (byte) c;
            } else if (c < 0x800) {
                b[n++] = (byte) (0xc0 | (c >> 6));
                b[n++] = (byte) (0x80 | (c & 0x3f));
            } else if (Character.isHighSurrogate(c) && i + 1 < len && Character.isLowSurrogate(s.charAt(i + 1))) {
                int cp = Character.toCodePoint(c, s.charAt(++i));
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

    private static String readFrame(DataInputStream in) throws IOException {
        int len = in.readInt();
        if (len < 0 || len > (16 << 20)) throw new IOException("bad frame length " + len);
        byte[] b = new byte[len];
        in.readFully(b);
        return new String(b, StandardCharsets.UTF_8);
    }

    private void handle(Map<String, Object> r, Json.Out o) throws Exception {
        String op = (String) r.get("op");
        switch (op) {
            case "ping":
                break;
            case "info":
                screen.info(o);
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
                if (bool(r, "fresh")) screen.clearCache();
                o.put("seq", settle.seq());
                screen.dump(o, bool(r, "all"));
                break;
            }
            case "tap":
                input.tap(num(r, "x"), num(r, "y"));
                break;
            case "press":
                input.press(num(r, "x"), num(r, "y"), num(r, "ms"));
                break;
            case "swipe":
                input.swipe(num(r, "x1"), num(r, "y1"), num(r, "x2"), num(r, "y2"), num(r, "ms"), bool(r, "fling"));
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
                input.erase(num(r, "count"));
                break;
            case "mark":
                settle.mark();
                break;
            case "settle":
                settle.waitSettled(num(r, "quiet"), num(r, "nochange"), num(r, "timeout"), o);
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

    static boolean bool(Map<String, Object> r, String k) {
        return Boolean.TRUE.equals(r.get(k));
    }
}
