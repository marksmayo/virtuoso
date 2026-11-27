// Installed in every page before its own scripts: what a settle needs to know. The time of the
// last DOM change; work pending: requests in flight (fetch and XHR; one running longer than
// 1.5 s is a long poll and stops counting) and short timers (1 s or less, set within 1.5 s of
// the mark: a debounce or a delayed reveal; a poll re-armed for ever stops counting); finite
// animations still running (an endless spinner never ends); whether anything changed since the
// last mark (a new document is a change); whether the page has loaded (and been idle since).
// state() answers [quiet ms, pending, animations, changed, loaded]; mark() starts a new action,
// as a press, a tap or a key reaching the page does.
(() => {
  if (window.__virtuoso) return;
  const v = window.__virtuoso = { last: performance.now(), changed: true, inflight: new Map(), timers: new Map(), intervals: new Map(), n: 0, marked: performance.now() };
  // An action starts: the timers it sets from now on are its work. An interval set before it
  // (a ticker the page started as it loaded) is not, and stops counting.
  const act = () => {
    const now = v.marked = performance.now();
    for (const [id, at] of v.intervals) if (at < now) { v.timers.delete(id); v.intervals.delete(id); }
  };
  // An action starts the quiet over: a page still before it is not still after it until its
  // effects have had their time.
  v.mark = () => { act(); v.changed = false; v.last = v.marked; };
  // So does input as it reaches the page: a flow's tap or key is the action, with no mark of
  // its own. A press of the mouse, a touch that ends (a finger's tap; a scroll's drag is
  // cancelled), a key. Not a wheel or a scroll's drag: scrollUntilVisible looks again after each,
  // and a load the scroll sets going (a feed's next page) is not its to wait for.
  for (const e of ['mousedown', 'touchend', 'keydown'])
    addEventListener(e, act, true);
  // Every shadow root the page makes, closed ones too, for the screen read (which a page
  // script cannot otherwise reach): the root's mode is the page's as it asked.
  const roots = v.roots = new WeakMap(), attach = Element.prototype.attachShadow;
  Element.prototype.attachShadow = function (init) { const r = attach.call(this, init); roots.set(this, r); return r; };
  const touch = () => { v.last = performance.now(); v.changed = true; };
  new MutationObserver(touch).observe(document, { subtree: true, childList: true, attributes: true, characterData: true });
  // Input as it reaches the page starts the quiet over too: delivering a tap can take longer
  // than the quiet a settle asks for, and its effects come after it.
  for (const e of ['pointerdown', 'pointerup', 'mousedown', 'mouseup', 'click', 'touchstart', 'touchend', 'keydown', 'keyup', 'input', 'wheel', 'scroll'])
    addEventListener(e, () => { v.last = performance.now(); }, true);
  // Pointer downs and moves as they reach the page: a compressed swipe sends its next event once
  // the page has handled the last (see WebDevice::swipe). Counted from the first such swipe on:
  // a listener on every move hands each of a finger's moves to the page's main thread, and every
  // other swipe would answer that much later.
  v.downs = 0; v.moves = 0;
  v.count = () => {
    if (v.counting) return;
    v.counting = true;
    addEventListener('pointerdown', () => { v.downs++; }, true);
    addEventListener('pointermove', () => { v.moves++; }, true);
  };
  // Resolves once `n` of `what` (downs or moves) have reached the page and two of its tasks have
  // run since (what the handler set renders in one of its own), or after `cap` ms.
  v.handled = (what, n, cap) => new Promise((done) => {
    const end = performance.now() + cap;
    const look = () => v[what] >= n || performance.now() > end ? v.after(() => v.after(done)) : st(look, 1);
    look();
  });
  // Clicks as they reach the page: a phone's tap is one once Chrome has made it of the touch.
  v.clicks = 0;
  addEventListener('click', () => { v.clicks++; }, true);
  // Loaded is complete and drained since: a server-rendered page's scripts go on after its load,
  // hydrating it in task after task and changing nothing on it, and its handlers are not there
  // until they are done. A message posted to the page waits behind the tasks already queued (a
  // scheduler's chunk of work, ~5 ms); three that come straight back, one after another, find
  // nothing queued. (Not an idle callback: the benchmark's runners were slow to call one, up to
  // its cap, after pages that had long been ready.) At most half a second's wait. How long it
  // took, and whether it ended at its cap, are kept for the launch line (`idleTook`, `idleCapped`).
  v.idle = false;
  v.idleTook = null;
  v.idleCapped = false;
  const post = MessagePort.prototype.postMessage;
  const settled = () => {
    const ch = new MessageChannel(), began = performance.now();
    let sent = 0, quick = 0;
    const ping = () => { sent = performance.now(); post.call(ch.port2, 0); };
    ch.port1.onmessage = () => {
      const now = performance.now();
      quick = now - sent < 2 ? quick + 1 : 0;
      if (quick >= 3 || now - began > 500) { v.idle = true; v.idleTook = now - began; v.idleCapped = quick < 3; ch.port1.close(); } else ping();
    };
    ping();
  };
  if (document.readyState === 'complete') settled();
  else document.addEventListener('readystatechange', () => { if (document.readyState === 'complete') settled(); });
  // An action's next frames and the tasks it schedules (requested or posted within 1.5 s of the
  // mark) are work pending until they have run, while the action's 1.5 s last: a page drawing
  // in its next frame, or rendering in tasks a scheduler posts one after another (React), only
  // looks still meanwhile. A message is had once a message posted after it has (in order).
  v.frames = 0;
  v.posted = 0;
  const ours = (now) => now - v.marked <= 1500;
  // A frame asked for in a frame's callback is that loop's, begun when its first was asked
  // for: a loop the page ran before the action (an animation library's) is not the action's.
  const raf = window.requestAnimationFrame, caf = window.cancelAnimationFrame, frames = new Set();
  let loop = null;
  window.requestAnimationFrame = function (fn) {
    if (typeof fn !== 'function') return raf.call(this, fn);
    const born = loop === null ? performance.now() : loop, counts = born >= v.marked && ours(performance.now());
    let id;
    id = raf.call(this, function (...a) {
      if (frames.delete(id)) v.frames--;
      const was = loop;
      loop = born;
      try { return fn.apply(this, a); } finally { loop = was; }
    });
    if (counts) { frames.add(id); v.frames++; }
    return id;
  };
  window.cancelAnimationFrame = function (id) { if (frames.delete(id)) v.frames--; return caf.call(this, id); };
  // A task of virtuoso's own after those queued now (a landing read waits two of them for what
  // a scroll's handler rendered): posted unwrapped, it is no work of the page's.
  v.after = (fn) => { const c = new MessageChannel(); c.port1.onmessage = () => { c.port1.close(); fn(); }; post.call(c.port2, 0); };
  const had = new MessageChannel();
  had.port1.onmessage = () => { if (v.posted > 0) v.posted--; };
  MessagePort.prototype.postMessage = function (...a) {
    const r = post.apply(this, a);
    if (this !== had.port2 && ours(performance.now())) { v.posted++; post.call(had.port2, 0); }
    return r;
  };
  const track = () => { const id = ++v.n; v.inflight.set(id, performance.now()); return () => { v.inflight.delete(id); touch(); }; };
  const f = window.fetch;
  if (f) window.fetch = function (...a) { const done = track(); return f.apply(this, a).finally(done); };
  // A fetch ends with its answer's headers: the body comes after, and reading it is a request
  // still in flight (the page does nothing with the answer until it has it).
  for (const k of ['text', 'json', 'arrayBuffer', 'blob', 'formData', 'bytes']) {
    const read = window.Response && Response.prototype[k];
    if (read) Response.prototype[k] = function (...a) { const done = track(); return read.apply(this, a).finally(done); };
  }
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
  // A short interval an action started (1 s or less, set within 1.5 s of the mark) is work
  // pending until its next tick, while the action's 1.5 s last: between ticks the page only
  // looks still. A ticker started long before, or a heartbeat, does not hold a settle.
  const si = window.setInterval, ci = window.clearInterval;
  window.setInterval = function (fn, ms, ...a) {
    const now = performance.now();
    if (typeof fn !== 'function' || !(ms <= 1000) || now - v.marked > 1500) return si.call(this, fn, ms, ...a);
    let id;
    const tick = function (...b) {
      const t = performance.now();
      if (v.timers.has(id)) {
        if (t - now > 1500) { v.timers.delete(id); v.intervals.delete(id); }
        else v.timers.set(id, t + ms);
      }
      return fn.apply(this, b);
    };
    id = si.call(this, tick, ms, ...a);
    v.timers.set(id, now + ms);
    v.intervals.set(id, now);
    return id;
  };
  window.clearInterval = function (id) { v.timers.delete(id); v.intervals.delete(id); return ci.call(this, id); };
  v.state = () => {
    const now = performance.now();
    let requests = 0;
    for (const t of v.inflight.values()) if (now - t < 1500) requests++;
    // A timer is pending until just after it is due; an interval the action started, for its
    // whole 1.5 s, however late a busy runner brings its next tick.
    for (const [id, due] of v.timers) if (due + 50 > now || (v.intervals.has(id) && now - v.intervals.get(id) < 1500)) requests++;
    if (ours(now)) requests += v.frames + v.posted;
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
    return [now - v.last, requests, animations, v.changed, v.idle && document.readyState === 'complete', v.idleTook, v.idleCapped];
  };
  // A settle's wait, in the page: resolves with [state, still] once the page has been still
  // for `quiet` ms (nothing pending, loaded), or after `upTo` ms. Busy, it looks every `poll`
  // ms; idle, again when its quiet will be up. (The page's own setTimeout, not the one wrapped
  // above: this wait is not work of the page's.)
  v.until = (quiet, upTo, poll) => new Promise((done) => {
    const end = performance.now() + upTo;
    const look = () => {
      const s = v.state(), left = end - performance.now();
      const idle = s[1] === 0 && s[2] === 0 && s[4];
      if (idle && s[0] >= quiet) return done([s, true]);
      if (left <= 0) return done([s, false]);
      st(look, Math.max(1, Math.min(idle ? quiet - s[0] : poll, left)));
    };
    look();
  });
})();
