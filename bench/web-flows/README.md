# Web reference flows

Tester Army's web benchmark (tester-army/e2e, `apps/web-benchmark`, at the commit the mobile
benchmark pins), one Maestro flow per scenario: the main path of the scenario's deterministic
test, step for step, in Maestro's own syntax (no maestro-runner or virtuoso extensions). Each
opens its scenario at `/e/<slug>` with `clearState`, as the tests open them. Every other tool's
port follows these flows, not the tests.

What a flow cannot do in Maestro's syntax is left out of it, and said in a comment there; a
scenario that cannot be done at all is left out of the benchmark for every tool:

| Scenario | Left out because |
|---|---|
| rich-text-editor | It selects text with Shift+Arrow: Maestro has no key combinations. |
| hover-menu | Its menus open on hover: Maestro has no hover. |
| css-content-ui | Its labels are CSS ::before text, not the page's: Maestro can neither find nor check them. |

Steps a flow leaves out of its test (each said in the flow): filter-deep-link's URL check and
newsletter-signup's console check (Maestro reads neither), context-menu-trap's right-click (Maestro has none); price-sorting follows the test that
checks the opening order (the sorting test picks from a native `<select>`).
