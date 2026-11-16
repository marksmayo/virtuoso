import { test } from '@e2e-dev/web';
import { expect } from 'e2e';

test('virtualized_table', async ({ browser, screen }) => {
  await browser.goto('/e/virtualized-table');
  await expect(screen.getByText('Row 1', { exact: true })).toBeVisible();
  const claim = screen.getByText('Claim', { exact: true });
  await screen.getByTestId('table-viewport').scrollUntilVisible(claim, { timeout: 140000 });
  await expect(screen.getByText(/^Row 4322.*/)).toBeVisible();
  await claim.tap();
  await expect(screen.getByText('Golden Row claimed', { exact: true })).toBeVisible();
});
