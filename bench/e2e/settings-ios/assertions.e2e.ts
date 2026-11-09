import { test } from '@e2e-dev/mobile';
import { expect } from 'e2e';

// Assertions: many checks of what one screen shows.
test('assertions', async ({ app, screen }) => {
  await app.open();
  const row = (t: string) => screen.getByText(t, { exact: true }).first();
  for (const t of ['General', 'Accessibility', 'Camera', 'Search', 'StandBy', 'Screen Time']) {
    await expect(row(t)).toBeVisible();
  }
  await expect(screen.getByTestId('com.apple.settings.general')).toBeVisible();
  await expect(screen.getByText(/^Apple Intelligence/).first()).toBeVisible();
  // No relative locators: Accessibility below General, by their boxes.
  const general = await row('General').boundingBox();
  const accessibility = await row('Accessibility').boundingBox();
  expect(general !== null && accessibility !== null && accessibility.y >= general.y + general.height).toBe(true);
  await expect(screen.getByText('This text is not on screen')).toHaveCount(0);
  for (const t of ['General', 'Accessibility', 'Camera', 'Search']) {
    await expect(row(t)).toBeVisible();
  }
});
