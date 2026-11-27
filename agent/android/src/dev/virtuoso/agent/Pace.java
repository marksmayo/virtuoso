package dev.virtuoso.agent;

/**
 * How fast typing goes, and how a field that lost keys is put right: pure rules, run on the host
 * by the tests too.
 */
final class Pace {
    /** A chunk's keys take about this long at the measured pace: the next chunk then measures again. */
    static final long CHUNK_MS = 450;
    /** At most this many keys a chunk. */
    static final int MAX_CHUNK = 16;

    private Pace() {}

    /** The longest typing waits for a field to take input focus: it types anyway after. */
    static final long FOCUS_WAIT_MS = 2000;
    /** How long a WebView holding input focus is given for its page's field to report it. */
    static final long WEB_VIEW_FOCUS_MS = 200;

    /**
     * Whether typing can start, `waitedMs` into waiting for input focus: at once on an editable
     * field. A WebView holding input focus hands its keys to the page's focused field, which
     * Chromium may never report: WEB_VIEW_FOCUS_MS for it to, then types. Anything else (no
     * focus yet, a screen still opening) is given FOCUS_WAIT_MS.
     */
    static boolean canType(boolean editable, boolean webView, long waitedMs) {
        return editable || (webView && waitedMs >= WEB_VIEW_FOCUS_MS) || waitedMs >= FOCUS_WAIT_MS;
    }

    /**
     * How many keys go in one send (only its last waited for), at `perKeyMs` a key: as many as
     * fit in CHUNK_MS, 1 to MAX_CHUNK. Unmeasured (0) is the most.
     */
    static int chunkFor(long perKeyMs) {
        if (perKeyMs <= 0) return MAX_CHUNK;
        return (int) Math.max(1, Math.min(MAX_CHUNK, CHUNK_MS / perKeyMs));
    }

    /** How long typing waits for the field to show the keys it sent before it checks them. */
    static final long ECHO_WAIT_MS = 300;
    /** A field short of its keys is repaired only once its text has not changed for this long. */
    static final long ECHO_QUIET_MS = 300;
    /** A field that shows none of its keys is given this long before they all go again. */
    static final long ECHO_MAX_MS = 2000;

    /**
     * Whether typing is done waiting for the field to show its keys, `waitedMs` after they went
     * and `quietMs` after its text last changed: `before`, `want` and `shown` are lengths. At
     * once when it shows them all. A field short of them gets keys again, so only when they
     * cannot still be on their way: a slow emulator showed none of "sneaker" at 300 ms, all
     * of it went again, and the first key, late, made it "ssneaker". So: once it shows some of
     * them and has stopped changing, or has shown none for ECHO_MAX_MS.
     */
    static boolean echoDone(int before, int want, int shown, long waitedMs, long quietMs) {
        if (shown >= want || waitedMs >= ECHO_MAX_MS) return true;
        return shown > before && waitedMs >= ECHO_WAIT_MS && quietMs >= ECHO_QUIET_MS;
    }

    /**
     * Where typing `text` goes on from, in a field showing `shown` (null: it cannot be told),
     * when the agent lost while typing it may have typed some of it: past the longest start of
     * the text the field ends with. A key is not typed twice; at worst (the field ended with
     * the text's start already, and the lost agent typed nothing) one goes untyped.
     */
    static int resumeAt(String shown, String text) {
        if (shown == null) return 0;
        for (int k = Math.min(shown.length(), text.length()); k > 0; k--) {
            if (shown.endsWith(text.substring(0, k))) return k;
        }
        return 0;
    }

    /** What to do to a field that should show `before + typed` and shows `shown`. */
    static final class Repair {
        final int deletes;
        final String retype;

        Repair(int deletes, String retype) {
            this.deletes = deletes;
            this.retype = retype;
        }
    }

    /**
     * Keys lost leave a field shorter than it should be: from where `shown` and `before + typed`
     * part, delete what is shown past that and type the rest again. Null when there is nothing to
     * do: it shows it all, or more (a field that formats its text, as a phone number's dashes).
     */
    static Repair repair(String before, String typed, String shown) {
        String want = before + typed;
        if (shown.length() >= want.length()) return null;
        int p = 0;
        int n = Math.min(shown.length(), want.length());
        while (p < n && shown.charAt(p) == want.charAt(p)) p++;
        // Not into what was there before: the field's own text is not typed again.
        if (p < before.length()) return null;
        return new Repair(shown.length() - p, want.substring(p));
    }
}
