// bench/web-flows/checkout_review.yaml
import { text } from './support.js';

it('checkout_review', () => {
  cy.visit('/e/checkout-review');
  text('Thanks, {{firstName}}! Review your order below.').should('be.visible');
  // Each value shares its line with a bold label ("Delivery:", "Ship to:"): found by its text.
  cy.contains('Arrives by undefined').should('be.visible');
  cy.contains('221B Baker Street, London').should('be.visible');
  text('Place order - $123.00').click();
  text('Order placed successfully').should('be.visible');
  cy.contains(/Order #10482/).should('be.visible');
});
