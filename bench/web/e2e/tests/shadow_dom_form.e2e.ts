import { test } from '@e2e-dev/web';
import { expect } from 'e2e';
import { openShadows } from './shared.ts';

test('shadow_dom_form', async ({ browser, screen }) => {
  await browser.addInitScript(openShadows);
  await browser.goto('/e/shadow-dom-form');
  const hint = (await screen.getByText(/^Access code hint:.*/).textContent()) ?? '';
  await screen.getByPlaceholder('Access code').fill(hint.match(/SHADOW-\d+/)![0]);
  await screen.getByText('Submit', { exact: true }).tap();
  await expect(screen.getByText('Access granted', { exact: true })).toBeVisible();
});
