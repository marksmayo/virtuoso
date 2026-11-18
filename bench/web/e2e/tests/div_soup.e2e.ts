import { test } from '@e2e-dev/web';
import { expect } from 'e2e';

test('div_soup', async ({ browser, screen }) => {
  await browser.goto('/e/div-soup');
  // The keypad comes after the code's slots, which show the digits typed: its tile is the last.
  for (const glyph of ['7', '2', '8', '‹', '9', '1', '→']) await screen.getByText(glyph, { exact: true }).last().tap();
  await expect(screen.getByText('Access granted', { exact: true })).toBeVisible();
});
