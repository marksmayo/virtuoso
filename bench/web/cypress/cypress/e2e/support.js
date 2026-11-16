// Helpers the ports share.
const escape = (s) => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

// The element whose text is exactly t (trimmed), as the other tools' ports find text; in `within`
// when given (an iframe's body), shadow roots included.
export const text = (t, within) => (within ? within.contains(new RegExp(`^\\s*${escape(t)}\\s*$`), { includeShadowDom: true }) : cy.contains(new RegExp(`^\\s*${escape(t)}\\s*$`), { includeShadowDom: true }));

// An iframe's body, once its document has one, for `cy.wrap(...).within` or chaining.
export const frameBody = (title) => cy.get(`iframe[title="${title}"]`).its('0.contentDocument.body').should('not.be.empty').then(cy.wrap);

// Closed shadow roots are out of every query's reach; the usual answer is to have the page open
// them, before it loads (virtuoso reads them through its own hook in the page the same way).
export function openShadows(win) {
  const attach = win.Element.prototype.attachShadow;
  win.Element.prototype.attachShadow = function (init) {
    return attach.call(this, { ...init, mode: 'open' });
  };
}
