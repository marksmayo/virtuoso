// The screen as virtuoso reads it: every rendered element in document order, through shadow
// roots (closed ones too, as the tracker keeps them) and same-origin iframes, as
// [depth, left, top, right, bottom, text, id, hint, tag, flags]. Text is the element's own
// (its direct text, or a field's value): text inside inline tags stays theirs, as Maestro's web
// mode reads it. The id is `id`, else `data-testid`. Flags as screen.rs numbers them.
// A frame from another origin, whose document this script cannot read, is listed in `xo` as
// [its node's index, its URL, its content's left, top]: virtuoso reads it in its own context.
(() => {
  const CLICKABLE = 1, ENABLED = 2, FOCUSED = 4, SCROLLABLE = 8, CHECKED = 16, VISIBLE = 32,
    SELECTED = 64, EDITABLE = 128, CHECKABLE = 256, PASSWORD = 1024;
  const out = [], xo = [];
  // The screen is what of the page shows: on a phone, the visual viewport, which the keyboard
  // shortens and scrolls (Chrome lays the keyboard over the page, leaves innerHeight as it was,
  // and moves what shows to keep the focused field in sight). Bounds are in its terms, as a
  // tap's point is.
  const vv = window.visualViewport;
  const ox = vv ? vv.offsetLeft : 0, oy = vv ? vv.offsetTop : 0;
  const vw = innerWidth, vh = vv ? Math.min(innerHeight, Math.round(vv.height)) : innerHeight;
  const TEXTY = /^(text|search|email|url|tel|password|number|date|datetime-local|month|time|week)$/;
  const ROLES = /^(button|link|checkbox|radio|switch|tab|menuitem|menuitemcheckbox|menuitemradio|option|combobox|textbox|slider|treeitem)$/;
  const CHECKY = /^(checkbox|radio|switch|menuitemcheckbox|menuitemradio)$/;
  // Each style and property read once an element: none changes while the screen is read.
  const page = document.scrollingElement;
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
  // Whether another element is on top at (x, y) in `el`'s own frame (a fixed footer, an
  // overlay): not one of its own, nor one it is inside (an element that takes no pointer gives
  // the point to what is under it, its parent's box; that is not covering it).
  const covered = (el, x, y) => {
    const root = el.getRootNode();
    const hit = (root.elementFromPoint ? root : el.ownerDocument).elementFromPoint(x, y);
    if (!hit || hit === el) return false;
    const inner = el.shadowRoot || (window.__virtuoso && __virtuoso.roots && __virtuoso.roots.get(el));
    return !(el.contains(hit) || hit.contains(el) || (inner && inner.contains(hit)));
  };
  // `clip` is what of the page the boxes around an element let show, in the window's terms:
  // [left, top, right, bottom]. A box that scrolls or clips its content narrows it for what is
  // inside; a fixed element escapes its boxes, back to the window.
  const WINDOW = [0, 0, vw, vh];
  const walk = (el, depth, dx, dy, clip) => {
    const cs = getComputedStyle(el);
    if (cs.display === 'none') return;
    const vis = cs.visibility;
    const shown = vis !== 'hidden' && vis !== 'collapse' && el.getClientRects().length > 0;
    if (cs.position === 'fixed') clip = WINDOW;
    let d = depth, at = -1;
    let inner = clip;
    if (shown && el.localName !== 'script' && el.localName !== 'style') {
      at = out.length;
      const r = el.getBoundingClientRect();
      const left = Math.round(r.left + dx), top = Math.round(r.top + dy), right = Math.round(r.right + dx), bottom = Math.round(r.bottom + dy);
      const tag = el.localName, role = el.getAttribute('role') || '', type = (el.type || '').toLowerCase();
      let f = 0;
      const editable = el.isContentEditable, ovx = cs.overflowX, ovy = cs.overflowY;
      const field = (tag === 'input' && (TEXTY.test(type) || type === '')) || tag === 'textarea' || editable;
      if ((tag === 'a' && el.hasAttribute('href')) || tag === 'button' || tag === 'select' || tag === 'summary' || tag === 'label' ||
          (tag === 'input' && type !== 'hidden') || tag === 'textarea' || ROLES.test(role) || el.onclick || cs.cursor === 'pointer' || editable) f |= CLICKABLE;
      if (!el.disabled && el.getAttribute('aria-disabled') !== 'true') f |= ENABLED;
      if (el === active) f |= FOCUSED;
      if ((ovy === 'auto' || ovy === 'scroll') && el.scrollHeight > el.clientHeight ||
          (ovx === 'auto' || ovx === 'scroll') && el.scrollWidth > el.clientWidth ||
          // The page itself, when it is taller or wider than the window.
          el === page && (el.scrollHeight > innerHeight || el.scrollWidth > innerWidth)) f |= SCROLLABLE;
      const checkable = type === 'checkbox' || type === 'radio' || CHECKY.test(role);
      if (checkable) f |= CHECKABLE;
      if (el.checked || el.getAttribute('aria-checked') === 'true') f |= CHECKED;
      if (el.selected || el.getAttribute('aria-selected') === 'true') f |= SELECTED;
      if (field) f |= EDITABLE;
      if (type === 'password') f |= PASSWORD;
      // Visible: some of it shows through its boxes and the window, and at the middle of what
      // shows nothing else is on top of it.
      const sl = Math.max(left, clip[0]), st = Math.max(top, clip[1]), sr = Math.min(right, clip[2]), sb = Math.min(bottom, clip[3]);
      if (sr > sl && sb > st && cs.opacity !== '0' && !covered(el, (sl + sr) / 2 - dx, (st + sb) / 2 - dy)) f |= VISIBLE;
      // (The html and body elements' overflow is the window's own: they clip nothing more.)
      if ((ovx !== 'visible' || ovy !== 'visible') && el !== el.ownerDocument.body && el !== el.ownerDocument.documentElement) {
        const cl = r.left + el.clientLeft + dx, ct = r.top + el.clientTop + dy;
        inner = [Math.max(clip[0], cl), Math.max(clip[1], ct), Math.min(clip[2], cl + el.clientWidth), Math.min(clip[3], ct + el.clientHeight)];
      }
      // Its aria-label: its accessibility text, which a selector's text matches as Maestro's does.
      out.push([depth, left, top, right, bottom, own(el), el.id || el.getAttribute('data-testid') || '', el.getAttribute('placeholder') || '', tag, f, el.getAttribute('aria-label') || '']);
      d = depth + 1;
    }
    const root = el.shadowRoot || (window.__virtuoso && __virtuoso.roots && __virtuoso.roots.get(el));
    if (root) for (const c of root.children) walk(c, d, dx, dy, inner);
    if (el.localName === 'iframe') {
      try {
        const doc = el.contentDocument;
        if (doc && doc.documentElement) {
          const r = el.getBoundingClientRect();
          const fl = r.left + el.clientLeft + dx, ft = r.top + el.clientTop + dy;
          const frame = [Math.max(clip[0], fl), Math.max(clip[1], ft), Math.min(clip[2], fl + el.clientWidth), Math.min(clip[3], ft + el.clientHeight)];
          walk(doc.documentElement, d, fl, ft, frame);
        }
        else if (at >= 0) throw 0;
      } catch (e) {
        if (at >= 0) { const r = el.getBoundingClientRect(); xo.push([at, el.src, Math.round(r.left + el.clientLeft + dx), Math.round(r.top + el.clientTop + dy)]); }
      }
    }
    for (const c of el.children) walk(c, d, dx, dy, inner);
  };
  walk(document.documentElement, 0, -ox, -oy, WINDOW);
  // Where the visible part is in the page, and whether a field being typed into has the focus:
  // how a tap read off this screen is placed (on Chrome on Android, see WebDevice::taps).
  const a = document.activeElement;
  const typing = !!a && (a.isContentEditable || a.tagName === 'TEXTAREA' || (a.tagName === 'INPUT' && !/^(button|checkbox|radio|submit|reset|file|image|range|color)$/.test(a.type)));
  // The page's origin, the screen's app: in the same read, not a call of its own.
  return { w: vw, h: vh, nodes: out, xo, view: [ox, oy, typing], origin: location.origin };
})()
