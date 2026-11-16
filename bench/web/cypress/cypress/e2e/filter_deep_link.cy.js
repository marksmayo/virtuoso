// bench/web-flows/filter_deep_link.yaml
import { text } from './support.js';

it('filter_deep_link', () => {
  cy.visit('/e/filter-deep-link');
  cy.get('[placeholder="Filter products"]').type('mug');
  text('Titanium Mug').should('be.visible');
});
