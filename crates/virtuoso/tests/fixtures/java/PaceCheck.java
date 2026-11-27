package dev.virtuoso.agent;

/** Runs the agent's typing pace rules on the JVM for the host's tests: each argument is a
 * case, `chunk:<per-key ms>` or `repair:<what was there>|<what was typed>|<what shows>`, and
 * each line printed is its answer (a chunk size, or `<deletes> <text to retype>`, or `none`). */
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
            } else {
                String[] p = a.substring(7).split("\\|", -1);
                Pace.Repair r = Pace.repair(p[0], p[1], p[2]);
                System.out.println(r == null ? "none" : r.deletes + " " + r.retype);
            }
        }
    }
}
