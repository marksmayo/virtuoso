import { test } from '@e2e-dev/mobile';
import { expect } from 'e2e';
import { openScenario } from './open.ts';
import { addReceipt, IOS } from './media.ts';

test('Photo Picker', async ({ app, device, screen }) => {
  // The engine has no addMedia: the receipt goes into the gallery over adb (on iOS, simctl).
  addReceipt();
  await openScenario({ app, screen }, 'Photo Picker');
  await screen.getByTestId('choose-photo').tap();
  // The system photo picker, newest first: the receipt just added. iOS labels a photo "Photo,
  // <date>, <time>", and the grid around the photos joins every one of those: a single cell's
  // label has two commas.
  await screen.getByLabel(IOS ? /^Photo, [^,]+, [^,]+$/ : /^Photo taken on /).first().tap();
  await expect(screen.getByTestId('success-message')).toHaveText(/^Receipt attached: /);
});
