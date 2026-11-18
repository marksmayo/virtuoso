import { test } from '@e2e-dev/web';
import { expect } from 'e2e';

test('onboarding_wizard', async ({ browser, screen }) => {
  await browser.goto('/e/onboarding-wizard');
  // The fields have no label, id or placeholder of their own: each by the div that names it,
  // just before it; a toggle is the div after its label, in its row.
  const after = (label: string, tag = 'input') => browser.locator(`xpath=//div[normalize-space()="${label}"]/following-sibling::${tag}[1]`);
  const tap = (t: string) => screen.getByText(t, { exact: true }).tap();
  const see = (t: string) => expect(screen.getByText(t, { exact: true })).toBeVisible();
  await see('Step 1 of 5');
  await after('Full name').fill('Maria Novak');
  await after('Email').fill('maria.novak@example.com');
  await after('Phone').fill('+48 601 222 333');
  await tap('Next');
  await see('Step 2 of 5');
  await after('Street').fill('Krucza 12');
  await after('City').fill('Warsaw');
  await after('Postal code').fill('00-585');
  await tap('Choose');
  await tap('Poland');
  await tap('Next');
  await see('Step 3 of 5');
  await see('June 2026');
  for (let i = 0; i < 3; i++) await tap('«');
  for (let i = 0; i < 6; i++) await tap('‹');
  for (let i = 0; i < 3; i++) await tap('‹ month');
  await see('March 1990');
  await tap('14');
  await see('Selected: March 14, 1990');
  await tap('Choose');
  await tap('Product designer');
  for (let i = 0; i < 7; i++) await tap('+');
  await tap('Next');
  await see('Step 4 of 5');
  for (const label of ['Product updates', 'SMS alerts', 'Weekly digest']) await after(label, 'div').tap();
  await tap('Studio');
  await tap('Next');
  await see('Step 5 of 5');
  await see('Maria Novak · maria.novak@example.com · +48 601 222 333');
  await see('Krucza 12, 00-585 Warsaw, Poland');
  await see('Born March 14, 1990 · Product designer · 7 years');
  await see('Plan: Studio');
  await after('Support PIN from the brief').fill('8412');
  await after('I confirm the details are correct', 'div').tap();
  await tap('Finish');
  await see('Onboarding complete');
  await see('Welcome aboard, Maria Novak');
});
