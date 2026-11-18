// bench/web-flows/debounced_search.yaml
import { text } from './support.js';

it('debounced_search', () => {
  cy.visit('/e/debounced-search');
  cy.get('[data-testid="search-input"]').type('trail mix');
  text('Trail Mix 500 g').click();
  text('Added Trail Mix 500 g to the cart').should('be.visible');
});
