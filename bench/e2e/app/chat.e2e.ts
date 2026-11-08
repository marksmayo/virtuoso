import { test } from '@e2e-dev/mobile';
import { expect } from 'e2e';
import { guest } from './shared.ts';

test('chat', async ({ app, device, screen }) => {
  const ctx = { app, device, screen };
  await guest(ctx);
  await screen.getByTestId('tab-chats').tap();
  await screen.getByTestId('chat-bot').tap();
  await expect(screen.getByText('Hi! Ask me anything.')).toBeVisible();
  await expect(screen.getByTestId('send')).toBeDisabled();
  await screen.getByTestId('message-input').fill('Hello there');
  await screen.getByTestId('send').tap();
  await expect(screen.getByText('You said: Hello there')).toBeVisible({ timeout: 10_000 });
  await screen.getByTestId('message-1').doubleTap();
  await expect(screen.getByText(/❤️ Loved$/)).toBeVisible();
  await screen.getByTestId('message-2').longPress();
  await screen.getByTestId('menu-copy').tap();
  await expect(screen.getByText('Copied')).toBeVisible();
  await screen.getByTestId('message-2').longPress();
  await expect(screen.getByTestId('menu-delete')).toBeVisible();
  // Outside the menu, near the top of the screen (1080 x 2400): it closes.
  await screen.tapAt({ x: 540, y: 240 });
  await expect(screen.getByTestId('menu-delete')).toBeHidden();
  await screen.getByTestId('message-2').longPress();
  await screen.getByTestId('menu-delete').tap();
  await expect(screen.getByText('You said: Hello there')).toBeHidden();
  // No clipboard on Android ("clipboard is not supported on this device"): the text, typed.
  await screen.getByTestId('message-input').fill('Pasted hello');
  await screen.getByTestId('send').tap();
  await expect(screen.getByText('You said: Pasted hello')).toBeVisible({ timeout: 10_000 });
  await screen.getByTestId('quick-Track my order').tap();
  await expect(screen.getByText('You said: Track my order')).toBeVisible({ timeout: 10_000 });
});
