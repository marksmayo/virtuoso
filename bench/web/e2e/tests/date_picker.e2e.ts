import { test } from '@e2e-dev/web';
import { expect } from 'e2e';

test('date_picker', async ({ browser, screen }) => {
  await browser.goto('/e/date-picker');
  await screen.getByPlaceholder('Select date').tap();
  await expect(screen.getByText('June 2026', { exact: true })).toBeVisible();
  for (let i = 0; i < 3; i++) await screen.getByText('›', { exact: true }).tap();
  await expect(screen.getByText('September 2026', { exact: true })).toBeVisible();
  await screen.getByText('17', { exact: true }).tap();
  await expect(screen.getByText('September 2026', { exact: true })).toBeHidden();
  await screen.getByText('Confirm booking', { exact: true }).tap();
  await expect(screen.getByText('Appointment booked for September 17, 2026', { exact: true })).toBeVisible();
});
