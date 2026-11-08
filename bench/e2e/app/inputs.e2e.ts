import { test } from '@e2e-dev/mobile';
import { expect } from 'e2e';
import { guest, lab } from './shared.ts';

const pick = (a) => a[Math.floor(Math.random() * a.length)];
const letters = (n) => Array.from({ length: n }, () => 'abcdefghijklmnopqrstuvwxyz'[Math.floor(Math.random() * 26)]).join('');

test('inputs', async ({ app, device, screen }) => {
  const ctx = { app, device, screen };
  await guest(ctx);
  await lab(screen, 'inputs');
  await expect(screen.getByText('Keyboard: hidden')).toBeVisible();
  await screen.getByTestId('note-input').fill('Buy milk');
  await device.dismissKeyboard();
  await expect(screen.getByText('Keyboard: hidden')).toBeVisible();
  // The note survives a stop, and a kill.
  for (let i = 0; i < 2; i++) {
    await device.closeApp();
    await app.open();
    await lab(screen, 'inputs');
    await expect(screen.getByText('Buy milk')).toBeVisible();
  }
  const key = screen.getByTestId('key-input');
  await key.fill('hello');
  await key.press('Enter');
  await expect(screen.getByText('Submitted: hello')).toBeVisible();
  await key.tap();
  await key.press('Backspace');
  await key.press('Backspace');
  await expect(screen.getByText('Backspaces: 2')).toBeVisible();
  await key.press('Backspace');
  await key.press('Enter');
  await expect(screen.getByText('Submitted: he')).toBeVisible();
  // No copy and paste: the text read, then typed.
  await screen.getByTestId('paste-input').fill(await screen.getByTestId('copy-source').textContent());
  await expect(screen.getByText('Pasted the code')).toBeVisible();
  await device.dismissKeyboard();
  // No random inputs: values of each kind, made here.
  const values = {
    'rand-text': letters(8),
    'rand-number': String(Math.floor(10000 + Math.random() * 90000)),
    'rand-email': `${letters(6)}@example.com`,
    'rand-name': pick(['Ava Martin', 'Noah Tremblay', 'Mia Roy']),
    'rand-city': pick(['Laval', 'Ottawa', 'Toronto']),
    'rand-country': pick(['Canada', 'France', 'Japan']),
    'rand-color': pick(['teal', 'maroon', 'olive']),
  };
  for (const [id, value] of Object.entries(values)) {
    const field = screen.getByTestId(id);
    await screen.scrollUntilVisible(field, { direction: 'down' });
    await field.fill(value);
    await device.dismissKeyboard();
  }
  await screen.scrollUntilVisible(screen.getByTestId('rand-check'), { direction: 'down' });
  await expect(screen.getByText(/^text \d+ chars · number digits · email valid · name \d+ words · city set · country set · color set$/)).toBeVisible();
  const catchBack = screen.getByTestId('catch-back');
  await screen.scrollUntilVisible(catchBack, { direction: 'down' });
  await catchBack.tap();
  await device.back();
  await expect(screen.getByText('Back presses: 1')).toBeVisible();
  await catchBack.tap();
  await device.back();
  await expect(screen.getByTestId('back-presses')).toBeHidden();
  await app.clearState();
  await expect(screen.getByText('Discover')).toBeVisible();
});
