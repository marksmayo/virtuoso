import { test } from '@e2e-dev/mobile';
import { expect } from 'e2e';
import { adb, lab } from './shared.ts';
import { fileURLToPath } from 'node:url';

test('device', async ({ app, device, screen }) => {
  const ctx = { app, device, screen };
  const APP = 'dev.speedrun.bench';
  const count = async (id, prefix) => parseInt((await screen.getByTestId(id).textContent()).replace(prefix, ''), 10);
  // Cleared first, as launchApp clearState does (over adb: clearState() would launch it too, and
  // the launch count below starts at one).
  adb('shell', 'pm', 'clear', APP);
  // Maestro's launchApp grants every permission unless told otherwise: the location, here.
  await device.openApp(APP, { relaunch: true, launchArguments: ['--es', 'greeting', 'hello', '--es', 'mode', 'bench'], permissions: { location: 'grant' } });
  await screen.getByTestId('onboarding-skip').tap();
  await screen.getByTestId('guest').tap();
  await lab(screen, 'device');
  await expect(screen.getByText('Arguments: greeting=hello, mode=bench')).toBeVisible();
  await expect(screen.getByText('Launches: 1')).toBeVisible();
  // The engine's setLocation left the app waiting for a fix: the GPS and network test providers
  // set over adb instead, as the Maestro runners do.
  const locate = (latLon) => {
    adb('shell', 'appops', 'set', 'com.android.shell', 'android:mock_location', 'allow');
    for (const p of ['gps', 'network']) {
      try { adb('shell', 'cmd', 'location', 'providers', 'add-test-provider', p); } catch {}
      adb('shell', 'cmd', 'location', 'providers', 'set-test-provider-enabled', p, 'true');
      adb('shell', 'cmd', 'location', 'providers', 'set-test-provider-location', p, '--location', latLon);
    }
  };
  locate('45.5017,-73.5673');
  await expect(screen.getByText('Location: 45.5017, -73.5673')).toBeVisible({ timeout: 10_000 });
  // No travel: the route's end.
  locate('45.5088,-73.5540');
  await expect(screen.getByText('Location: 45.5088, -73.5540')).toBeVisible({ timeout: 20_000 });
  await device.openApp(APP, { relaunch: true, permissions: { camera: 'deny' } });
  await lab(screen, 'device');
  await expect(screen.getByText(/^Camera: (denied|undetermined)$/)).toBeVisible();
  await device.setPermission('camera', 'grant');
  // Maestro's launch grants the rest too: the photos with it.
  await device.openApp(APP, { relaunch: true, permissions: { notifications: 'grant', photos: 'grant' } });
  await lab(screen, 'device');
  await expect(screen.getByText('Camera: granted')).toBeVisible();
  if (await screen.getByTestId('allow-photos').isVisible()) {
    await screen.getByTestId('allow-photos').tap();
    // The system's dialog: iOS's wordings, or Android's.
    await screen.getByText(/^(Allow Full Access|Allow Access to All Photos|Allow all|Allow)$/).first().tap();
  }
  await expect(screen.getByText(/^Photos: \d+$/)).toBeVisible();
  // No media call: the photo into the gallery over adb.
  const photo = fileURLToPath(new URL('../../../../speedrun/bench/app/flows/media/photo.png', import.meta.url));
  const name = `photo-${Date.now()}.png`;
  adb('push', photo, `/sdcard/Pictures/${name}`);
  adb('shell', 'am', 'broadcast', '-a', 'android.intent.action.MEDIA_SCANNER_SCAN_FILE', '-d', `file:///sdcard/Pictures/${name}`);
  await screen.getByTestId('refresh-device').tap();
  await expect(screen.getByText(/^Photos: [1-9]\d*$/)).toBeVisible({ timeout: 5_000 });
  const resumed = await count('resumed', 'Back in the foreground: ');
  await device.home();
  await device.openApp(APP);
  await expect(screen.getByText(`Back in the foreground: ${resumed + 1}`)).toBeVisible();
  const launches = await count('launches', 'Launches: ');
  await device.closeApp();
  await app.open();
  await lab(screen, 'device');
  await expect(screen.getByTestId('launches')).toHaveText(`Launches: ${launches + 1}`);
  await device.setOrientation('landscape-left');
  await expect(screen.getByText('Orientation: landscape')).toBeVisible({ timeout: 5_000 });
  await device.setOrientation('portrait');
  await expect(screen.getByText('Orientation: portrait')).toBeVisible({ timeout: 5_000 });
  await device.setAirplaneMode(true);
  await expect(screen.getByText('Network: offline')).toBeVisible({ timeout: 10_000 });
  await device.setAirplaneMode(false);
  await expect(screen.getByText('Network: online')).toBeVisible({ timeout: 15_000 });
});
