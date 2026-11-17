# Web reference flows

Tester Army's web benchmark (tester-army/e2e, `apps/web-benchmark`, at the commit the mobile
benchmark pins), one Maestro flow per scenario: the main path of the scenario's deterministic
test (for a scenario with a planted bug, the test that asserts the bug), step for step, in Maestro's own syntax (no maestro-runner or virtuoso extensions). Each
opens its scenario at `/e/<slug>` with `clearState`, as the tests open them. Every other tool's
port follows these flows, not the tests.

What a flow cannot do in Maestro's syntax is left out of it, and said in a comment there; a
scenario that cannot be done at all is left out of the benchmark for every tool:

| Scenario | Left out because |
|---|---|
| rich-text-editor | It selects text with Shift+Arrow: Maestro has no key combinations. |
| hover-menu | Its menus open on hover: Maestro has no hover. |
| css-content-ui | Its labels are CSS ::before text, not the page's: Maestro can neither find nor check them. |
| canvas-only, canvas-whiteboard | Their state is only in the canvas's pixels: Maestro can tap a point but not read a pixel, so a flow could not fail. |
| image-only-ui | Images with empty alt text and hidden icons: nothing to find or check. |
| drag-and-drop | HTML5 drag and drop only: Maestro's drag is a swipe (a finger's, on the web), which does not start one. |
| file-round-trip | It downloads a file and uploads it back: Maestro has neither. |

Steps a flow leaves out of its test (each said in the flow): filter-deep-link's URL check and
newsletter-signup's console check (Maestro reads neither), context-menu-trap's right-click (Maestro has none); control-inventory's right-click, file upload and footnote check, and so its closing success; kanban-board drags with swipes between
points (Maestro has no element-to-element drag), in the 1280 x 800 window every tool runs in;
price-sorting follows the test that checks the opening order (the sorting test picks from a native `<select>`).

A phone field (the benchmark's Pixel 7, `VIRTUOSO_BENCH_EMULATE=pixel-7`) runs these same flows
in the phone's 412 × 839 viewport, but for a flow with a copy of its own in `pixel-7/`: there,
`kanban_board` alone, whose swipes go between screen points (measured in the phone's layout).
