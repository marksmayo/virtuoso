// bench/web-flows/newsletter_signup.yaml
import { text } from './support.js';

it('newsletter_signup', () => {
  cy.visit('/e/newsletter-signup');
  cy.get('[placeholder="you@example.com"]').type('maria.novak@example.com');
  text('Subscribe').click();
  cy.get('[data-testid="success-message"]').should('not.exist');
  cy.get('[data-testid="error-message"]').should('not.exist');
  cy.get('[placeholder="you@example.com"]').should('have.value', 'maria.novak@example.com');
});
