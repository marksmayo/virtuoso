package dev.virtuoso.agent;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/**
 * Just enough JSON for the protocol: requests parse into Map / List / String / Double /
 * Boolean / null, and replies are written with {@link Out}.
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
        if (p.i != s.length()) throw new IllegalArgumentException("trailing characters in JSON");
        return v;
    }

    private void ws() {
        while (i < s.length() && Character.isWhitespace(s.charAt(i))) i++;
    }

    private Object value() {
        ws();
        if (i >= s.length()) throw new IllegalArgumentException("unexpected end of JSON");
        char c = s.charAt(i);
        switch (c) {
            case '{': {
                i++;
                Map<String, Object> m = new LinkedHashMap<>();
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
                int start = i;
                while (i < s.length() && "+-0123456789.eE".indexOf(s.charAt(i)) >= 0) i++;
                return Double.parseDouble(s.substring(start, i));
            }
        }
    }

    private void expect(char c) {
        if (s.charAt(i) != c) throw new IllegalArgumentException("expected '" + c + "' at " + i);
        i++;
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

        Out key(String k) {
            if (!first) b.append(',');
            first = false;
            str(k);
            b.append(':');
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
