import { test } from '@e2e-dev/mobile';
import { expect } from 'e2e';

// Repeated taps: in and out of one screen five times.
test('taps', async ({ app, device, screen }) => {
  await app.open();
  for (let i = 0; i < 5; i++) {
    await screen.getByText('Network & internet').tap();
    await expect(screen.getByText('Internet', { exact: true })).toBeVisible();
    await device.back();
  }
});
