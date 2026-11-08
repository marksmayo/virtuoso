import { test } from '@e2e-dev/mobile';
import { expect } from 'e2e';
import { openScenario } from './open.ts';

test('Flattened Login', async ({ app, device, screen }) => {
  await openScenario({ app, screen }, 'Flattened Login');
  const fields = screen.getByRole('textbox');
  await fields.nth(0).fill('tester@tester.army');
  await fields.nth(1).fill('benchmark123');
  await device.dismissKeyboard();
  await screen.getByText('SIGN IN').tap();
  await expect(screen.getByText('Logged in successfully')).toBeVisible();
});
