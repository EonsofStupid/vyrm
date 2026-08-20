import { test, expect } from '@playwright/test';

const baseURL = process.env.CONNECTOME_URL || 'http://127.0.0.1:4387';

function clusterStatus(project, observedAt, denied = 0) {
  const metrics = (attempted = 0, allowed = 0, deniedCount = 0) => ({
    attempted, allowed, denied: deniedCount, failed: 0, current_in_flight: 0,
    peak_in_flight: attempted ? 1 : 0, request_bytes: attempted * 96,
    response_bytes: attempted * 32, total_duration_micros: attempted * 250,
    max_duration_micros: attempted ? 250 : 0,
  });
  const operations = {
    append: metrics(), snapshot: metrics(), vote: metrics(), artifact: metrics(),
    runtime_commit: metrics(denied, 0, denied),
  };
  return {
    project_scope: project,
    cluster: 'cluster:workbench',
    shard: 3,
    raft_node_id: 1,
    canonical_node_id: 'node:workbench-one',
    current_term: 4,
    current_leader: 1,
    last_log_index: 22,
    last_applied_index: 22,
    snapshot_index: 20,
    purged_index: 20,
    state: 'leader',
    credentials: { generation: 1, leaf_digest: 'ab'.repeat(32) },
    telemetry: {
      observed_at: observedAt,
      transport_ingress: {
        contract_version: 1,
        started_at: 100,
        observed_at: observedAt,
        policy: {
          max_global_in_flight: 256,
          max_identity_in_flight: 64,
          max_identity_requests_per_window: 4096,
          window_millis: 1000,
          max_tracked_identities: 1024,
        },
        operations,
        identities: {},
        current_in_flight: 0,
        peak_in_flight: denied ? 1 : 0,
        accepted_connections: denied,
        denied_connections: 0,
        connection_request_bytes: denied * 96,
        overflowed: false,
      },
      artifacts: {
        contract_version: 1,
        started_at: 100,
        observed_at: observedAt,
        policy: {
          max_active_sessions: 64,
          max_reserved_bytes: 68719476736,
          stale_incomplete_after_millis: 86400000,
          completed_receipt_retention_millis: 604800000,
          max_retained_receipts: 4096,
        },
        inventory: { active_sessions: 0, reserved_bytes: 0, partial_bytes: 0, retained_receipts: 0 },
        begin_requests: 0, chunk_requests: 0, complete_requests: 0,
        begin_responses: 0, accepted_chunks: 0, completed_responses: 0,
        completed_receipt_replays: 0, denied: 0, failed: 0, quota_denials: 0,
        gc_runs: 0, gc_removed_incomplete: 0, gc_removed_completed: 0,
        gc_reclaimed_partial_bytes: 0, overflowed: false,
      },
      consensus_traces: {
        started_at: 100,
        observed_at: observedAt,
        prepared_observations: 0, chunk_observations: 0, completed_observations: 0,
        failed_observations: 0, commit_acknowledgements: 0, cursor_conflicts: 0,
        leader_changes: 0, leader_unavailable: 0, denied: 0, failed: 0, overflowed: false,
      },
    },
  };
}

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

test('global temporal evidence can be frozen rewound and inspected', async ({ page, request }) => {
  const seeded = await request.post(`${baseURL}/api/flights`, {
    data: {
      prompt: 'Persist a flight so its runtime mutations enter the global evidence stream.',
      provider: 'observe',
      context_mode: 'fresh',
      budget: 512,
      acceptance_marker: '',
      reasoning_profile: 'default',
    },
  });
  expect(seeded.ok()).toBeTruthy();

  await page.goto(`${baseURL}/#stream`);
  await expect(page.getByRole('heading', { name: 'Temporal evidence stream' })).toBeVisible();
  await expect(page.locator('.stream-lane')).toHaveCount(6);
  await expect(page.locator('.stream-packet')).not.toHaveCount(0);
  await page.getByTitle('First mutation').click();
  await expect(page.locator('.stream-event-detail')).toContainText('cursor');
  await page.getByTitle('Fast-forward mutations').click();
  await expect(page.getByTitle('Play or freeze stream')).toContainText('Freeze time');
  await page.locator('.stream-packet').last().click();
  await expect(page.getByTitle('Play or freeze stream')).toContainText('Resume time');
  await page.getByRole('button', { name: 'Inspect mutation + audit' }).click();
  await expect(page.locator('#inspector-body')).toContainText('runtime mutation');
  await expect(page.locator('#inspector-body')).toContainText('Audit digest');
});

