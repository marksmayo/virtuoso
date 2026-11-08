import { test } from '@e2e-dev/mobile';
import { expect } from 'e2e';

// Scrolling: to the bottom of a long list for an element, into it, and back to the top.
test('scroll', async ({ app, device, screen }) => {
  await app.open();
  const about = screen.getByText('About emulated device');
  await screen.scrollUntilVisible(about, { direction: 'down' });
  await about.tap();
  await expect(screen.getByText('Device name')).toBeVisible();
  await device.back();
  await screen.scrollUntilVisible(screen.getByText('Network & internet'), { direction: 'up' });
});
