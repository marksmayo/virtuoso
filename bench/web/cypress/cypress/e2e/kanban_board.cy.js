// bench/web-flows/kanban_board.yaml
import { text } from './support.js';

// A pointer-event board (nothing is HTML5 draggable): the pointer events sent to the card, at
// the card's centre, halfway, and the column's centre. Cypress's trigger places an event on its
// element (and the dragged card follows the pointer), so each is dispatched with its own
// coordinates, one command after another.
function drag(card, column) {
  cy.get(`[data-testid="${card}"]`).then(($card) => {
    cy.get(`[data-testid="${column}"]`).then(($column) => {
      const el = $card[0];
      const win = el.ownerDocument.defaultView;
      const from = el.getBoundingClientRect();
      const to = $column[0].getBoundingClientRect();
      const a = { x: from.left + from.width / 2, y: from.top + from.height / 2 };
      const b = { x: to.left + to.width / 2, y: to.top + to.height / 2 };
      // The card found again for each event, after a frame's pause: the board reads the drag from
      // its last render, which a real pointer's events have between them.
      const send = (type, p, buttons) =>
        cy.wait(50).get(`[data-testid="${card}"]`).then(($now) => {
          $now[0].dispatchEvent(new win.PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 1, isPrimary: true, pointerType: 'mouse', button: 0, buttons, clientX: p.x, clientY: p.y }));
        });
      send('pointerdown', a, 1);
      send('pointermove', { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 }, 1);
      send('pointermove', b, 1);
      send('pointerup', b, 0);
    });
  });
}

it('kanban_board', () => {
  // The board captures the pointer on pointerdown; a simulated pointer is no live pointer, so
  // setPointerCapture throws and the drag never starts. The capture is stubbed out (the events
  // are sent to the card already), as Cypress users do for such boards.
  cy.visit('/e/kanban-board', {
    onBeforeLoad(win) {
      cy.stub(win.Element.prototype, 'setPointerCapture');
    },
  });
  drag('card-fix-payment-bug', 'column-done');
  cy.get('[data-testid="column-done"] [data-testid="card-fix-payment-bug"]').should('be.visible');
  drag('card-refactor-onboarding', 'column-in-progress');
  text('Update pricing page').should('be.visible');
  text('Submit board').click();
  text('Board matches the goal').should('be.visible');
});
