// bench/web-flows/playbook_cleanup.yaml
import { text } from './support.js';

it('playbook_cleanup', () => {
  cy.visit('/e/playbook-cleanup');
  text('Launch Approval Draft').should('be.visible');
  cy.get('button').filter((_, el) => el.textContent.trim() === '⋯').eq(1).click();
  text('Delete').click();
  text('Are you sure you want to remove "Launch Approval Draft"?').should('be.visible');
  text('Remove').click();
  cy.contains(/^\s*Launch Approval Draft\s*$/).should('not.exist');
  cy.get('[placeholder="Search"]').type('Launch Approval Draft');
  text('No playbooks match "Launch Approval Draft".').should('be.visible');
});
