// bench/web-flows/iframe_form.yaml
import { frameBody, text } from './support.js';

it('iframe_form', () => {
  cy.visit('/e/iframe-form');
  frameBody('coupon-frame')
    .contains(/^[A-Z]+-\d+$/)
    .invoke('text')
    .then((code) => {
      frameBody('checkout-frame').find('[placeholder="Coupon code"]').type(code.trim());
    });
  frameBody('checkout-frame').within((body) => text('Apply', cy.wrap(body)).click());
  // The page takes the frames down and says so itself.
  text('Coupon applied').should('be.visible');
});
