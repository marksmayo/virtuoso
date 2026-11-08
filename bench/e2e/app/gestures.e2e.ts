import { test } from '@e2e-dev/mobile';
import { expect } from 'e2e';
import { guest, lab } from './shared.ts';

test('gestures', async ({ app, device, screen }) => {
  const ctx = { app, device, screen };
  await guest(ctx);
  await lab(screen, 'gestures');
  const pad = screen.getByTestId('swipe-pad');
  // A swipe's direction is the content's: the finger moves the other way.
  for (const [finger, content, n] of [['LEFT', 'right', 1], ['RIGHT', 'left', 2], ['UP', 'down', 3], ['DOWN', 'up', 4]] as const) {
    await pad.swipe({ direction: content });
    await expect(screen.getByText(`Last swipe: ${finger} (${n})`)).toBeVisible();
  }
  const tapPad = screen.getByTestId('tap-pad');
  const box = await tapPad.boundingBox();
  await tapPad.tap({ position: { x: box.width * 0.15, y: box.height * 0.2 } });
  await expect(screen.getByText('Tapped top left')).toBeVisible();
  await tapPad.tap({ position: { x: box.width * 0.85, y: box.height * 0.8 } });
  await expect(screen.getByText('Tapped bottom right')).toBeVisible();
  await screen.getByTestId('double-tap-target').doubleTap();
  await expect(screen.getByText('Single: 0 · Double: 1')).toBeVisible();
  await screen.getByTestId('double-tap-target').tap();
  await expect(screen.getByText('Single: 1 · Double: 1')).toBeVisible();
  await screen.getByTestId('long-press-target').longPress();
  await expect(screen.getByText('Long presses: 1')).toBeVisible();
});
