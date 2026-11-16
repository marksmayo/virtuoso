// bench/web-flows/otp_auto_advance.yaml
import { text } from './support.js';

it('otp_auto_advance', () => {
  cy.visit('/e/otp-auto-advance');
  // Each key typed into whichever box has the focus, as a user types.
  cy.get('[data-testid="otp-input-0"]').click().type('4');
  cy.focused().should('have.attr', 'data-testid', 'otp-input-1').type('9302');
  cy.focused().should('have.attr', 'data-testid', 'otp-input-5').type('7');
  text('Code verified').should('be.visible');
});
