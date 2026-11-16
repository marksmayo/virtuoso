import { test } from '@e2e-dev/web';
import { expect } from 'e2e';

test('newsletter_signup', async ({ browser, screen }) => {
  await browser.goto('/e/newsletter-signup');
  const email = screen.getByPlaceholder('you@example.com');
  await email.fill('maria.novak@example.com');
  await screen.getByText('Subscribe', { exact: true }).tap();
  await expect(screen.getByTestId('success-message')).toBeHidden();
  await expect(screen.getByTestId('error-message')).toBeHidden();
  expect(await email.inputValue()).toBe('maria.novak@example.com');
});
