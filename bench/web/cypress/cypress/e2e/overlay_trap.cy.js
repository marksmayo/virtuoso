// bench/web-flows/overlay_trap.yaml
import { text } from './support.js';

it('overlay_trap', () => {
  cy.visit('/e/overlay-trap');
  text('Accept cookies').click();
  cy.contains(/close overlay/i).click();
  text('Continue').click();
  text('You made it past the overlay').should('be.visible');
});
