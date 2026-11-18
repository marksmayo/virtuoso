// bench/web-flows/deferred_dialog.yaml
import { text } from './support.js';

it('deferred_dialog', () => {
  const seen = [];
  cy.on('window:confirm', (message) => {
    seen.push(message);
    return true;
  });
  cy.visit('/e/deferred-dialog');
  text('Submit report').dblclick();
  text('Report submitted after deferred confirmation').should('be.visible');
  cy.wrap(seen).should('deep.equal', ['Really submit the report?']);
});
