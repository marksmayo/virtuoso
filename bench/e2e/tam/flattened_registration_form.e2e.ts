import { test } from '@e2e-dev/mobile';
import { expect } from 'e2e';
import { openScenario } from './open.ts';

test('Flattened Registration Form', async ({ app, device, screen }) => {
  await openScenario({ app, screen }, 'Flattened Registration Form');
  // The fields have no label or id of their own: in order, under their captions.
  const fields = screen.getByRole('textbox');
  await fields.nth(0).fill('Jane');
  await fields.nth(1).fill('Merchant');
  await fields.nth(2).fill('jane@example.com');
  await fields.nth(3).fill('benchmark123');
  await device.dismissKeyboard();
  await screen.getByText('CREATE ACCOUNT').tap();
  await expect(screen.getByText('Account created')).toBeVisible();
});
