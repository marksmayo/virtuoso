// The screen as virtuoso reads it: every rendered element in document order, through open
// shadow roots and same-origin iframes, as
// [depth, left, top, right, bottom, text, id, hint, tag, flags]. Text is the element's own
// (its direct text, or a field's value): text inside inline tags stays theirs, as Maestro's web
// mode reads it. The id is `id`, else `data-testid`. Flags as screen.rs numbers them.
// A frame from another origin, whose document this script cannot read, is listed in `xo` as
// [its node's index, its URL, its content's left, top]: virtuoso reads it in its own context.
(() => {
  const CLICKABLE = 1, ENABLED = 2, FOCUSED = 4, SCROLLABLE = 8, CHECKED = 16, VISIBLE = 32,
    SELECTED = 64, EDITABLE = 128, CHECKABLE = 256, PASSWORD = 1024;
  const out = [], xo = [];
  const vw = innerWidth, vh = innerHeight;
  const TEXTY = /^(text|search|email|url|tel|password|number|date|datetime-local|month|time|week)$/;
  const ROLES = /^(button|link|checkbox|radio|switch|tab|menuitem|menuitemcheckbox|menuitemradio|option|combobox|textbox|slider|treeitem)$/;
  const active = (() => { let a = document.activeElement; while (a && a.shadowRoot && a.shadowRoot.activeElement) a = a.shadowRoot.activeElement; return a; })();
  const own = (el) => {
    const tag = el.localName;
    if (tag === 'input') return el.type === 'checkbox' || el.type === 'radio' ? '' : el.value || '';
    if (tag === 'textarea') return el.value || '';
    if (tag === 'select') return el.selectedIndex >= 0 ? el.options[el.selectedIndex].text : '';
    let t = '';
    for (const n of el.childNodes) if (n.nodeType === 3) t += n.data;
    return t.replace(/\s+/g, ' ').trim();
  };
  const walk = (el, depth, dx, dy) => {
    const cs = getComputedStyle(el);
    if (cs.display === 'none') return;
    const shown = cs.visibility !== 'hidden' && cs.visibility !== 'collapse' && el.getClientRects().length > 0;
    let d = depth, at = -1;
    if (shown && el.localName !== 'script' && el.localName !== 'style') {
      at = out.length;
      const r = el.getBoundingClientRect();
      const left = Math.round(r.left + dx), top = Math.round(r.top + dy), right = Math.round(r.right + dx), bottom = Math.round(r.bottom + dy);
      const tag = el.localName, role = el.getAttribute('role') || '', type = (el.type || '').toLowerCase();
      let f = 0;
      const field = (tag === 'input' && (TEXTY.test(type) || type === '')) || tag === 'textarea' || el.isContentEditable;
      if ((tag === 'a' && el.hasAttribute('href')) || tag === 'button' || tag === 'select' || tag === 'summary' || tag === 'label' ||
          (tag === 'input' && type !== 'hidden') || tag === 'textarea' || ROLES.test(role) || el.onclick || cs.cursor === 'pointer' || el.isContentEditable) f |= CLICKABLE;
      if (!el.disabled && el.getAttribute('aria-disabled') !== 'true') f |= ENABLED;
      if (el === active) f |= FOCUSED;
      if ((cs.overflowY === 'auto' || cs.overflowY === 'scroll') && el.scrollHeight > el.clientHeight ||
          (cs.overflowX === 'auto' || cs.overflowX === 'scroll') && el.scrollWidth > el.clientWidth) f |= SCROLLABLE;
      const checkable = type === 'checkbox' || type === 'radio' || /^(checkbox|radio|switch|menuitemcheckbox|menuitemradio)$/.test(role);
      if (checkable) f |= CHECKABLE;
      if (el.checked || el.getAttribute('aria-checked') === 'true') f |= CHECKED;
      if (el.selected || el.getAttribute('aria-selected') === 'true') f |= SELECTED;
      if (field) f |= EDITABLE;
      if (type === 'password') f |= PASSWORD;
      if (right > 0 && bottom > 0 && left < vw && top < vh && cs.opacity !== '0') f |= VISIBLE;
      out.push([depth, left, top, right, bottom, own(el), el.id || el.getAttribute('data-testid') || '', el.getAttribute('placeholder') || '', tag, f]);
      d = depth + 1;
    }
    if (el.shadowRoot) for (const c of el.shadowRoot.children) walk(c, d, dx, dy);
    if (el.localName === 'iframe') {
      try {
        const doc = el.contentDocument;
        if (doc && doc.documentElement) {
          const r = el.getBoundingClientRect();
          walk(doc.documentElement, d, dx + r.left + el.clientLeft, dy + r.top + el.clientTop);
        }
        else if (at >= 0) throw 0;
      } catch (e) {
        if (at >= 0) { const r = el.getBoundingClientRect(); xo.push([at, el.src, Math.round(r.left + el.clientLeft + dx), Math.round(r.top + el.clientTop + dy)]); }
      }
    }
    for (const c of el.children) walk(c, d, dx, dy);
  };
  walk(document.documentElement, 0, 0, 0);
  return { w: vw, h: vh, nodes: out, xo };
})()
