import { test } from '@e2e-dev/web';
import { expect } from 'e2e';

test('infinite_scroll', async ({ browser, screen }) => {
  await browser.goto('/e/infinite-scroll');
  const claim = screen.getByText('Claim', { exact: true });
  await screen.getByTestId('feed').scrollUntilVisible(claim, { timeout: 20000 });
  await expect(screen.getByText('Golden Ticket', { exact: true })).toBeVisible();
  await claim.tap();
  await expect(screen.getByText('Golden Ticket claimed', { exact: true })).toBeVisible();
});
