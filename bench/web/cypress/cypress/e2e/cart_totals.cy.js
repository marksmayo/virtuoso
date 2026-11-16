// bench/web-flows/cart_totals.yaml

const reads = (id, value) => cy.get(`[data-testid="${id}"]`).should(($el) => expect($el.text().trim()).to.equal(value));

it('cart_totals', () => {
  cy.visit('/e/cart-totals');
  reads('summary-total', '$54.00');
  cy.get('[data-testid="increase-tee"]').click();
  cy.get('[data-testid="increase-tee"]').click();
  reads('quantity-tee', '3');
  reads('line-total-tee', '$60.00');
  reads('summary-subtotal', '$49.00');
  reads('summary-total', '$54.00');
});
