// bench/web-flows/aria_hidden_flow.yaml
import { text } from './support.js';

it('aria_hidden_flow', () => {
  cy.visit('/e/aria-hidden-flow');
  text('Choose the Pro plan').should('be.visible');
  text('Pro').click();
  text('Pro plan - $29/mo').should('be.visible');
  text('Confirm Pro plan').click();
  text('Plan activated').should('be.visible');
});
