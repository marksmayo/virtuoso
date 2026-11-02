---
name: virtuoso
description: Write, run and debug Maestro-compatible mobile UI flows on Android with the virtuoso CLI. Use when asked to test an Android app end to end, write a Maestro flow, or find out why a flow fails.
---

# virtuoso

virtuoso runs Maestro flows (YAML) on an Android device or emulator, fast. It reads the same
flow files as `maestro test`.

## Run

```sh
virtuoso devices                     # what is connected
virtuoso test flows/                 # every flow in a folder (config.yaml is honoured)
virtuoso test login.yaml -e USER=me  # variables, visible as ${USER}
virtuoso test flows/ --format junit --output report.xml
virtuoso test flows/ --video         # a video of every flow in the debug output folder
virtuoso lint flows/                 # check flows without a device
```

Exit status: 0 all passed, 1 a flow failed, 2 the run could not start.

## Look at the device

```sh
virtuoso hierarchy     # what is on screen: class, bounds, "text", #id, clickable
virtuoso screenshot    # screenshot.png
```

Use the labels and ids `virtuoso hierarchy` prints in selectors. `virtuoso mcp` serves the same
over the Model Context Protocol (tools: look, run, screenshot).

## Write a flow

```yaml
appId: com.example.app
---
- launchApp:
    clearState: true
- tapOn: Sign in                 # text: a case-insensitive regex over the whole label
- tapOn:
    id: email                    # resource id, or the part after the /
- inputText: ${USER}
- assertVisible: Welcome
- scrollUntilVisible:
    element: Settings
- runFlow:
    when:
      visible: Allow
    commands:
      - tapOn: Allow
```

Also: `back`, `pressKey`, `swipe`, `eraseText`, `hideKeyboard`, `extendedWaitUntil`,
`assertNotVisible`, `assertTrue`, `copyTextFrom`, `pasteText`, `setClipboard`, `takeScreenshot`,
`runScript` / `evalScript` (JavaScript, with `http`, `faker` and `require`), `repeat`, `retry`,
`openLink`, `setLocation`, `setPermissions`, `setAirplaneMode`, `setDarkMode`, and
maestro-runner's `wait`, `runShell`, `dragAndDrop`, `openNotifications`, `acceptAlert`,
`dismissAlert`, and `runFlow` with `else`.

## When a flow fails

The error names the command and the element. Run `virtuoso hierarchy` on the screen it stopped
on, compare the labels, and fix the selector (or wait for the element with
`extendedWaitUntil`). Failure screenshots go to the debug output folder.
