// bench/web-flows/order_history.yaml
import { text } from './support.js';

it('order_history', () => {
  cy.visit('/e/order-history');
  text('Orders').click();
  text('Loading your orders...').should('be.visible');
  text('Settings').click();
  cy.get('[data-testid="settings-panel"]').should('be.visible');
  text('Orders').click();
  text('Loading your orders...').should('be.visible');
});
