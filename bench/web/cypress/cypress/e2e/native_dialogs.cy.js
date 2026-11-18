// bench/web-flows/native_dialogs.yaml
import { text } from './support.js';

it('native_dialogs', () => {
  // Cypress answers the browser's own dialogs itself: confirm and alert through its events, the
  // prompt by a stub put in before the page loads.
  const seen = [];
  cy.on('window:confirm', (message) => {
    seen.push(message);
    return true;
  });
  cy.on('window:alert', (message) => seen.push(message));
  cy.visit('/e/native-dialogs', {
    onBeforeLoad(win) {
      cy.stub(win, 'prompt').callsFake((message) => {
        seen.push(message);
        return 'GIFT-7';
      });
    },
  });
  text('Place order').click();
  text('Order placed with code GIFT-7').should('be.visible');
  cy.wrap(seen).should('deep.equal', ['Place this order?', 'Enter your gift code', 'Order placed. Thank you!']);
});
