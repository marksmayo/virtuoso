// bench/web-flows/sticky_chrome.yaml
import { text } from './support.js';

it('sticky_chrome', () => {
  cy.visit('/e/sticky-chrome');
  text('Terms of Service').should('be.visible');
  cy.get('[data-testid="success-message"]').should('not.exist');
  // Cypress scrolls the button to the top of its box, under the sticky header: to the middle.
  text('Accept terms').scrollIntoView({ offset: { top: -300, left: 0 } }).click();
  text('Terms accepted').should('be.visible');
});
