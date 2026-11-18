// bench/web-flows/div_soup.yaml
import { text } from './support.js';

it('div_soup', () => {
  cy.visit('/e/div-soup');
  // The keypad comes after the code's slots, which show the digits typed: its tile is the last.
  for (const glyph of ['7', '2', '8', '‹', '9', '1', '→']) {
    cy.get('div')
      .filter((_, el) => el.children.length === 0 && el.textContent.trim() === glyph)
      .last()
      .click();
  }
  text('Access granted').should('be.visible');
});
