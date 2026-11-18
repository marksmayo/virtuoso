import { test } from '@e2e-dev/web';
import { expect } from 'e2e';

test('login_form', async ({ browser, screen }) => {
  await browser.goto('/e/login-form');
  await screen.getByPlaceholder('Email').fill('tester@tester.army');
  await screen.getByPlaceholder('Password').fill('benchmark123');
  await screen.getByText('Log in', { exact: true }).tap();
  await expect(screen.getByText('Logged in successfully', { exact: true })).toBeVisible();
  await screen.getByText('Log out', { exact: true }).tap();
  await expect(screen.getByText('Log in', { exact: true })).toBeVisible();
});
