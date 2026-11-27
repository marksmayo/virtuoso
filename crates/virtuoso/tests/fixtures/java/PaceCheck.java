package dev.virtuoso.agent;

/** Runs the agent's typing pace rules on the JVM for the host's tests: each argument is a
 * case, `chunk:<per-key ms>` or `repair:<what was there>|<what was typed>|<what shows>`, and
 * each line printed is its answer (a chunk size, or `<deletes> <text to retype>`, or `none`).
 *
 * `echo:<what was there>|<what was typed>|<ms>=<text>,<ms>=<text>...` plays typing against a
 * field that shows each text from its moment on (what was there until the first), checked
 * every 5 ms as Input.type checks it: it prints the field at the end (its last text, with the
 * repair typing made) and when the check ended, `<field>@<ms>`.
 * `resume:<what shows>|<text>` prints what typing the text again after a lost agent types. */
public final class PaceCheck {
    public static void main(String[] args) {
        for (String a : args) {
            if (a.startsWith("chunk:")) {
                System.out.println(Pace.chunkFor(Long.parseLong(a.substring(6))));
            } else if (a.startsWith("again:") || a.startsWith("why:")) {
                // again:<held|timeout|other>:<waited ms>, why:<held|timeout>
                String[] p = a.split(":");
                Throwable t = p[1].equals("held") ? new IllegalStateException("UiAutomationService x already registered!")
                        : p[1].equals("timeout") ? new java.util.concurrent.TimeoutException("Timeout while connecting UiAutomation@1")
                        : new RuntimeException("other");
                System.out.println(p[0].equals("again") ? String.valueOf(Pace.askAgain(t, Long.parseLong(p[2]))) : Pace.noUiAutomation(t));
            } else if (a.startsWith("focus:")) {
                String[] p = a.substring(6).split(":");
                System.out.println(Pace.canType(p[0].equals("editable"), p[0].equals("webview"), Long.parseLong(p[1])));
            } else if (a.startsWith("echo:")) {
                String[] p = a.substring(5).split("\\|", -1);
                System.out.println(echo(p[0], p[1], p[2].split(",")));
            } else if (a.startsWith("resume:")) {
                String[] p = a.substring(7).split("\\|", -1);
                System.out.println(p[1].substring(Pace.resumeAt(p[0], p[1])));
            } else {
                String[] p = a.substring(7).split("\\|", -1);
                Pace.Repair r = Pace.repair(p[0], p[1], p[2]);
                System.out.println(r == null ? "none" : r.deletes + " " + r.retype);
            }
        }
    }

    /** The field `timeline` describes at `ms`. */
    private static String shownAt(String before, String[] timeline, long ms) {
        String shown = before;
        for (String e : timeline) {
            int eq = e.indexOf('=');
            if (Long.parseLong(e.substring(0, eq)) <= ms) shown = e.substring(eq + 1);
        }
        return shown;
    }

    private static String echo(String before, String typed, String[] timeline) {
        int want = before.length() + typed.length();
        long now = 0, changed = 0;
        String shown = shownAt(before, timeline, now);
        while (!Pace.echoDone(before.length(), want, shown.length(), now, now - changed)) {
            now += 5;
            String next = shownAt(before, timeline, now);
            if (!next.equals(shown)) changed = now;
            shown = next;
        }
        Pace.Repair r = Pace.repair(before, typed, shown);
        String last = shownAt(before, timeline, Long.MAX_VALUE);
        String field = r == null ? last : last.substring(0, last.length() - Math.min(r.deletes, last.length())) + r.retype;
        return field + "@" + now;
    }
}
