import { test, expect } from '@playwright/test';

const baseURL = process.env.CONNECTOME_URL || 'http://127.0.0.1:4387';

test('renders the canonical stamped RRD snapshot', async ({ page, request }) => {
  const response = await request.get(`${baseURL}/api/snapshot`);
  expect(response.ok()).toBeTruthy();
  const snapshot = await response.json();
  expect(snapshot.format_version).toBe(1);
  expect(snapshot.read.assembly_attempts).toBeGreaterThan(0);
  expect(snapshot.read.runtime_cursor).toBe(snapshot.readiness.runtime_cursor);
  expect(snapshot.product_capabilities.capabilities.length).toBeGreaterThan(0);

  await page.goto(baseURL);
  await expect(page.getByRole('heading', { name: 'One engine, one read stamp' })).toBeVisible();
  await expect(page.locator('#status')).toHaveText('RRD connected');
  await expect(page.locator('.metric')).toHaveCount(4);
  await expect(page.locator('#stamp')).toContainText(`cursor ${snapshot.read.runtime_cursor}`);
});

test('models graph activity and capabilities render from one contract', async ({ page }) => {
  await page.goto(baseURL);
  await expect(page.locator('#status')).toHaveText('RRD connected');

  await page.getByRole('button', { name: /Models/ }).click();
  await expect(page.getByRole('heading', { name: 'Logical models' })).toBeVisible();

  await page.getByRole('button', { name: /Graph/ }).click();
  await expect(page.getByRole('heading', { name: 'Temporal graph' })).toBeVisible();

  await page.getByRole('button', { name: /Activity/ }).click();
  await expect(page.getByRole('heading', { name: 'Committed activity' })).toBeVisible();

  await page.getByRole('button', { name: /Capabilities/ }).click();
  await expect(page.getByRole('heading', { name: 'Surface dispositions' })).toBeVisible();
  await expect(page.locator('.cap')).not.toHaveCount(0);
});

test('query execution crosses the public scope-bound API', async ({ page }) => {
  await page.goto(baseURL);
  await expect(page.locator('#status')).toHaveText('RRD connected');
  await page.getByRole('button', { name: /Query/ }).click();
  await page.locator('#query-source').fill('FROM record:document KNOWN HEAD PROJECT id EXPLAIN CONTRACT');
  await page.getByRole('button', { name: 'Run query' }).click();
  await expect(page.locator('#query-result pre')).toBeVisible();
  await expect(page.locator('#query-result')).toContainText('known_at_cursor');
});

test('legacy ungoverned mutations fail explicitly', async ({ request }) => {
  for (const endpoint of ['/api/flights', '/api/demos/prompt-strength', '/api/cluster/samples']) {
    const response = await request.post(`${baseURL}${endpoint}`, { data: {} });
    expect(response.status()).toBe(501);
    expect((await response.json()).required_action).toContain('rrd-contract/rrd-engine');
  }
  const wrongMethod = await request.post(`${baseURL}/api/snapshot`, { data: {} });
  expect(wrongMethod.status()).toBe(405);
});
