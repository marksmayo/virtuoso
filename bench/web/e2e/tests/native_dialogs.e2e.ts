import { test } from '@e2e-dev/web';
import { expect } from 'e2e';

test('native_dialogs', async ({ browser, screen }) => {
  // The browser's own dialogs, answered in turn, each message checked as it opens.
  const expected: [string, string?][] = [['Place this order?'], ['Enter your gift code', 'GIFT-7'], ['Order placed. Thank you!']];
  const seen: string[] = [];
  await browser.onDialog(async (dialog) => {
    const [, answer] = expected[seen.length] ?? [];
    seen.push(dialog.message);
    await dialog.accept(answer);
  });
  await browser.goto('/e/native-dialogs');
  await screen.getByText('Place order', { exact: true }).tap();
  await expect(screen.getByText('Order placed with code GIFT-7', { exact: true })).toBeVisible();
  expect(seen).toEqual(expected.map(([message]) => message));
});
