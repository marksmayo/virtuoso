import { test } from '@e2e-dev/mobile';
import { expect } from 'e2e';

// App launch: a cold start of Settings, then one check that it shows.
test('launch', async ({ app, screen }) => {
  await app.open();
  await expect(screen.getByText('General', { exact: true })).toBeVisible();
});
