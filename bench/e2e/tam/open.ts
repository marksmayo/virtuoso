import type { App, Screen } from 'e2e';
import { expect } from 'e2e';

/**
 * Opens a scenario from the benchmark app's home list, started fresh: a row's label is its
 * name, a full stop and its description; the scenario is up once its row has left the tree
 * (adapted from tester-army/e2e's apps/mobile-benchmark tests/fixtures.ts).
 */
export async function openScenario({ app, screen }: { app: App; screen: Screen }, name: string): Promise<void> {
  await app.open();
  const row = screen.getByLabel(`${name}. `, { exact: false });
  await row.tap();
  await expect(row).toBeHidden({ timeout: 10_000 });
}
