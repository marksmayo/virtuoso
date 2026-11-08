import { test } from '@e2e-dev/mobile';
import { expect } from 'e2e';
import { guest, lab } from './shared.ts';

/** The matches of a locator, with their boxes (the positional selectors' terms). */
async function boxes(l) {
  const n = await l.count();
  const out = [];
  for (let i = 0; i < n; i++) out.push({ l: l.nth(i), b: await l.nth(i).boundingBox() });
  return out;
}

test('selectors', async ({ app, device, screen }) => {
  const ctx = { app, device, screen };
  await guest(ctx);
  await lab(screen, 'selectors');
  const choose = screen.getByText('Choose');
  await choose.nth(1).tap();
  await expect(screen.getByText('Chose Plus')).toBeVisible();
  await expect(screen.getByText(/^(C)(h)(o)(s)(e) (P)(l)(u)(s)(.)?(.)?$/)).toBeVisible();
  const alternatives = [...Array(4000).keys()].map((i) => 'Plan ' + i).join('|') + '|Chose Plus';
  await expect(screen.getByText(new RegExp(`^(${alternatives})$`))).toBeVisible();
  // No relative selectors: compared by the elements' boxes. "Choose" below "$10".
  const ten = await screen.getByText('$10', { exact: true }).boundingBox();
  const belowTen = (await boxes(choose)).filter((c) => c.b.y >= ten.y + ten.height).sort((a, b) => a.b.y - b.b.y)[0];
  await belowTen.l.tap();
  await expect(screen.getByText('Chose Pro')).toBeVisible();
  await screen.getByTestId('plan-basic').getByText('Choose').tap();
  await expect(screen.getByText('Chose Basic')).toBeVisible();
  // "Basic" left of the first "Choose".
  const first = await choose.first().boundingBox();
  const basic = await screen.getByText('Basic', { exact: true }).boundingBox();
  expect(basic.x + basic.width).toBeLessThanOrEqual(first.x);
  // An element with a "Choose" child, the third; one with both "Plus" and "$10" in it.
  await expect(screen.getByRole('group').filter({ has: choose }).nth(2)).toBeVisible();
  const plus = screen.getByTestId('plan-plus');
  await expect(plus.getByText('Plus', { exact: true })).toBeVisible();
  await expect(plus.getByText('$10', { exact: true })).toBeVisible();
  const item = screen.getByText('Item', { exact: true });
  await item.nth(3).tap();
  await expect(screen.getByText('Picked item 4')).toBeVisible();
  // "Item" above the index result: the first of them.
  const result = await screen.getByTestId('index-result').boundingBox();
  const aboveResult = (await boxes(item)).filter((c) => c.b.y + c.b.height <= result.y)[0];
  await aboveResult.l.tap();
  await expect(screen.getByText('Picked item 1')).toBeVisible();
  // "Open" below "States".
  const states = await screen.getByText('States', { exact: true }).boundingBox();
  const open = (await boxes(screen.getByText('Open', { exact: true }))).filter((c) => c.b.y >= states.y + states.height)[0];
  await open.l.tap();
  await expect(screen.getByText('Open was tapped')).toBeVisible();
  await expect(screen.getByTestId('locked')).toBeDisabled();
  // A tap on it, disabled as it is: at its centre.
  const locked = await screen.getByTestId('locked').boundingBox();
  await screen.tapAt({ x: locked.x + locked.width / 2, y: locked.y + locked.height / 2 });
  await expect(screen.getByText('Locked was tapped')).toBeHidden();
  await expect(screen.getByTestId('period-Week')).toBeSelected();
  await expect(screen.getByTestId('period-Day')).not.toBeSelected();
  // The period right of Week: the nearest.
  const week = await screen.getByTestId('period-Week').boundingBox();
  const right = (await boxes(screen.getByTestId(/^period-/))).filter((c) => c.b.x >= week.x + week.width).sort((a, b) => a.b.x - b.b.x)[0];
  await right.l.tap();
  await expect(screen.getByTestId('period-Month')).toBeSelected();
  await expect(screen.getByText('Showing: Month')).toBeVisible();
  await expect(screen.getByTestId('notify-me')).toBeChecked();
  await expect(screen.getByTestId('hide-me')).not.toBeChecked();
  await screen.getByTestId('hide-me').tap();
  await expect(screen.getByTestId('hide-me')).toBeChecked();
  const focus = screen.getByTestId('focus-field');
  await screen.scrollUntilVisible(focus, { direction: 'up' });
  await focus.tap();
  await expect(focus).toBeFocused();
  await device.dismissKeyboard();
  const slow = screen.getByTestId('slow-button');
  await screen.scrollUntilVisible(slow, { direction: 'down' });
  await slow.tap();
  await expect(screen.getByText('Slow button: done')).toBeVisible({ timeout: 5_000 });
  // An optional tap on what is not there.
  const absent = screen.getByText('Not on this screen');
  if (await absent.isVisible()) await absent.tap();
  // The step's label ("The square box shows") names it in Maestro's report: no selector.
  await expect(screen.getByTestId('box-square')).toBeVisible();
});
