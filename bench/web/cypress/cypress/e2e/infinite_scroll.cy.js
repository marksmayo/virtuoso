// bench/web-flows/infinite_scroll.yaml
import { scrollUntilThere, text } from './support.js';

it('infinite_scroll', () => {
  cy.visit('/e/infinite-scroll');
  scrollUntilThere('Claim', '[data-testid="feed"]');
  text('Golden Ticket').should('be.visible');
  text('Claim').click();
  text('Golden Ticket claimed').should('be.visible');
});
