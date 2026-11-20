package dev.virtuoso.agent;

/** Runs the agent's JSON string writer on the JVM for the host's tests: for each argument (a
 * string given as hex UTF-16 code units, so lone surrogates survive the command line), a line
 * with Json.Out's string literal of it, as hex UTF-16 code units too (the console's encoding
 * stays out of it). */
public final class AgentCheck {
    public static void main(String[] args) {
        for (String hex : args) {
            StringBuilder s = new StringBuilder();
            for (int i = 0; i + 4 <= hex.length(); i += 4) s.append((char) Integer.parseInt(hex.substring(i, i + 4), 16));
            String json = new Json.Out().str(s.toString()).toString();
            StringBuilder out = new StringBuilder();
            for (int i = 0; i < json.length(); i++) out.append(String.format("%04x", (int) json.charAt(i)));
            System.out.println(out);
        }
    }
}
