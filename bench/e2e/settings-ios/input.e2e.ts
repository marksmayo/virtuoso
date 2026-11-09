import { test } from '@e2e-dev/mobile';

// Text input: type into the search field, erase, type again.
test('input', async ({ app, screen }) => {
  await app.open();
  // iOS 26 puts Settings' search field at the bottom of the screen.
  const field = screen.getByRole('searchbox').or(screen.getByRole('textbox')).first();
  await field.tap();
  await field.fill('brightness level');
  await field.clear();
  await field.fill('dark mode');
  // The keyboard down: iOS's has no dismiss key and stays up through a drag, so its Search key.
  await field.press('Enter');
});
