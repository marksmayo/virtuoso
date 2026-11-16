import { test } from '@e2e-dev/web';
import { expect } from 'e2e';

test('context_menu_trap', async ({ browser, screen }) => {
  await browser.goto('/e/context-menu-trap');
  await screen.getByText('⋯', { exact: true }).nth(1).tap();
  await screen.getByText('Rename', { exact: true }).tap();
  await screen.getByLabel('New file name').fill('summary.pdf');
  await screen.getByText('Save', { exact: true }).tap();
  await expect(screen.getByText('File renamed to summary.pdf', { exact: true })).toBeVisible();
  await expect(screen.getByText('quarterly-report.pdf', { exact: true })).toBeVisible();
  await expect(screen.getByText('report.pdf', { exact: true })).toBeHidden();
});
