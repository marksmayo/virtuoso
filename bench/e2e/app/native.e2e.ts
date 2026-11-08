import { test } from '@e2e-dev/mobile';
import { expect } from 'e2e';
import { guest, lab } from './shared.ts';

test('native', async ({ app, device, screen }) => {
  const ctx = { app, device, screen };
  await guest(ctx);
  await lab(screen, 'native');
  const exact = (s) => screen.getByText(s, { exact: true });
  await exact('Add').tap();
  await exact('Add').tap();
  await screen.getByTestId('native-add').tap();
  await expect(screen.getByText('Count: 3')).toBeVisible();
  await exact('Details').tap();
  await expect(screen.getByTestId('native-details')).toBeVisible();
  await exact('Details').tap();
  await expect(screen.getByTestId('native-details')).toBeHidden();
  for (let i = 0; i < 2; i++) {
    await exact('Load').tap();
    await expect(screen.getByTestId('native-bar')).toBeVisible({ timeout: 5_000 });
    await expect(screen.getByTestId('native-bar')).toBeHidden({ timeout: 10_000 });
  }
  await expect(screen.getByText('Loads finished: 2')).toBeVisible();
  await exact('Card').tap();
  await exact('Card button').tap();
  await expect(screen.getByText('Card taps: 1')).toBeVisible();
  await exact('Lock').tap();
  await exact('Add').tap();
  await expect(screen.getByText('Count: 4')).toBeVisible();
  await exact('List').tap();
  await screen.scrollUntilVisible(exact('Item 80'), { direction: 'down' });
  await exact('Item 80').tap();
  await expect(screen.getByText('Picked: Item 80')).toBeVisible();
});
