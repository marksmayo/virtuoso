// bench/web-flows/date_picker.yaml
import { text } from './support.js';

it('date_picker', () => {
  cy.visit('/e/date-picker');
  cy.get('[placeholder="Select date"]').click();
  text('June 2026').should('be.visible');
  for (let i = 0; i < 3; i++) text('›').click();
  text('September 2026').should('be.visible');
  text('17').click();
  cy.contains(/^\s*September 2026\s*$/).should('not.exist');
  text('Confirm booking').click();
  text('Appointment booked for September 17, 2026').should('be.visible');
});
