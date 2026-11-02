package dev.virtuoso.agent;

import java.io.ByteArrayOutputStream;
import java.io.InputStream;
import java.nio.charset.StandardCharsets;
import java.util.List;

/**
 * Runs a program as the shell user, the agent's own uid, without a round trip through adb.
 * argv is passed as is: no shell parses it, so values from a flow cannot add commands.
 */
final class Exec {
    private Exec() {}

    static void run(List<Object> argv, Json.Out o) throws Exception {
        String[] a = new String[argv.size()];
        for (int i = 0; i < a.length; i++) a[i] = String.valueOf(argv.get(i));
        Process p = new ProcessBuilder(a).redirectErrorStream(true).start();
        p.getOutputStream().close();
        ByteArrayOutputStream buf = new ByteArrayOutputStream(1 << 16);
        InputStream in = p.getInputStream();
        byte[] chunk = new byte[1 << 16];
        int n;
        while ((n = in.read(chunk)) > 0) buf.write(chunk, 0, n);
        int code = p.waitFor();
        o.put("code", code).put("out", buf.toString(StandardCharsets.UTF_8.name()));
    }
}
