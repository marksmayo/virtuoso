import type { Device } from '@e2e-dev/mobile';
import type { App, Locator, Screen } from 'e2e';
import { expect } from 'e2e';
import { execFileSync } from 'node:child_process';

/**
 * The Speedrun Bench app's shared steps (github.com/hubastard/speedrun, bench/app/flows/_shared),
 * one function each. The harness clears the app before every run; a test that clears it again
 * mid-way says so.
 */
export type Ctx = { app: App; device: Device; screen: Screen };

/** Maestro's `runFlow: when: visible`: waits a while (its optional lookup, 7 s) for it, then goes on. */
export async function shows(locator: Locator, timeout = 7_000): Promise<boolean> {
  return locator.waitFor({ state: 'visible', timeout }).then(
    () => true,
    () => false,
  );
}

/**
 * A fresh start as a guest: onboarding skipped, no sign in. Cleared then opened, as launchApp
 * clearState does: the engine opens the app as it prepares, which spends the first launch's
 * onboarding.
 */
export async function guest({ app, screen }: Ctx): Promise<void> {
  await app.clearState();
  await screen.getByTestId('onboarding-skip').tap();
  await screen.getByTestId('guest').tap();
  await expect(screen.getByTestId('tab-home')).toBeVisible({ timeout: 10_000 });
}

/** Opens a lab screen from the Lab tab. */
export async function lab(screen: Screen, name: string): Promise<void> {
  await screen.getByTestId('tab-lab').tap();
  await screen.getByTestId(`lab-${name}`).tap();
}

/** One animation scene: open it, tap its button three times, check the count, go back. */
export async function scene(screen: Screen, name: string): Promise<void> {
  const row = screen.getByTestId(`scene-${name}`);
  await screen.scrollUntilVisible(row, { direction: 'down' });
  await row.tap();
  for (let i = 0; i < 3; i++) await screen.getByTestId('lab-tap').tap();
  await expect(screen.getByText('Taps: 3')).toBeVisible();
  await screen.getByTestId('header-back').tap();
}

/**
 * scrollUntilVisible, again where one gives up: a long way (thirty posts) takes more swipes than
 * one allows, and a list that loads its next page at the end has nothing new for a moment.
 */
export async function scrollTo(screen: Screen, target: Locator, direction: 'up' | 'down', attempts = 3): Promise<void> {
  for (let attempt = 1; ; attempt++) {
    try {
      return await screen.scrollUntilVisible(target, { direction, timeout: 40_000 });
    } catch (e) {
      if (attempt >= attempts) throw e;
    }
  }
}

/** "Not now" to the notifications prompt, when the app asks. */
export async function dismissNotify(screen: Screen): Promise<void> {
  if (await shows(screen.getByTestId('notify-title'))) await screen.getByTestId('notify-later').tap();
}

/** Signs in from a fresh launch with no saved state (the onboarding first). */
export async function signIn({ app, screen }: Ctx, phone: string, code: string, remember: boolean): Promise<void> {
  await app.clearState();
  await screen.getByTestId('onboarding-skip').tap();
  // Typed key by key: the field formats the number as it comes, and the code field submits
  // itself at the sixth digit, so a fill's read-back of what it set can find something else.
  await screen.getByTestId('phone-input').tap();
  await screen.getByTestId('phone-input').pressSequentially(phone);
  if (remember) await screen.getByTestId('remember-me').tap();
  await screen.getByTestId('next').tap();
  const codeInput = screen.getByTestId('code-input');
  await expect(codeInput).toBeVisible({ timeout: 10_000 });
  await codeInput.tap();
  await codeInput.pressSequentially(code);
  // Home, or the notification prompt over it: either means the sign in is done.
  await expect(screen.getByTestId('tab-home').or(screen.getByTestId('notify-title')).first()).toBeVisible({ timeout: 15_000 });
  await dismissNotify(screen);
  await expect(screen.getByText(/^Hi, /)).toBeVisible({ timeout: 10_000 });
}

/** From the Shop tab: opens a product, picks a size, adds it, comes back. */
export async function addToCart(screen: Screen, product: string, size: string): Promise<void> {
  await screen.getByTestId('tab-shop').tap();
  const card = screen.getByTestId(`product-${product}`);
  await screen.scrollUntilVisible(card, { direction: 'down' });
  await card.tap();
  await screen.getByTestId(`size-${size}`).tap();
  await screen.getByTestId('add-to-cart').tap();
  await expect(screen.getByText('Added to cart')).toBeVisible();
  await screen.getByTestId('header-back').tap();
}

/** From the cart: checkout with a valid address, then the confirmation. */
export async function checkout({ device, screen }: Ctx, order: string): Promise<void> {
  await screen.getByTestId('checkout').tap();
  await screen.getByTestId('name-input').fill('Alex Rivera');
  await screen.getByTestId('email-input').fill('alex@example.com');
  await screen.getByTestId('street-input').fill('1 Main Street');
  await screen.getByTestId('city-input').fill('Montreal');
  await screen.getByTestId('postal-input').fill('H2X 1Y4');
  // The keyboard covers the sticky footer.
  await device.dismissKeyboard();
  await screen.getByTestId('place-order').tap();
  await expect(screen.getByText(`Order #${order}`)).toBeVisible({ timeout: 10_000 });
}

/** The rating, badge and survey popups that may follow an order, each a while after the last. */
export async function dismissOrderPopups(screen: Screen): Promise<void> {
  if (await shows(screen.getByTestId('rate-title'))) await screen.getByTestId('rate-later').tap();
  if (await shows(screen.getByTestId('badge-title'))) await screen.getByTestId('badge-close').tap();
  if (await shows(screen.getByTestId('survey-title'))) {
    await screen.getByTestId('nps-9').tap();
    await expect(screen.getByText('Your score: 9')).toBeVisible();
    await screen.getByTestId('nps-submit').tap();
  }
}

/** Runs adb on the benchmark's device, for what the engine has no call for. */
export function adb(...args: string[]): string {
  const serial = process.env.VIRTUOSO_BENCH_DEVICE;
  return execFileSync('adb', serial ? ['-s', serial, ...args] : args, { encoding: 'utf8' });
}
