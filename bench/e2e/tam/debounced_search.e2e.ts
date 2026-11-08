import { test } from '@e2e-dev/mobile';
import { expect } from 'e2e';
import { openScenario } from './open.ts';

test('Debounced Search', async ({ app, device, screen }) => {
  await openScenario({ app, screen }, 'Debounced Search');
  await screen.getByTestId('search-input').fill('benchmark target');
  await screen.getByTestId('search-input').press('Enter');
  await screen.getByTestId('result-Benchmark Target').tap();
  await expect(screen.getByTestId('success-message')).toHaveText('Selected Benchmark Target');
});
