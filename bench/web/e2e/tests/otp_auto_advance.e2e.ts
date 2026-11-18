import { test } from '@e2e-dev/web';
import { expect } from 'e2e';

test('otp_auto_advance', async ({ browser, screen }) => {
  await browser.goto('/e/otp-auto-advance');
  await screen.getByTestId('otp-input-0').tap();
  await browser.keyboard.type('4');
  await expect(screen.getByTestId('otp-input-1')).toBeFocused();
  await browser.keyboard.type('9302');
  await expect(screen.getByTestId('otp-input-5')).toBeFocused();
  await browser.keyboard.type('7');
  await expect(screen.getByText('Code verified', { exact: true })).toBeVisible();
});
