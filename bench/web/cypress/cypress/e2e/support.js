// Helpers the ports share.
const escape = (s) => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

// The element whose text is exactly t (trimmed), as the other tools' ports find text; in `within`
// when given (an iframe's body), shadow roots included.
export const text = (t, within) => (within ? within.contains(new RegExp(`^\\s*${escape(t)}\\s*$`), { includeShadowDom: true }) : cy.contains(new RegExp(`^\\s*${escape(t)}\\s*$`), { includeShadowDom: true }));

// Scrolls `over` (a selector; the window when it does not scroll itself) a screenful at a time
// until an element whose text is exactly t is in the page: a feed or table that renders rows
// as they come. Cypress has no wheel; scrollTo by steps is its way.
// 20 s, as a flow's scrollUntilVisible has by default.
export function scrollUntilThere(t, over, timeout = 20000) {
  const end = Date.now() + timeout;
  const there = (doc) => Cypress.$(doc).find('*').filter((_, el) => el.children.length === 0 && el.textContent.trim() === t).length > 0;
  const step = () =>
    cy.get(over).then(($el) => {
      if (there($el[0].ownerDocument)) return;
      const el = $el[0];
      if (Date.now() > end) throw new Error(`${t} not found after scrolling for ${timeout} ms (scrollTop ${el.scrollTop} of ${el.scrollHeight})`);
      const own = el.scrollHeight > el.clientHeight + 1 && getComputedStyle(el).overflowY !== 'visible';
      const target = own ? cy.wrap(el) : cy.window();
      const height = own ? el.clientHeight : el.ownerDocument.defaultView.innerHeight;
      const from = own ? el.scrollTop : el.ownerDocument.defaultView.scrollY;
      target.scrollTo(0, from + height * 0.8, { duration: 0, ensureScrollable: false });
      // The browser's own scroll event comes with the next frame, which Cypress's frame of the
      // app may not draw: the event is sent as well, as Cypress users do.
      if (own) cy.wrap(el).trigger('scroll', { log: false });
      step();
    });
  step();
}

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
