const { test, expect } = require('@playwright/test');

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
  await expect(page.getByRole('heading', { name: 'Prompt flight recorder' })).toBeVisible();
  await page.locator('#flight-prompt-a').fill('inspect runtime from a deliberately vague prompt');
  await page.locator('#flight-prompt-b').fill('Inspect runtime routing and verify the observed file path.');
  await page.locator('#flight-context').selectOption('fresh');
  await page.locator('#flight-provider').selectOption('observe');
  await page.getByRole('button', { name: 'Run both prompts' }).click();

  await expect(page.locator('.flight-visual')).toBeVisible();
  await expect(page.locator('.comparison-arm')).toHaveCount(2);
  await expect(page.locator('.film-frame')).not.toHaveCount(0);
  await page.locator('.film-frame').nth(1).click();
  await expect(page.locator('.micro-event')).toContainText('Baseline context purged');
  await page.getByRole('button', { name: 'Inspect raw event' }).click();
  await expect(page.locator('#inspector-body')).toContainText('prompt flight event');
});

test('weak and strong demos expose temporal bursts and bidirectional playback', async ({ page }) => {
  await page.goto(`${baseURL}/#flight`);
  await page.getByRole('button', { name: 'Restore guided pair' }).click();
  await page.getByRole('button', { name: 'Run both prompts' }).click();

  await expect(page.locator('.comparison-arm')).toHaveCount(2);
  await expect(page.locator('.comparison-arm.role-weak')).toContainText('Make this better.');
  await expect(page.locator('.comparison-arm.role-strong')).toContainText('Trace one prompt');
  await expect(page.locator('.comparison-verdict')).toContainText('less context');
  await expect(page.locator('.burst-column')).toHaveCount(8);

  await page.getByTitle('Latest event').click();
  await expect(page.locator('.micro-event')).toContainText('Strong prompt produced');
  await page.getByTitle('Rewind through time').click();
  await expect(page.getByTitle('Play or freeze')).toContainText('Freeze time');
  await page.locator('.burst-column').nth(2).click();
  await expect(page.getByTitle('Play or freeze')).toContainText('Resume time');
  await expect(page.locator('.event-data-strip')).toContainText('signal volume');
  await expect(page.locator('.payload-breakdown')).toBeVisible();
});

test('custom prompt editing is stable while live snapshots and flight polling continue', async ({ page }) => {
  await page.goto(`${baseURL}/#flight`);
  const prompt = 'Compare the runtime graph and report verified evidence with a stop condition.';
  await page.locator('#flight-prompt-a').fill(prompt);
  await expect(page.locator('#prompt-contract-a')).toContainText('explicit signals');
  await page.waitForTimeout(5500);
  await expect(page.locator('#flight-prompt-a')).toHaveValue(prompt);
  await expect(page.locator('.contract-signals .present')).not.toHaveCount(0);
});
