// bench/web-flows/shadow_dom_form.yaml
import { openShadows, text } from './support.js';

it('shadow_dom_form', () => {
  cy.visit('/e/shadow-dom-form', { onBeforeLoad: openShadows });
  cy.contains(/^Access code hint:.*/, { includeShadowDom: true })
    .invoke('text')
    .then((hint) => {
      cy.get('input[placeholder="Access code"]', { includeShadowDom: true }).type(hint.match(/SHADOW-\d+/)[0]);
    });
  text('Submit').click();
  text('Access granted').should('be.visible');
});
