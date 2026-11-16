import { test } from '@e2e-dev/web';
import { expect } from 'e2e';

test('async_states', async ({ browser, screen }) => {
  await browser.goto('/e/async-states');
  await expect(screen.getByTestId('initial-loading')).toBeHidden({ timeout: 20000 });
  await expect(screen.getByText('Verify', { exact: true })).toBeDisabled();
  await screen.getByText('Send code', { exact: true }).tap();
  const toast = (await screen.getByText(/^Your code is \d{4}$/).textContent()) ?? '';
  await screen.getByPlaceholder('Verification code').fill(toast.match(/\d{4}/)![0]);
  await expect(screen.getByText('Verify', { exact: true })).toBeEnabled();
  await screen.getByText('Verify', { exact: true }).tap();
  await expect(screen.getByText('Code verified successfully', { exact: true })).toBeVisible();
});
