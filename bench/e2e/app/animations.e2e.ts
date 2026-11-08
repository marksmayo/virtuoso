import { test } from '@e2e-dev/mobile';
import { expect } from 'e2e';
import { guest, scene } from './shared.ts';

test('animations', async ({ app, device, screen }) => {
  const ctx = { app, device, screen };
  await guest(ctx);
  await screen.getByTestId('tab-lab').tap();
  await screen.getByTestId('lab-animations').tap();
  for (const s of ['still', 'reanimated', 'native-driver', 'js-driver', 'lottie', 'spinner', 'ticker', 'counter', 'marquee', 'shimmer', 'carousel', 'progress']) {
    await scene(screen, s);
  }
});
