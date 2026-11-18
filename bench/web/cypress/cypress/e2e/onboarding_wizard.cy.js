// bench/web-flows/onboarding_wizard.yaml
import { text } from './support.js';

// The fields have no label, id or placeholder of their own: each by the div that names it, just
// before it; a toggle is the div after its label, in its row.
const after = (label, tag = 'input') => text(label).nextAll(tag).first();

it('onboarding_wizard', () => {
  cy.visit('/e/onboarding-wizard');
  text('Step 1 of 5').should('be.visible');
  after('Full name').type('Maria Novak');
  after('Email').type('maria.novak@example.com');
  after('Phone').type('+48 601 222 333');
  text('Next').click();
  text('Step 2 of 5').should('be.visible');
  after('Street').type('Krucza 12');
  after('City').type('Warsaw');
  after('Postal code').type('00-585');
  text('Choose').click();
  text('Poland').click();
  text('Next').click();
  text('Step 3 of 5').should('be.visible');
  text('June 2026').should('be.visible');
  for (let i = 0; i < 3; i++) text('«').click();
  for (let i = 0; i < 6; i++) text('‹').click();
  for (let i = 0; i < 3; i++) text('‹ month').click();
  text('March 1990').should('be.visible');
  text('14').click();
  text('Selected: March 14, 1990').should('be.visible');
  text('Choose').click();
  text('Product designer').click();
  for (let i = 0; i < 7; i++) text('+').click();
  text('Next').click();
  text('Step 4 of 5').should('be.visible');
  for (const label of ['Product updates', 'SMS alerts', 'Weekly digest']) after(label, 'div').click();
  text('Studio').click();
  text('Next').click();
  text('Step 5 of 5').should('be.visible');
  text('Maria Novak · maria.novak@example.com · +48 601 222 333').should('be.visible');
  text('Krucza 12, 00-585 Warsaw, Poland').should('be.visible');
  text('Born March 14, 1990 · Product designer · 7 years').should('be.visible');
  text('Plan: Studio').should('be.visible');
  after('Support PIN from the brief').type('8412');
  after('I confirm the details are correct', 'div').click();
  text('Finish').click();
  text('Onboarding complete').should('be.visible');
  text('Welcome aboard, Maria Novak').should('be.visible');
});
