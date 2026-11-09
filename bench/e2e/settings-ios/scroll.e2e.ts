import { test } from '@e2e-dev/mobile';
import { expect } from 'e2e';

// Scrolling: down a long list for an element, into it, and back to the top.
test('scroll', async ({ app, device, screen }) => {
  await app.open();
  const privacy = screen.getByText('Privacy & Security', { exact: true });
  // On screen is not enough: the toolbar with the search field covers the list's last visible
  // row, Privacy & Security at first. On to the row after it, so it clears the toolbar.
  await screen.scrollUntilVisible(screen.getByText('Game Center', { exact: true }), { direction: 'down' });
  await privacy.tap();
  await expect(screen.getByText('Location Services')).toBeVisible();
  await device.back();
  await screen.scrollUntilVisible(screen.getByText('General', { exact: true }), { direction: 'up' });
});
