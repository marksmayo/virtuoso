// bench/web-flows/lying_labels.yaml
import { text } from './support.js';

it('lying_labels', () => {
  cy.visit('/e/lying-labels');
  // Every accessible name lies: the button whose own text is Pay now, the field beside Amount.
  cy.get('button')
    .filter((_, el) => [...el.childNodes].some((n) => n.nodeType === 3 && n.textContent.trim() === 'Pay now'))
    .click();
  text('Amount').siblings('input').type('42.50');
  text('Submit payment').click();
  text('Payment sent').should('be.visible');
});
