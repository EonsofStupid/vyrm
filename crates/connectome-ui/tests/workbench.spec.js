import { test, expect } from '@playwright/test';

const baseURL = process.env.CONNECTOME_URL || 'http://127.0.0.1:4387';

test('developer lenses remain navigable and inspectable', async ({ page }) => {
  await page.goto(baseURL);
  await expect(page.getByRole('heading', { name: 'Runtime overview' })).toBeVisible();
  await expect(page.locator('.metric-card')).toHaveCount(4);

  await page.getByRole('button', { name: /Claims/ }).click();
  await expect(page).toHaveURL(/#claims$/);
  await expect(page.getByRole('heading', { name: 'Current claims' })).toBeVisible();
  await page.locator('tbody tr').first().click();
  await expect(page.locator('#inspector-body')).toContainText('Digest');

  await page.getByRole('button', { name: /Graph/ }).click();
  await expect(page).toHaveURL(/#graph$/);
  await expect(page.locator('#graph .graph-node')).not.toHaveCount(0);
});

test('source routing and read-only transport are enforced', async ({ page, request }) => {
  await page.goto(`${baseURL}/#routes`);
  await page.locator('#route-query').fill('connectome_runtime');
  await page.getByRole('button', { name: 'Route source' }).click();
  await expect(page.locator('.route-result')).toHaveCount(1);
  await expect(page.locator('.route-result')).toContainText('lib.rs');

  const response = await request.post(`${baseURL}/api/snapshot`, { data: {} });
  expect(response.status()).toBe(405);
  expect((await response.json()).error).toContain('read-only');
});

test('prompt flights can be launched, frozen, expanded, and compared', async ({ page }) => {
  await page.goto(`${baseURL}/#flight`);
  await expect(page.getByRole('heading', { name: 'Reasoning flight lab' })).toBeVisible();
  await page.locator('#flight-prompt').fill('Inspect runtime routing and verify the observed file path.');
  await page.locator('#flight-context').selectOption('fresh');
  await page.locator('#flight-provider').selectOption('observe');
  await page.locator('[data-reasoning-profile="high"]').click();
  const launch = page.locator('.launch-button');
  await launch.click();
  await expect(launch).toBeEnabled({ timeout: 15_000 });

  await expect(page.locator('.flight-visual')).toBeVisible();
  await expect(page.locator('.flight-switcher')).toContainText('CONFIGURED high');
  await expect(page.locator('.telemetry-lane')).toHaveCount(4);
  await expect(page.locator('.telemetry-packet')).not.toHaveCount(0);
  await expect(page.locator('.film-frame')).not.toHaveCount(0);
  await page.locator('.film-frame').nth(1).click();
  await expect(page.locator('.micro-event')).toContainText('Baseline context purged');
  await page.getByRole('button', { name: 'Inspect raw event' }).click();
  await expect(page.locator('#inspector-body')).toContainText('prompt flight event');
});

test('reasoning profiles expose exact effort and bidirectional event playback', async ({ page }) => {
  await page.goto(`${baseURL}/#flight`);
  await page.locator('[data-prompt-preset="strong"]').click();
  await page.locator('[data-reasoning-profile="extreme"]').click();
  await page.locator('#flight-context').selectOption('fresh');
  await page.locator('#flight-provider').selectOption('observe');
  const launch = page.locator('.launch-button');
  await launch.click();
  await expect(launch).toBeEnabled({ timeout: 15_000 });

  await expect(page.locator('.flight-switcher')).toContainText('Extreme');
  await expect(page.locator('.flight-switcher')).toContainText('xhigh');
  await expect(page.locator('.visual-readout')).toContainText('EVENT MASS');
  await expect(page.locator('.telemetry-river')).toBeVisible();

  await page.getByTitle('Latest event').click();
  await expect(page.locator('.micro-event')).toBeVisible();
  await page.getByTitle('Rewind through time').click();
  await expect(page.getByTitle('Play or freeze')).toContainText('Freeze time');
  await page.locator('.telemetry-packet').first().click();
  await expect(page.getByTitle('Play or freeze')).toContainText('Resume time');
  await expect(page.locator('.event-data-strip')).toContainText('signal volume');
  await expect(page.locator('.payload-breakdown')).toBeVisible();
  await page.locator('.event-envelope').click();
  await expect(page.locator('.event-envelope pre')).toBeVisible();
});

test('persisted schema is readable as a developer contract', async ({ page, request }) => {
  const response = await request.post(`${baseURL}/api/flights`, {
    data: {
      prompt: 'Install and inspect the governed prompt-flight types.',
      provider: 'observe',
      context_mode: 'fresh',
      budget: 512,
      acceptance_marker: '',
      reasoning_profile: 'default',
    },
  });
  expect(response.ok()).toBeTruthy();

  const schemaResponse = await request.get(`${baseURL}/api/runtime/schema`);
  expect(schemaResponse.ok()).toBeTruthy();
  const schema = await schemaResponse.json();
  expect(schema.revision).toBeGreaterThan(0);
  expect(schema.records.prompt_flight.properties.status.required).toBeTruthy();

  await page.goto(`${baseURL}/#schema`);
  await expect(page.getByRole('heading', { name: 'Runtime schema' })).toBeVisible();
  await expect(page.locator('.schema-revision')).toContainText('prompt flight');
  await expect(page.locator('.schema-groups')).toContainText('prompt flight');
  await expect(page.locator('.schema-groups')).toContainText('required');
  await expect(page.locator('.schema-contract-line')).toContainText('Deny or commit');
});

test('custom prompt editing is stable while live snapshots and flight polling continue', async ({ page }) => {
  await page.goto(`${baseURL}/#flight`);
  const prompt = 'Compare the runtime graph and report verified evidence with a stop condition.';
  await page.locator('#flight-prompt').fill(prompt);
  await expect(page.locator('#prompt-contract')).toContainText('explicit signals');
  await page.waitForTimeout(5500);
  await expect(page.locator('#flight-prompt')).toHaveValue(prompt);
  await expect(page.locator('.contract-signals .present')).not.toHaveCount(0);
  await page.locator('#flight-prompt').pressSequentially(' scope');
  await expect(page).toHaveURL(/#flight$/);
});
