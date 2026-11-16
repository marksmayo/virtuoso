// bench/web-flows/login_form.yaml
import { text } from './support.js';

it('login_form', () => {
  cy.visit('/e/login-form');
  cy.get('[placeholder="Email"]').type('tester@tester.army');
  cy.get('[placeholder="Password"]').type('benchmark123');
  text('Log in').click();
  text('Logged in successfully').should('be.visible');
  text('Log out').click();
  text('Log in').should('be.visible');
});
