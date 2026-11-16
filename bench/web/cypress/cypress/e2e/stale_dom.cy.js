// bench/web-flows/stale_dom.yaml
import { text } from './support.js';

it('stale_dom', () => {
  cy.visit('/e/stale-dom');
  text('Progress: 0 / 3').should('be.visible');
  for (const n of [1, 2]) {
    text('Tap me').click();
    text(`Progress: ${n} / 3`).should('be.visible');
  }
  text('Tap me').click();
  text('Target clicked 3 times').should('be.visible');
  cy.contains(/^\s*Tap me\s*$/).should('not.exist');
});
