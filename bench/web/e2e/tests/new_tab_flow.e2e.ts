import { test } from '@e2e-dev/web';
import { expect } from 'e2e';

test('new_tab_flow', async ({ browser, screen }) => {
  await browser.goto('/e/new-tab-flow');
  const tab = await browser.waitForPopup(() => screen.getByText('Open verification tab', { exact: true }).tap());
  const code = (await screen.getByText(/^TAB-\d+$/).textContent()) ?? '';
  // Closing the active tab makes the one that opened it active again.
  await tab.close();
  await screen.getByPlaceholder('Verification code').fill(code.trim());
  await screen.getByText('Verify', { exact: true }).tap();
  await expect(screen.getByText('Verified successfully', { exact: true })).toBeVisible();
});
