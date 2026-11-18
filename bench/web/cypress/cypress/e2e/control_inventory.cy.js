// bench/web-flows/control_inventory.yaml
import { text } from './support.js';

it('control_inventory', () => {
  cy.visit('/e/control-inventory');
  text('Agree to terms').click();
  text('Medium').click();
  text('terms agreed, size Medium').should('be.visible');
  text('Agree to terms').click();
  text('terms not agreed, size Medium').should('be.visible');
  text('Express').click();
  text('Express delivery selected').should('be.visible');
  text('Hold me').click();
  text('tapped').should('be.visible');
  // A long press: the button pressed, held past its 500 ms, released.
  text('Hold me').trigger('pointerdown', { button: 0 }).trigger('mousedown', { button: 0 });
  cy.wait(800);
  text('Hold me').trigger('pointerup', { button: 0 }).trigger('mouseup', { button: 0 });
  text('long-pressed').should('be.visible');
  text('Double-click me').click();
  text('clicked once').should('be.visible');
  text('Double-click me').dblclick();
  text('double-clicked').should('be.visible');
  text('Open details').click();
  text('on details').should('be.visible');
  cy.go('back');
  text('back on the inventory').should('be.visible');
});
