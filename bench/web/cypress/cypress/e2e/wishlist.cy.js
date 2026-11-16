// bench/web-flows/wishlist.yaml
import { text } from './support.js';

const reads = (id, value) => cy.get(`[data-testid="${id}"]`).should(($el) => expect($el.text().trim()).to.equal(value));

it('wishlist', () => {
  cy.visit('/e/wishlist');
  cy.get('[data-testid="save-planner"]').click();
  cy.get('[data-testid="save-notebook"]').click();
  text('Wishlist (2)').click();
  text('No saved items yet.').should('be.visible');
  cy.contains('Weekly Desk Planner').should('not.exist');
  text('Shop').click();
  reads('save-planner', '♥ Saved');
  reads('save-pen', '♡ Save');
});
