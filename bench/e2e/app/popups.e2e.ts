import { test } from '@e2e-dev/mobile';
import { expect } from 'e2e';
import { dismissOrderPopups, guest } from './shared.ts';

test('popups', async ({ app, device, screen }) => {
  const ctx = { app, device, screen };
  await guest(ctx);
  await screen.getByTestId('tab-lab').tap();
  await screen.getByTestId('lab-popups-first').tap();
  await dismissOrderPopups(screen);
  await expect(screen.getByTestId('badge-title')).toBeHidden();
  await screen.getByTestId('lab-popups-second').tap();
  await dismissOrderPopups(screen);
  await expect(screen.getByTestId('survey-title')).toBeHidden();
  await expect(screen.getByTestId('lab-device')).toBeVisible();
  // Checks for popups that do not show share one wait (7 s, as Maestro's), not 7 s each.
  const from = Date.now();
  const left = () => Math.max(0, 7_000 - (Date.now() - from));
  const check = (l) => l.waitFor({ state: 'visible', timeout: left() }).then(() => true, () => false);
  if (await check(screen.getByText('Try Pro free for a month'))) await screen.getByText('Not now').tap();
  for (let i = 0; i < 2; i++) if (await check(screen.getByTestId('notify-title'))) await screen.getByTestId('notify-later').tap();
  expect(Date.now() - from).toBeLessThan(12_000);
});
