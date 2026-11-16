// Installed in every page before its own scripts: what a settle needs to know. The time of the
// last DOM change; work pending: requests in flight (fetch and XHR; one running longer than
// 1.5 s is a long poll and stops counting) and short timers (1 s or less, set within 1.5 s of
// the mark: a debounce or a delayed reveal; a poll re-armed for ever stops counting); finite
// animations still running (an endless spinner never ends); whether anything changed since the
// last mark (a new document is a change); whether the page has loaded.
// state() answers [quiet ms, pending, animations, changed, loaded]; mark() starts a new action.
(() => {
  if (window.__virtuoso) return;
  const v = window.__virtuoso = { last: performance.now(), changed: true, inflight: new Map(), timers: new Map(), n: 0, marked: performance.now() };
  // An action starts the quiet over: a page still before it is not still after it until its
  // effects have had their time.
  v.mark = () => { v.changed = false; v.marked = v.last = performance.now(); };
  const touch = () => { v.last = performance.now(); v.changed = true; };
  new MutationObserver(touch).observe(document, { subtree: true, childList: true, attributes: true, characterData: true });
  // Input as it reaches the page starts the quiet over too: delivering a tap can take longer
  // than the quiet a settle asks for, and its effects come after it.
  for (const e of ['pointerdown', 'pointerup', 'mousedown', 'mouseup', 'click', 'touchstart', 'touchend', 'keydown', 'keyup', 'input', 'wheel', 'scroll'])
    addEventListener(e, () => { v.last = performance.now(); }, true);
  const track = () => { const id = ++v.n; v.inflight.set(id, performance.now()); return () => { v.inflight.delete(id); touch(); }; };
  const f = window.fetch;
  if (f) window.fetch = function (...a) { const done = track(); return f.apply(this, a).finally(done); };
  const send = XMLHttpRequest.prototype.send;
  XMLHttpRequest.prototype.send = function (...a) { this.addEventListener('loadend', track()); return send.apply(this, a); };
  const st = window.setTimeout, ct = window.clearTimeout;
  window.setTimeout = function (fn, ms, ...a) {
    const now = performance.now();
    if (typeof fn !== 'function' || !(ms <= 1000) || now - v.marked > 1500) return st.call(this, fn, ms, ...a);
    let id;
    id = st.call(this, function (...b) { v.timers.delete(id); return fn.apply(this, b); }, ms, ...a);
    v.timers.set(id, now + ms);
    return id;
  };
  window.clearTimeout = function (id) { v.timers.delete(id); return ct.call(this, id); };
  v.state = () => {
    const now = performance.now();
    let requests = 0;
    for (const t of v.inflight.values()) if (now - t < 1500) requests++;
    for (const due of v.timers.values()) if (due + 50 > now) requests++;
    let animations = 0;
    try {
      for (const a of document.getAnimations()) {
        if (a.playState !== 'running') continue;
        const t = a.effect && a.effect.getComputedTiming();
        if (t && t.endTime !== Infinity) animations++;
      }
    } catch (e) { /* no Web Animations */ }
    // An animation that has just ended starts the quiet over: its end event and what the page
    // does on it come a frame later.
    if (animations < (v.anims || 0)) v.last = now;
    v.anims = animations;
    return [now - v.last, requests, animations, v.changed, document.readyState === 'complete'];
  };
})();
