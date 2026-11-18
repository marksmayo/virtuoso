// bench/web-flows/promo_storefront.yaml
import { text } from './support.js';

it('promo_storefront', () => {
  cy.visit('/e/promo-storefront');
  text('Accept').click();
  cy.get('[data-testid="cookie-banner"]').should('not.exist');
  text('💬').click();
  text('Hi! How can we help?').should('be.visible');
  text('Close').click();
  cy.get('[data-testid="chat-panel"]').should('not.exist');
  cy.contains('Great bass').should('be.visible');
  text('50% OFF').should('be.visible');
  cy.get('button').filter((_, el) => el.textContent.trim() === 'Buy now').first().click();
  text('Order confirmed').should('be.visible');
});
