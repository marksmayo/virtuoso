// bench/web-flows/gift_card_purchase.yaml
import { text } from './support.js';

const reads = (id, value) => cy.get(`[data-testid="${id}"]`).should(($el) => expect($el.text().trim()).to.equal(value));

it('gift_card_purchase', () => {
  cy.visit('/e/gift-card-purchase');
  reads('total-amount', '$0.00');
  cy.get('[data-testid="amount-25"]').click();
  reads('total-amount', '$25.00');
  cy.get('[placeholder="friend@example.com"]').type('friend@example.com');
  cy.get('[data-testid="buy-button"]').click();
  text('Gift card sent to friend@example.com').should('be.visible');
  text('A $25.00 gift card is on its way.').should('be.visible');
});
