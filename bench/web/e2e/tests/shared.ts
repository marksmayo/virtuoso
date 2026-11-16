// Closed shadow roots are out of every locator's reach; the usual answer is to have the page open
// them, before it loads (virtuoso reads them through its own hook in the page the same way).
export function openShadows() {
  const attach = Element.prototype.attachShadow;
  Element.prototype.attachShadow = function (init: ShadowRootInit) {
    return attach.call(this, { ...init, mode: 'open' });
  };
}
