# The web benchmark in Firefox

The same flows, app and fields as Chrome's (`bench/web-flows`, desktop 1280 × 800 and "as a
Pixel 7"), on the same GitHub runners (Linux, Windows, macOS), in Firefox. `web-bench.yml` with
`browser: firefox` runs them; `browser: chrome` (the default) is Chrome's benchmark, unchanged.

## The Firefox

**Firefox 161.0**, the latest release (`FIREFOX_VERSION` in `web-bench.yml`; Mozilla's
`LATEST_FIREFOX_VERSION` when it was pinned). Every runner installs it the same way:
`browser-actions/setup-firefox@v1` with that version, from Mozilla's release archive, its path
handed to the tools as `VIRTUOSO_FIREFOX` (as Chrome's is `VIRTUOSO_CHROME`). Selenium's
geckodriver is pinned too (**0.37.1**, `GECKODRIVER_VERSION`), downloaded from its release by
`bench/ci/setup-web.sh` (its path in `VIRTUOSO_GECKODRIVER`).

Every tool that can drive stock Firefox drives that one binary: virtuoso, Puppeteer, Selenium and
Cypress. Playwright and e2e cannot (below).

## Each tool

| Tool | In Firefox? | How | As a Pixel 7 |
|---|---|---|---|
| virtuoso | yes, once its Firefox support lands (branch `f1-firefox`) | `--device firefox`: Firefox over WebDriver BiDi, `VIRTUOSO_FIREFOX` | its own `--emulate pixel-7` (`VIRTUOSO_EMULATE`), as far as its Firefox support goes |
| Playwright | yes, **its own Firefox build** | `firefox.launch()`: Playwright's patched Firefox (`playwright-core install firefox`), over its Juggler protocol | viewport 412 × 839, scale 2.625, touch, `isMobile`, user agent: all of it (its Firefox emulates touch: `ontouchstart`, a coarse pointer) |
| Puppeteer | yes | `puppeteer.launch({ browser: 'firefox', executablePath })`: stock Firefox over WebDriver BiDi | `page.emulate`: viewport, scale and user agent; its touch override is BiDi's `emulation.setTouchOverride`, which Firefox 161 does not have yet (Puppeteer skips it); `isMobile` has no BiDi counterpart |
| Selenium | yes | geckodriver, stock Firefox; BiDi on to size the page | geckodriver has no mobile emulation: the viewport and scale over BiDi (`browsingContext.setViewport`), the user agent by Firefox's `general.useragent.override` preference; touch as Puppeteer's (none in 161) |
| Cypress | yes | `cypress run --browser <VIRTUOSO_FIREFOX>` (Cypress drives Firefox over BiDi) | viewport and user agent, as in Chrome (Cypress emulates no touch or scale in either) |
| e2e | yes, **Playwright's Firefox** | its web engine with `browser: 'firefox'`: it launches Playwright's Firefox itself (in Chrome it attaches over CDP to the pinned Chrome; Firefox has no CDP) | viewport and user agent, as in Chrome (no touch or scale setting) |
| Maestro | **no**: sits the Firefox fields out | its web mode is chromedriver and CDP only (`CdpWebDriver`, `ChromeDriverService` in maestro-client 2.11.0) | |

The README notes Maestro's absence under each Firefox field (`VIRTUOSO_BENCH_NOT_MEASURED`).

**Firefox as a Pixel 7.** Firefox has no device emulation of Chrome's kind (one switch for screen,
scale, touch, mobile viewport and user agent). What each tool sets is in the table: every tool
gets the 412 × 839 page and the Pixel 7's user agent (`bench/web/phones.json`); the scale (2.625)
all but Cypress and e2e; touch only Playwright, through its patched build. Stock Firefox 161 has
no BiDi touch override, so Puppeteer, Selenium (and virtuoso, through BiDi) run the phone fields
with a mouse's pointer: a page that asks `ontouchstart` or `(pointer: coarse)` sees a desktop.

## Playwright's Firefox is not stock Firefox

Playwright drives Firefox through Juggler, a protocol of its own patched into its own Firefox
builds; stock Firefox does not have it. The newest Playwright (1.65.1, the one here) bundles
Firefox **159.0** (build r1572), two releases behind the 161.0 the other tools drive; e2e's
playwright-core (1.65.0) bundles the same build. So in the Firefox fields Playwright and e2e
drive a different binary from everyone else: a Gecko two releases older, with Playwright's
patches (its own input, emulation and network hooks). Their times are still Playwright's and
e2e's on Firefox, the way their users run Firefox, but not on the same binary; the README's
"Device" line for each field says so ("Firefox 161.0 (Playwright and e2e on Playwright's own
Firefox build)").

The option: Playwright 1.65 has an experimental, undocumented channel that drives stock Firefox
over WebDriver BiDi (`firefox.launch({ channel: 'moz-firefox', executablePath })`).
`VIRTUOSO_BENCH_PLAYWRIGHT_FIREFOX=stock` runs Playwright that way, on the pinned 161.0 (checked
here: it launches, sizes and scales the page and sets the user agent; no touch, as for the other
BiDi tools). It is not how Playwright users run Firefox and Playwright does not support it yet,
so it is off by default; e2e has no such option (its engine launches Playwright's Firefox with
no channel).

## Running it

    gh workflow run web-bench.yml -f browser=firefox
    gh workflow run web-bench.yml -f browser=firefox -f fields=pixel-7 -f systems=linux

Until virtuoso's Firefox support is on the branch run, leave virtuoso out
(`-f tools=playwright,puppeteer,selenium,cypress,e2e`): it would fail every run at launch.
Each job's results are `web-results-firefox-<field>-<system>-<n>`, each report
`web-report-firefox-<field>-<system>` (Chrome's keep their names). For the README:

    virtuoso-bench readme --section firefox \
      --field firefox:desktop:linux=web-firefox-desktop-linux.json \
      --field firefox:desktop:windows=web-firefox-desktop-windows.json \
      --field firefox:desktop:macos=web-firefox-desktop-macos.json
    virtuoso-bench readme --section firefox-phone --field firefox:pixel-7:linux=...

A `--field` given as `<browser>:<screen>:<runner>` is named as the README names the web fields
("Firefox (Linux runner)", "Firefox as a Pixel 7 (Linux runner)"); a new section goes in after
the README's last one.

## Locally

`VIRTUOSO_BENCH_WEB_BROWSER=firefox` with `VIRTUOSO_FIREFOX` (and `VIRTUOSO_GECKODRIVER`, or
Selenium Manager finds one) runs any tool's `run.mjs` in Firefox; `node --test
bench/web/browser.test.mjs` checks each tool's launch without a browser.
