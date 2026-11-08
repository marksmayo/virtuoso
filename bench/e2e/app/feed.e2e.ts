import { test } from '@e2e-dev/mobile';
import { expect } from 'e2e';
import { scrollTo, signIn } from './shared.ts';

test('feed', async ({ app, device, screen }) => {
  const ctx = { app, device, screen };
  await signIn(ctx, '5550101234', '123456', false);
  await expect(screen.getByText(/^Post 1: /)).toBeVisible({ timeout: 10_000 });
  await expect(screen.getByTestId('announcement')).toBeVisible();
  // A banner arrives late and pushes the feed down.
  await expect(screen.getByTestId('promo-banner')).toBeVisible({ timeout: 5_000 });
  await screen.getByTestId('like-1').tap();
  await expect(screen.getByText(/^Liked · /)).toBeVisible();
  await screen.getByTestId('photo-2').doubleTap();
  // The like button sits under the tab bar: bring it up.
  const like2 = screen.getByTestId('like-2');
  await screen.scrollUntilVisible(like2, { direction: 'down' });
  await expect(like2).toBeSelected();
  // The first page has 20 posts: the next ones load at the end of the list.
  await scrollTo(screen, screen.getByText(/^Post 30: /), 'down');
  const like30 = screen.getByTestId('like-30');
  await screen.scrollUntilVisible(like30, { direction: 'down' });
  await like30.tap();
  await expect(like30).toBeSelected();
  const like1 = screen.getByTestId('like-1');
  await scrollTo(screen, like1, 'up');
  // Pull to refresh (a drag down from the top), until it does.
  const refreshed = screen.getByText(/^Refreshed \d+×$/);
  for (let i = 0; i < 5 && !(await refreshed.isVisible()); i++) await screen.swipe({ direction: 'up', momentum: 'fast' });
  await expect(refreshed).toBeVisible({ timeout: 5_000 });
  await screen.swipe({ direction: 'down' });
});
