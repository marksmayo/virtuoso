import { test } from '@e2e-dev/mobile';
import { expect } from 'e2e';
import { guest, lab } from './shared.ts';

test('scripting', async ({ app, device, screen }) => {
  const ctx = { app, device, screen };
  // The flow's scripting, as test code: its env, an output object, a script's values back.
  const GREETING: string = 'Hello';
  const output: Record<string, any> = { started: true };
  await guest(ctx);
  expect(output.started === true).toBeTruthy();
  const plan = (PLAN) => ({ plan: PLAN, greeting: GREETING + ', ' + PLAN.toLowerCase() });
  Object.assign(output, plan('Pro'));
  console.log('picked ' + output.plan);
  expect(output.plan === 'Pro').toBeTruthy();
  expect(output.greeting === 'Hello, pro').toBeTruthy();
  await lab(screen, 'selectors');
  let taps = 0;
  for (let i = 0; i < 3; i++) {
    await screen.getByText('Item').nth(taps).tap();
    taps++;
  }
  await expect(screen.getByText('Picked item 3')).toBeVisible();
  expect(taps === 3).toBeTruthy();
  while (!(await screen.getByText(`Chose ${output.plan}`).isVisible())) {
    await screen.getByTestId(`plan-${output.plan.toLowerCase()}`).getByText('Choose').tap();
  }
  if (GREETING === 'Hello') await screen.getByTestId('open-button').tap();
  await expect(screen.getByText('Open was tapped')).toBeVisible();
  if (GREETING === 'Bye') await screen.getByTestId('hide-me').tap();
  await expect(screen.getByTestId('hide-me')).not.toBeChecked();
  const COUNT: string = '7';
  expect(COUNT === '7').toBeTruthy();
  let tries = 0;
  for (let attempt = 0; attempt < 3; attempt++) {
    tries++;
    if (tries >= 2) break;
  }
  expect(tries === 2).toBeTruthy();
  await expect(screen.getByText('Nothing like this')).toBeHidden({ timeout: 1_000 });
  Object.assign(output, { finished: true });
});
