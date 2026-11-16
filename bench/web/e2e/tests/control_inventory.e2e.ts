import { test } from '@e2e-dev/web';
import { expect } from 'e2e';

test('control_inventory', async ({ browser, screen }) => {
  await browser.goto('/e/control-inventory');
  const tap = (t: string) => screen.getByText(t, { exact: true }).tap();
  const see = (t: string) => expect(screen.getByText(t, { exact: true })).toBeVisible();
  await tap('Agree to terms');
  await tap('Medium');
  await see('terms agreed, size Medium');
  await tap('Agree to terms');
  await see('terms not agreed, size Medium');
  await tap('Express');
  await see('Express delivery selected');
  await tap('Hold me');
  await see('tapped');
  await screen.getByText('Hold me', { exact: true }).longPress();
  await see('long-pressed');
  await tap('Double-click me');
  await see('clicked once');
  await screen.getByText('Double-click me', { exact: true }).doubleTap();
  await see('double-clicked');
  await tap('Open details');
  await see('on details');
  await browser.back();
  await see('back on the inventory');
});
