import { test } from '@e2e-dev/mobile';
import { expect } from 'e2e';
import { openScenario } from './open.ts';

test('Text Input Variations', async ({ app, device, screen }) => {
  await openScenario({ app, screen }, 'Text Input Variations');
  const submit = screen.getByTestId('submit-button');
  await expect(submit).toBeDisabled();
  await screen.getByTestId('username-input').fill('tester');
  await screen.getByTestId('pin-input').fill('1234');
  await screen.getByTestId('notes-input').fill('Notes long enough to count');
  await device.dismissKeyboard();
  await submit.tap();
  await expect(screen.getByTestId('success-message')).toHaveText('Form submitted');
});
