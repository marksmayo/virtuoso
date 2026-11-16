// Helpers the ports share. `text(t)`: the element whose own text is exactly t (trimmed), as the
// other tools' ports find text.
const escape = (s) => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
export const text = (t) => cy.contains(new RegExp(`^\\s*${escape(t)}\\s*$`));
