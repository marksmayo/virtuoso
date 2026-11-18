import { test } from '@e2e-dev/web';
import { expect } from 'e2e';

test('aria_hidden_flow', async ({ browser, screen }) => {
  await browser.goto('/e/aria-hidden-flow');
  await expect(screen.getByText('Choose the Pro plan', { exact: true })).toBeVisible();
  await screen.getByText('Pro', { exact: true }).tap();
  await expect(screen.getByText('Pro plan - $29/mo', { exact: true })).toBeVisible();
  await screen.getByText('Confirm Pro plan', { exact: true }).tap();
  await expect(screen.getByText('Plan activated', { exact: true })).toBeVisible();
});
