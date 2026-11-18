// bench/web-flows/product_reviews.yaml
import { text } from './support.js';

it('product_reviews', () => {
  cy.visit('/e/product-reviews');
  text('Peak Trail Running Shoes').should('be.visible');
  text('$139.00 - Sizes 6 to 13 - Ships in 2 days').should('be.visible');
  for (const part of ['4.7 out of 5', 'Marta', 'Deon', 'Priya', 'Grinds beans perfectly evenly']) cy.contains(part).should('be.visible');
});
