// bench/web-flows/context_menu_trap.yaml
import { text } from './support.js';

it('context_menu_trap', () => {
  cy.visit('/e/context-menu-trap');
  cy.get('button').filter((_, el) => el.textContent.trim() === '⋯').eq(1).click();
  text('Rename').click();
  cy.get('[aria-label="New file name"]').clear().type('summary.pdf');
  text('Save').click();
  text('File renamed to summary.pdf').should('be.visible');
  text('quarterly-report.pdf').should('be.visible');
  cy.contains(/^\s*report\.pdf\s*$/).should('not.exist');
});