test('retained cluster observations can be frozen rewound and inspected', async ({ page, request }) => {
  const snapshot = await (await request.get(`${baseURL}/api/snapshot`)).json();
  const first = await request.post(`${baseURL}/api/cluster/samples`, {
    data: { status: clusterStatus(snapshot.instance.id, 110, 0) },
  });
  expect(first.status()).toBe(201);
  const second = await request.post(`${baseURL}/api/cluster/samples`, {
    data: { status: clusterStatus(snapshot.instance.id, 120, 1) },
  });
  expect(second.status()).toBe(201);

  await page.goto(`${baseURL}/#cluster`);
  await expect(page.getByRole('heading', { name: 'Cluster flight recorder' })).toBeVisible();
  await expect(page.locator('.cluster-node')).toHaveCount(1);
  await expect(page.locator('.cluster-frame')).toHaveCount(2);
  await expect(page.locator('.cluster-alert')).toContainText('transport denied');
  await page.locator('.cluster-frame').first().click();
  await expect(page.getByTitle('Play or freeze cluster history')).toContainText('Resume time');
  await expect(page.locator('.cluster-clear')).toContainText('No derived alert');
  await page.getByTitle('Latest cluster sample').click();
  await page.getByRole('button', { name: 'Inspect raw status + audit' }).click();
  await expect(page.locator('#inspector-body')).toContainText('retained cluster observation');
  await expect(page.locator('#inspector-body')).toContainText('Audit digest');
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

  const retentionResponse = await request.get(`${baseURL}/api/runtime/retention`);
  expect(retentionResponse.ok()).toBeTruthy();
  const retention = await retentionResponse.json();
  expect(Array.isArray(retention.snapshots)).toBeTruthy();
  expect(Array.isArray(retention.pins)).toBeTruthy();

  await page.goto(`${baseURL}/#schema`);
  await expect(page.getByRole('heading', { name: 'Runtime schema' })).toBeVisible();
  await expect(page.locator('.schema-revision')).toContainText('prompt flight');
  await expect(page.locator('.schema-groups')).toContainText('prompt flight');
  await expect(page.locator('.schema-groups')).toContainText('required');
  await expect(page.locator('.schema-contract-line')).toContainText('Deny or commit');
});

test('query lab exposes exact plan evidence and deterministic rows', async ({ page, request }) => {
  const seed = await request.post(`${baseURL}/api/flights`, {
    data: {
      prompt: 'Create a typed runtime row for query inspection.',
      provider: 'observe',
      context_mode: 'fresh',
      budget: 512,
      acceptance_marker: '',
      reasoning_profile: 'default',
    },
  });
  expect(seed.ok()).toBeTruthy();

  await page.goto(`${baseURL}/#query`);
  await expect(page.getByRole('heading', { name: 'vyrmQL contract lab' })).toBeVisible();
  await page.locator('#query-source').fill(
    `FROM record:prompt_flight AT VALID ${Date.now()} KNOWN HEAD PROJECT id, status LIMIT 5 EXPLAIN CONTRACT`,
  );
  await page.getByRole('button', { name: 'Plan and execute' }).click();
  await expect(page.locator('.query-contract-grid')).toContainText('Exact');
  await expect(page.locator('.query-paths .selected')).toContainText('authoritative log scan');
  await expect(page.locator('.query-paths .rejected')).toContainText('no projection generation');
  await expect(page.locator('.query-rows')).toContainText('record:prompt_flight:');
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
