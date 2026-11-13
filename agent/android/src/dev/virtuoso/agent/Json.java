package dev.virtuoso.agent;

import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;

/**
 * Just enough JSON for the protocol: requests parse into Map / List / String / Long (whole
 * numbers) / Double / Boolean / null, and replies are written with {@link Out}.
 */
final class Json {
    private final String s;
    private int i;

    private Json(String s) {
        this.s = s;
    }

    static Object parse(String s) {
        Json p = new Json(s);
        Object v = p.value();
        p.ws();
        if (p.i != s.length()) throw p.bad();
        return v;
    }

    private void ws() {
        // serde_json writes no whitespace but these, and nothing else below a space
        while (i < s.length() && s.charAt(i) <= ' ') i++;
    }

    private Object value() {
        ws();
        if (i >= s.length()) throw bad();
        char c = s.charAt(i);
        switch (c) {
            case '{': {
                i++;
                Map<String, Object> m = new HashMap<>();
                ws();
                if (s.charAt(i) == '}') { i++; return m; }
                while (true) {
                    ws();
                    String k = string();
                    ws();
                    expect(':');
                    m.put(k, value());
                    ws();
                    if (s.charAt(i) == ',') { i++; continue; }
                    expect('}');
                    return m;
                }
            }
            case '[': {
                i++;
                List<Object> l = new ArrayList<>();
                ws();
                if (s.charAt(i) == ']') { i++; return l; }
                while (true) {
                    l.add(value());
                    ws();
                    if (s.charAt(i) == ',') { i++; continue; }
                    expect(']');
                    return l;
                }
            }
            case '"':
                return string();
            case 't': i += 4; return Boolean.TRUE;
            case 'f': i += 5; return Boolean.FALSE;
            case 'n': i += 4; return null;
            default: {
                // Whole numbers (every one the host sends but a location's) without a substring
                // and a double parse.
                int start = i;
                long n = 0;
                boolean whole = true;
                for (char d; i < s.length() && "+-0123456789.eE".indexOf(d = s.charAt(i)) >= 0; i++) {
                    if (d >= '0' && d <= '9') n = n * 10 + d - '0';
                    else if (d != '-' || i != start) whole = false;
                }
                if (whole && i - start < 19) return s.charAt(start) == '-' ? -n : n;
                return Double.parseDouble(s.substring(start, i));
            }
        }
    }

    private void expect(char c) {
        if (s.charAt(i) != c) throw bad();
        i++;
    }

    /** The host only sends serde_json's output: a parse error is a protocol bug, said briefly. */
    private IllegalArgumentException bad() {
        return new IllegalArgumentException("bad json at " + i);
    }

    private String string() {
        expect('"');
        StringBuilder b = new StringBuilder();
        while (true) {
            char c = s.charAt(i++);
            if (c == '"') return b.toString();
            if (c != '\\') { b.append(c); continue; }
            char e = s.charAt(i++);
            switch (e) {
                case 'n': b.append('\n'); break;
                case 't': b.append('\t'); break;
                case 'r': b.append('\r'); break;
                case 'b': b.append('\b'); break;
                case 'f': b.append('\f'); break;
                case 'u': b.append((char) Integer.parseInt(s.substring(i, i + 4), 16)); i += 4; break;
                default: b.append(e);
            }
        }
    }

    /** A streaming writer: the caller keeps commas right with {@link #key}. */
    static final class Out {
        StringBuilder b = new StringBuilder(16384);
        private boolean first = true;

        /** Empties the writer for the next reply. */
        Out reset() { b.setLength(0); first = true; return this; }

        /** Lets go of a buffer a big reply grew past 1 MB. */
        void shrink() { if (b.capacity() > (1 << 20)) b = new StringBuilder(16384); }

        Out open() { b.append('{'); first = true; return this; }
        Out close() { b.append('}'); first = false; return this; }
        Out openArray(String key) { key(key); b.append('['); first = true; return this; }
        Out closeArray() { b.append(']'); first = false; return this; }
        /** Starts an object inside an array. */
        Out item() { if (!first) b.append(','); b.append('{'); first = true; return this; }

        /**
         * Keys go in as they are, not through {@link #str}'s escaping: every key is a constant or a
         * package name, neither of which has a character JSON escapes.
         */
        Out key(String k) {
            if (!first) b.append(',');
            first = false;
            b.append('"').append(k).append("\":");
            return this;
        }

        Out put(String k, String v) { key(k); if (v == null) b.append("null"); else str(v); return this; }
        Out put(String k, long v) { key(k); b.append(v); return this; }
        Out put(String k, boolean v) { key(k); b.append(v); return this; }

        Out str(String v) {
            b.append('"');
            for (int j = 0; j < v.length(); j++) {
                char c = v.charAt(j);
                switch (c) {
                    case '"': b.append("\\\""); break;
                    case '\\': b.append("\\\\"); break;
                    case '\n': b.append("\\n"); break;
                    case '\r': b.append("\\r"); break;
                    case '\t': b.append("\\t"); break;
                    default:
                        if (c < 0x20) b.append(String.format("\\u%04x", (int) c));
                        else b.append(c);
                }
            }
            b.append('"');
            return this;
        }

        @Override public String toString() { return b.toString(); }
    }
}
