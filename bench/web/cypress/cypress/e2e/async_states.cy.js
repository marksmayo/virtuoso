// bench/web-flows/async_states.yaml
import { text } from './support.js';

it('async_states', () => {
  cy.visit('/e/async-states');
  cy.get('[data-testid="initial-loading"]', { timeout: 20000 }).should('not.exist');
  text('Verify').should('be.visible').and('be.disabled');
  text('Send code').click();
  cy.contains(/^Your code is \d{4}$/)
    .invoke('text')
    .then((toast) => {
      cy.get('[placeholder="Verification code"]').type(toast.match(/\d{4}/)[0]);
    });
  text('Verify').should('be.enabled').click();
  text('Code verified successfully').should('be.visible');
});
