import { test } from '@e2e-dev/web';
import { expect } from 'e2e';

test('kanban_board', async ({ browser, screen }) => {
  await browser.goto('/e/kanban-board');
  // A pointer-event board: e2e's drag from the card to the column.
  await screen.getByTestId('card-fix-payment-bug').dragTo(screen.getByTestId('column-done'));
  await expect(screen.getByTestId('column-done').getByTestId('card-fix-payment-bug')).toBeVisible();
  await screen.getByTestId('card-refactor-onboarding').dragTo(screen.getByTestId('column-in-progress'));
  await expect(screen.getByText('Update pricing page', { exact: true })).toBeVisible();
  await screen.getByText('Submit board', { exact: true }).tap();
  await expect(screen.getByText('Board matches the goal', { exact: true })).toBeVisible();
});
