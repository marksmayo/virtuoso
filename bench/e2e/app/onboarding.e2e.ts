import { test } from '@e2e-dev/mobile';
import { expect } from 'e2e';
import { Ctx } from './shared.ts';

test('onboarding', async ({ app, device, screen }) => {
  const ctx = { app, device, screen };
  await app.clearState();
  await expect(screen.getByText('Discover')).toBeVisible();
  await expect(screen.getByText('Page 1 of 3')).toBeVisible();
  // A swipe's direction is the content's: the finger moves the other way.
  await screen.swipe({ direction: 'right' });
  await expect(screen.getByText('Page 2 of 3')).toBeVisible();
  await screen.swipe({ direction: 'right' });
  await expect(screen.getByText('Page 3 of 3')).toBeVisible();
  await expect(screen.getByText('Share')).toBeVisible();
  await screen.scrollUntilVisible(screen.getByText('Discover'), { direction: 'left' });
  await expect(screen.getByText('Page 1 of 3')).toBeVisible();
  await screen.scrollUntilVisible(screen.getByText('Share'), { direction: 'right' });
  await expect(screen.getByText('Page 3 of 3')).toBeVisible();
  await screen.getByText('Get started').tap();
  await expect(screen.getByText("Let's start with your phone number")).toBeVisible();
  // Only the first launch after an install shows the onboarding.
  await device.closeApp();
  await app.open();
  await expect(screen.getByText("Let's start with your phone number")).toBeVisible();
  await expect(screen.getByText('Discover')).toBeHidden();
});
