import { test } from '@e2e-dev/mobile';
import { expect } from 'e2e';
import { openScenario } from './open.ts';

test('Control Inventory', async ({ app, device, screen }) => {
  await openScenario({ app, screen }, 'Control Inventory');
  // One exercise per verb, a tab of controls at a time, each checked by its status line.
  await screen.getByTestId('name-input').fill('Grace Hopper');
  await device.dismissKeyboard();
  await expect(screen.getByTestId('name-echo')).toHaveText('name: Grace Hopper');
  await screen.getByTestId('tab-toggles').tap();
  await screen.getByTestId('wifi-switch').tap();
  await expect(screen.getByTestId('wifi-status')).toHaveText('wifi: on');
  await screen.getByTestId('newsletter-checkbox').tap();
  await expect(screen.getByTestId('newsletter-status')).toHaveText('newsletter: on');
  await screen.getByTestId('size-large').tap();
  await expect(screen.getByTestId('size-status')).toHaveText('size: Large');
  const delayed = screen.getByTestId('delayed-button');
  await screen.scrollUntilVisible(delayed, { direction: 'down' });
  await expect(delayed).toBeEnabled();
  await delayed.tap();
  await expect(screen.getByTestId('delayed-status')).toHaveText('delayed: pressed');
  await screen.getByTestId('tab-gestures').tap();
  await screen.getByTestId('long-press-target').longPress();
  await expect(screen.getByTestId('gesture-status')).toHaveText('gesture: long press');
  await screen.getByTestId('double-tap-target').doubleTap();
  await expect(screen.getByTestId('tap-count')).toHaveText(/^2 taps/);
  const pad = screen.getByTestId('tap-pad');
  await screen.scrollUntilVisible(pad, { direction: 'down' });
  await pad.tap();
  await expect(screen.getByTestId('pad-status')).not.toHaveText('pad: none');
  const swipe = screen.getByTestId('swipe-pad');
  await screen.scrollUntilVisible(swipe, { direction: 'down' });
  await swipe.swipe({ direction: 'left' });
  await expect(screen.getByTestId('swipe-status')).toHaveText(/^swipe: .* to /);
});
