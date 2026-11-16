// bench/web-flows/virtualized_table.yaml
import { scrollUntilThere, text } from './support.js';

it('virtualized_table', () => {
  cy.visit('/e/virtualized-table');
  text('Row 1').should('be.visible');
  scrollUntilThere('Claim', '[data-testid="table-viewport"]', 140000);
  cy.contains(/^Row 4322.*/).should('be.visible');
  text('Claim').click();
  text('Golden Row claimed').should('be.visible');
});
