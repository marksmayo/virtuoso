import { test } from '@e2e-dev/mobile';
import { expect } from 'e2e';
import { guest, lab } from './shared.ts';

test('appearance', { video: 'off' }, async ({ app, device, screen }) => {
  const ctx = { app, device, screen };
  await guest(ctx);
  await device.setAppearance('light');
  await app.startRecording();
  await lab(screen, 'device');
  await expect(screen.getByText('Appearance: light')).toBeVisible();
  await device.setAppearance('dark');
  await expect(screen.getByText('Appearance: dark')).toBeVisible({ timeout: 5_000 });
  // No toggle: back to light.
  await device.setAppearance('light');
  await expect(screen.getByText('Appearance: light')).toBeVisible({ timeout: 5_000 });
  await app.stopRecording();
  await screen.getByTestId('header-back').tap();
  await screen.getByTestId('tab-profile').tap();
  await screen.getByTestId('settings').tap();
  await app.screenshot('settings');
  const version = screen.getByTestId('version');
  await screen.scrollUntilVisible(version, { direction: 'down' });
  await app.screenshot('version');
});
