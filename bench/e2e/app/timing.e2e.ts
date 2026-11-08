import { test } from '@e2e-dev/mobile';
import { expect } from 'e2e';
import { guest, lab, scene } from './shared.ts';

test('timing', async ({ app, device, screen }) => {
  const ctx = { app, device, screen };
  await guest(ctx);
  await lab(screen, 'animations');
  await scene(screen, 'confetti');
  // The button turns on 1.5 s after the screen opens: wait for it, as a flow must.
  const delayed = screen.getByTestId('scene-delayed');
  await screen.scrollUntilVisible(delayed, { direction: 'down' });
  await delayed.tap();
  await expect(screen.getByTestId('lab-tap')).toBeDisabled();
  await expect(screen.getByTestId('lab-tap')).toBeEnabled({ timeout: 5_000 });
  await screen.getByTestId('lab-tap').tap();
  await expect(screen.getByText('Taps: 1')).toBeVisible();
  await screen.getByTestId('header-back').tap();
  // A banner pushes the button down 1 s after the screen opens.
  await scene(screen, 'shift');
  // Each tap blocks the JavaScript thread for 1 s before it counts.
  const busy = screen.getByTestId('scene-busy');
  await screen.scrollUntilVisible(busy, { direction: 'down' });
  await busy.tap();
  await screen.getByTestId('lab-tap').tap();
  await expect(screen.getByText('Taps: 1')).toBeVisible();
  await screen.getByTestId('lab-tap').tap();
  await expect(screen.getByText('Taps: 2')).toBeVisible();
});
