import { test } from '@e2e-dev/web';
import { expect } from 'e2e';

test('stale_dom', async ({ browser, screen }) => {
  await browser.goto('/e/stale-dom');
  await expect(screen.getByText('Progress: 0 / 3', { exact: true })).toBeVisible();
  for (const n of [1, 2]) {
    await screen.getByText('Tap me', { exact: true }).tap();
    await expect(screen.getByText(`Progress: ${n} / 3`, { exact: true })).toBeVisible();
  }
  await screen.getByText('Tap me', { exact: true }).tap();
  await expect(screen.getByText('Target clicked 3 times', { exact: true })).toBeVisible();
  await expect(screen.getByText('Tap me', { exact: true })).toBeHidden();
});
