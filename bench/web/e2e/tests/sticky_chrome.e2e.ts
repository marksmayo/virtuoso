import { test } from '@e2e-dev/web';
import { expect } from 'e2e';

test('sticky_chrome', async ({ browser, screen }) => {
  await browser.goto('/e/sticky-chrome');
  await expect(screen.getByText('Terms of Service', { exact: true })).toBeVisible();
  await expect(screen.getByTestId('success-message')).toBeHidden();
  const accept = screen.getByText('Accept terms', { exact: true });
  await screen.getByTestId('scroll-container').scrollUntilVisible(accept, { timeout: 20000 });
  await accept.tap();
  await screen.getByTestId('scroll-container').scrollUntilVisible(screen.getByText('Terms accepted', { exact: true }), { direction: 'up', timeout: 20000 });
  await expect(screen.getByText('Terms accepted', { exact: true })).toBeVisible();
});
