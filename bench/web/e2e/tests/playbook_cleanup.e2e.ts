import { test } from '@e2e-dev/web';
import { expect } from 'e2e';

test('playbook_cleanup', async ({ browser, screen }) => {
  await browser.goto('/e/playbook-cleanup');
  await expect(screen.getByText('Launch Approval Draft', { exact: true })).toBeVisible();
  await screen.getByText('⋯', { exact: true }).nth(1).tap();
  await screen.getByText('Delete', { exact: true }).tap();
  await expect(screen.getByText('Are you sure you want to remove "Launch Approval Draft"?', { exact: true })).toBeVisible();
  await screen.getByText('Remove', { exact: true }).tap();
  await expect(screen.getByText('Launch Approval Draft', { exact: true })).toBeHidden();
  await screen.getByPlaceholder('Search').fill('Launch Approval Draft');
  await expect(screen.getByText('No playbooks match "Launch Approval Draft".', { exact: true })).toBeVisible();
});
