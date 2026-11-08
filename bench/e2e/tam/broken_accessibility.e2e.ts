import { test } from '@e2e-dev/mobile';
import { expect } from 'e2e';
import { openScenario } from './open.ts';

test('Broken Accessibility', async ({ app, device, screen }) => {
  await openScenario({ app, screen }, 'Broken Accessibility');
  // The shapes carry no label: the green circle by where it is on screen.
  await screen.tapAt({ x: 634, y: 1391 });
  await screen.getByRole('textbox').fill('7291');
  await screen.getByText('→').tap();
  await expect(screen.getByText('Access granted')).toBeVisible();
});
