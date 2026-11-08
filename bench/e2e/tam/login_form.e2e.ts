import { test } from '@e2e-dev/mobile';
import { expect } from 'e2e';
import { openScenario } from './open.ts';

test('Login Form', async ({ app, device, screen }) => {
  await openScenario({ app, screen }, 'Login Form');
  await screen.getByTestId('email-input').fill('tester@tester.army');
  await screen.getByTestId('password-input').fill('benchmark123');
  await device.dismissKeyboard();
  await screen.getByTestId('login-button').tap();
  await expect(screen.getByTestId('success-message')).toHaveText('Logged in successfully');
  await screen.getByTestId('logout-button').tap();
  await expect(screen.getByTestId('login-button')).toBeVisible();
});
