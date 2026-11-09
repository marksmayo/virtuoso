import { test } from '@e2e-dev/mobile';
import { expect } from 'e2e';

// Repeated taps: in and out of one screen five times.
test('taps', async ({ app, device, screen }) => {
  await app.open();
  for (let i = 0; i < 5; i++) {
    await screen.getByText('General', { exact: true }).tap();
    await expect(screen.getByText('About', { exact: true })).toBeVisible();
    await device.back();
  }
});
