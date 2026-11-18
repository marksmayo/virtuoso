// bench/web-flows/new_tab_flow.yaml
import { text } from './support.js';

it('new_tab_flow', () => {
  cy.visit('/e/new-tab-flow');
  // Cypress drives one tab only; its documented answer to window.open is a stub. The stub hands
  // the page a frame of its own to write the tab into, where the test reads it.
  cy.window().then((win) => {
    cy.stub(win, 'open').callsFake(() => {
      const frame = win.document.createElement('iframe');
      frame.id = 'opened-tab';
      win.document.body.appendChild(frame);
      return frame.contentWindow;
    });
  });
  text('Open verification tab').click();
  cy.get('#opened-tab')
    .its('0.contentDocument.body')
    .should('not.be.empty')
    .then(cy.wrap)
    .contains(/^TAB-\d+$/)
    .invoke('text')
    .then((code) => {
      cy.get('#opened-tab').invoke('remove');
      cy.get('[placeholder="Verification code"]').type(code.trim());
    });
  text('Verify').click();
  text('Verified successfully').should('be.visible');
});
