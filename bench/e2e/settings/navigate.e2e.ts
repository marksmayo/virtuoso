import { test } from '@e2e-dev/mobile';
import { expect } from 'e2e';

// Navigation: three screens one level down and back.
test('navigate', async ({ app, device, screen }) => {
  await app.open();
  await screen.getByText('Network & internet').tap();
  await expect(screen.getByText('Internet', { exact: true })).toBeVisible();
  await device.back();
  await screen.getByText('Connected devices').tap();
  await expect(screen.getByText('Pair new device')).toBeVisible();
  await device.back();
  await screen.getByText('Apps', { exact: true }).tap();
  await expect(screen.getByText('Recently opened apps')).toBeVisible();
  await device.back();
  await expect(screen.getByText('Notifications', { exact: true })).toBeVisible();
});
