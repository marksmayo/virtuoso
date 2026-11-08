import { test } from '@e2e-dev/mobile';
import { expect } from 'e2e';
import { guest, lab } from './shared.ts';

test('drag', async ({ app, device, screen }) => {
  const ctx = { app, device, screen };
  await guest(ctx);
  await lab(screen, 'drag');
  // From the handle across the track: the handle sits at the screen's left edge, where a
  // swipe rightwards is the system's back gesture.
  const handle = await screen.getByTestId('slider-handle').boundingBox();
  const track = await screen.getByTestId('slider-track').boundingBox();
  const y = handle.y + handle.height / 2;
  await screen.swipe({ from: { x: handle.x + handle.width / 2, y }, to: { x: track.x + track.width * 0.95, y }, duration: 400 });
  // A swipe's direction is the content's: the finger moves the other way.
  await expect(screen.getByText(/^Volume: (7\d|8\d|9\d|100)$/)).toBeVisible();
  // The card dragged down into the zone below it.
  const card = await screen.getByTestId('drag-card').boundingBox();
  const cx = card.x + card.width / 2;
  await screen.swipe({ from: { x: cx, y: card.y + card.height / 2 }, to: { x: cx, y: card.y + card.height / 2 + 800 }, duration: 600 });
  await expect(screen.getByText('Dropped in the zone')).toBeVisible();
  // The card list is above the screen's center.
  // scrollUntilVisible drags the screen's middle, below the cards: along their row instead.
  const card25 = screen.getByText('Card 25', { exact: true });
  const row = await screen.getByText('Card 1', { exact: true }).boundingBox();
  const ry = row.y + row.height / 2;
  for (let i = 0; i < 20 && !(await card25.isVisible()); i++) await screen.swipe({ from: { x: 860, y: ry }, to: { x: 220, y: ry }, duration: 300 });
  await expect(card25).toBeVisible();
  await screen.getByTestId('open-long-list').tap();
  const row120 = screen.getByText('Row 120', { exact: true });
  await screen.scrollUntilVisible(row120, { direction: 'down', timeout: 60_000 });
  await row120.tap();
  await expect(screen.getByText('Picked row 120')).toBeVisible();
  const row3 = screen.getByText('Row 3', { exact: true });
  await screen.scrollUntilVisible(row3, { direction: 'up', timeout: 60_000 });
  await row3.tap();
  await expect(screen.getByText('Picked row 3')).toBeVisible();
});
