import { test } from '@e2e-dev/mobile';
import { expect } from 'e2e';

// Text input: type into a search field, erase, type again.
test('input', async ({ app, device, screen }) => {
  await app.open();
  await screen.getByText('Search Settings').tap();
  const field = screen.getByRole('textbox');
  await field.fill('brightness level');
  await field.clear();
  await field.fill('dark theme');
  await device.dismissKeyboard();
  await device.back();
});
