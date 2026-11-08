import { test } from '@e2e-dev/mobile';
import { expect } from 'e2e';
import { openScenario } from './open.ts';

test('Sequential Onboarding', async ({ app, device, screen }) => {
  await openScenario({ app, screen }, 'Sequential Onboarding');
  await screen.getByTestId('email-input').fill('jane@example.com');
  await device.dismissKeyboard();
  await screen.getByTestId('continue-button').tap();
  await screen.getByTestId('first-name-input').fill('Jane');
  await device.dismissKeyboard();
  await screen.getByTestId('last-name-input').fill('Merchant');
  await device.dismissKeyboard();
  await screen.getByTestId('continue-button').tap();
  await screen.getByTestId('phone-input').fill('0612435678');
  await device.dismissKeyboard();
  await screen.getByTestId('continue-button').tap();
  await expect(screen.getByTestId('success-message')).toHaveText('Onboarding complete');
  await expect(screen.getByTestId('echo-email')).toHaveText('jane@example.com');
  await expect(screen.getByTestId('echo-name')).toHaveText('Jane Merchant');
  await expect(screen.getByTestId('echo-phone')).toHaveText('0612435678');
});
