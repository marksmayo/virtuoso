import { test } from '@e2e-dev/mobile';
import { expect } from 'e2e';
import { Ctx } from './shared.ts';

test('sign in', async ({ app, device, screen }) => {
  const ctx = { app, device, screen };
  await app.clearState();
  await screen.getByTestId('onboarding-skip').tap();
  await expect(screen.getByTestId('next')).toBeDisabled();
  const phone = screen.getByTestId('phone-input');
  // Typed key by key: the field formats the number as it comes, which a fill's read-back of
  // what it set does not find.
  await phone.tap();
  await phone.pressSequentially('5550101234');
  // The number is formatted as the digits come.
  await expect(screen.getByText('(555) 010-1234')).toBeVisible();
  await expect(screen.getByTestId('next')).toBeEnabled();
  // The engine presses no Backspace on Android: the field set to what four erased digits leave.
  await phone.clear();
  await phone.pressSequentially('555010');
  await expect(screen.getByText('(555) 010')).toBeVisible();
  await phone.pressSequentially('1234');
  await screen.getByTestId('next').tap();
  await expect(screen.getByText('Sent to (555) 010-1234')).toBeVisible({ timeout: 10_000 });
  await screen.getByTestId('code-input').tap();
  await screen.getByTestId('code-input').pressSequentially('111111');
  await expect(screen.getByText('Wrong code, try again')).toBeVisible({ timeout: 5_000 });
  await screen.getByTestId('code-input').tap();
  await screen.getByTestId('code-input').pressSequentially('123456');
  await expect(screen.getByTestId('tab-home').or(screen.getByTestId('notify-title')).first()).toBeVisible({ timeout: 15_000 });
  // The app asks to send notifications: yes.
  if (await screen.getByTestId('notify-title').isVisible()) {
    await screen.getByTestId('notify-on').tap();
    // Then the system's own dialog (Android 13 and up, as iOS): allowed. Maestro's launch
    // grants every permission, so a flow never meets it there.
    const allow = screen.getByText(/^Allow$/);
    if (await allow.waitFor({ state: 'visible', timeout: 3_000 }).then(() => true, () => false)) await allow.tap();
  }
  await expect(screen.getByText('Hi, Alex')).toBeVisible();
});
