import { test } from '@e2e-dev/web';
import { expect } from 'e2e';

test('deferred_dialog', async ({ browser, screen }) => {
  const seen: string[] = [];
  await browser.onDialog(async (dialog) => {
    seen.push(dialog.message);
    await dialog.accept();
  });
  await browser.goto('/e/deferred-dialog');
  await screen.getByText('Submit report', { exact: true }).doubleTap();
  await expect(screen.getByText('Report submitted after deferred confirmation', { exact: true })).toBeVisible();
  expect(seen).toEqual(['Really submit the report?']);
});
