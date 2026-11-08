import { test } from '@e2e-dev/mobile';
import { expect } from 'e2e';

// Start-up overhead: one command that needs the device, nothing else.
test('trivial', async ({ device }) => {
  await device.home();
});
