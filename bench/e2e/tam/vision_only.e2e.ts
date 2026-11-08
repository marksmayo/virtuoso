import { test } from '@e2e-dev/mobile';
import { expect } from 'e2e';
import { openScenario } from './open.ts';

test('Vision Only', async ({ app, device, screen }) => {
  await openScenario({ app, screen }, 'Vision Only');
  // The shapes have no labels: square, circle and diamond by where they are.
  await screen.tapAt({ x: 750, y: 1458 });
  await screen.tapAt({ x: 330, y: 1458 });
  await screen.tapAt({ x: 540, y: 1458 });
  await expect(screen.getByText('Sequence complete')).toBeVisible();
});
