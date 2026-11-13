package dev.virtuoso.agent;

import java.io.ByteArrayOutputStream;
import java.io.InputStream;
import java.nio.charset.StandardCharsets;
import java.util.Locale;

/**
 * setLocation's test-provider location, sent again every {@link #EVERY_MS} until the next one
 * or the host leaving. Sent once, it reached only the requests already made: an app that began
 * watching a moment later (a fresh emulator's location service still starting) waited for a
 * fix that never came. speedrun's agent re-sends too.
 */
final class MockLocation implements Runnable {
    static final long EVERY_MS = 2000;
    private static final String[] PROVIDERS = {"gps", "network"};

    private Thread timer;
    /** The location the timer re-sends ("lat,lon"). */
    private volatile String at;
    /** The mock_location op granted and the test providers added, for this host. */
    private boolean ready;

    synchronized void set(double lat, double lon) throws Exception {
        stop();
        if (!ready) {
            run(false, "appops", "set", "com.android.shell", "android:mock_location", "allow");
            for (String p : PROVIDERS) {
                run(false, "cmd", "location", "providers", "add-test-provider", p);
                run(false, "cmd", "location", "providers", "set-test-provider-enabled", p, "true");
            }
            ready = true;
        }
        at = String.format(Locale.ROOT, "%.7f,%.7f", lat, lon);
        send(at, true);
        // MockLocation is the timer's body itself: a lambda would be a class of its own in the dex.
        Thread t = new Thread(this, "virtuoso-location");
        t.setDaemon(true);
        t.start();
        timer = t;
    }

    @Override
    public void run() {
        while (true) {
            try {
                Thread.sleep(EVERY_MS);
                send(at, false);
            } catch (InterruptedException e) {
                return;
            } catch (Exception ignored) {
                // the next round tries again
            }
        }
    }

    /** Stops re-sending; the next host sets the providers up again. */
    synchronized void reset() {
        stop();
        ready = false;
    }

    private void stop() {
        if (timer != null) {
            timer.interrupt();
            timer = null;
        }
    }

    private static void send(String at, boolean check) throws Exception {
        for (String p : PROVIDERS) {
            run(check, "cmd", "location", "providers", "set-test-provider-location", p, "--location", at);
        }
    }

    /** Runs a program as the shell user; `check`: failing with its output when it fails. */
    private static void run(boolean check, String... argv) throws Exception {
        Process p = new ProcessBuilder(argv).redirectErrorStream(true).start();
        p.getOutputStream().close();
        ByteArrayOutputStream buf = new ByteArrayOutputStream();
        InputStream in = p.getInputStream();
        byte[] chunk = new byte[4096];
        int n;
        while ((n = in.read(chunk)) > 0) buf.write(chunk, 0, n);
        int code = p.waitFor();
        if (check && code != 0) {
            throw new IllegalStateException(String.join(" ", argv) + " failed (" + code + "): "
                    + buf.toString(StandardCharsets.UTF_8.name()).trim());
        }
    }
}
