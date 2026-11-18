// bench/web-flows/saved_addresses.yaml
import { text } from './support.js';

it('saved_addresses', () => {
  cy.visit('/e/saved-addresses');
  cy.get('[data-testid="delete-home"]').click();
  text('Address deleted').should('be.visible');
  cy.get('[data-testid="address-home"]').should('be.visible');
  cy.get('[data-testid="delete-home"]').click();
  text('Home').should('be.visible');
  cy.contains('12 Rosewood Lane, Portland').should('be.visible');
});
