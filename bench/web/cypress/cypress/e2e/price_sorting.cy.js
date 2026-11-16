// bench/web-flows/price_sorting.yaml
import { text } from './support.js';

it('price_sorting', () => {
  cy.visit('/e/price-sorting');
  for (const name of ['Trail Backpack 24L', 'Steel Water Bottle', 'Merino Hiking Socks', 'Camp Lantern', 'Titanium Mug', '$120.00', '$49.00']) {
    text(name).should('be.visible');
  }
});
