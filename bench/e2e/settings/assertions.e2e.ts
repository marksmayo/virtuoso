import { test } from '@e2e-dev/mobile';
import { expect } from 'e2e';

// Assertions: many checks of what one screen shows.
test('assertions', async ({ app, screen }) => {
  await app.open();
  for (const t of ['Network & internet', 'Connected devices', 'Apps', 'Notifications', 'Sound & vibration', 'Display & touch', 'Search Settings']) {
    await expect(screen.getByText(t, { exact: true })).toBeVisible();
  }
  await expect(screen.getByTestId('search_bar_title')).toBeVisible();
  await expect(screen.getByText(/Mobile.*hotspot/)).toBeVisible();
  await expect(screen.getByText('This text is not on screen')).toHaveCount(0);
  for (const t of ['Network & internet', 'Connected devices', 'Apps', 'Notifications']) {
    await expect(screen.getByText(t, { exact: true })).toBeVisible();
  }
});
