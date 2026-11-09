import { test } from '@e2e-dev/mobile';
import { expect } from 'e2e';

// Navigation: screens one and two levels down and back. The simulator has no VoiceOver, so
// Accessibility is checked by what it does show.
test('navigate', async ({ app, device, screen }) => {
  await app.open();
  await screen.getByText('General', { exact: true }).tap();
  await expect(screen.getByText('About', { exact: true })).toBeVisible();
  await device.back();
  await screen.getByText('Accessibility', { exact: true }).tap();
  await screen.getByText('Display & Text Size').tap();
  await expect(screen.getByText('Bold Text')).toBeVisible();
  await device.back();
  await device.back();
  await expect(screen.getByText('General', { exact: true })).toBeVisible();
});
