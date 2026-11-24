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

    /**
     * How many keys go in one send (only its last waited for), at `perKeyMs` a key: as many as
     * fit in CHUNK_MS, 1 to MAX_CHUNK. Unmeasured (0) is the most.
     */
    static int chunkFor(long perKeyMs) {
        if (perKeyMs <= 0) return MAX_CHUNK;
        return (int) Math.max(1, Math.min(MAX_CHUNK, CHUNK_MS / perKeyMs));
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
