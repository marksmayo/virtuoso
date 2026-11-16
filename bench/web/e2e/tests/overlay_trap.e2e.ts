import { test } from '@e2e-dev/web';
import { expect } from 'e2e';

test('overlay_trap', async ({ browser, screen }) => {
  await browser.goto('/e/overlay-trap');
  await screen.getByText('Accept cookies', { exact: true }).tap();
  await screen.getByText(/close overlay/i).tap();
  await screen.getByText('Continue', { exact: true }).tap();
  await expect(screen.getByText('You made it past the overlay', { exact: true })).toBeVisible();
});
