import { test } from '@e2e-dev/mobile';
import { expect } from 'e2e';
import { signIn } from './shared.ts';

test('remember me', async ({ app, device, screen }) => {
  const ctx = { app, device, screen };
  await signIn(ctx, '5550109999', '123456', true);
  // Android keeps no account across a cleared state ("Welcome back" is iOS's keychain).
  await app.clearState();
  await screen.getByTestId('onboarding-skip').tap();
  await expect(screen.getByText("Let's start with your phone number")).toBeVisible();
  await app.clearState();
  await screen.getByTestId('onboarding-skip').tap();
  await expect(screen.getByText("Let's start with your phone number")).toBeVisible();
});
