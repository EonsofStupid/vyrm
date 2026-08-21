(() => {
  'use strict';

  const views = new Set(['overview', 'estates', 'tables', 'models', 'visuals', 'flight', 'stream', 'traces', 'cluster', 'graph', 'schema', 'query', 'runs', 'claims', 'routes', 'activity']);
  const visualViews = new Set(['visuals', 'flight', 'stream', 'traces', 'cluster', 'graph']);
  const initialView = location.hash.slice(1);
  const promptPresets = {
    weak: 'Make this better.',
    strong: 'Trace this request from intake through context, routing, model events, tools, verification, and outcome. Preserve read-only execution, retain every observable provider envelope, stop on stale evidence, and report token, cache, reasoning, latency, and tool-call evidence.',
  };
  const reasoningProfiles = {
    default: { effort: 'medium', title: 'Default', detail: 'Balanced starting point for a baseline.' },
    high: { effort: 'high', title: 'High', detail: 'More model exploration when evals show a gain.' },
    extreme: { effort: 'xhigh', title: 'Extreme', detail: 'Provider xhigh for difficult, quality-first work.' },
    ultra: { effort: 'max', title: 'Ultra', detail: 'Provider max; highest latency and token risk.' },
  };
  const state = {
    data: null,
    view: views.has(initialView) ? initialView : 'overview',
    selected: null,
    tableId: 'claims',
    modelScope: null,
    visualCursor: null,
    graphScope: 'local',
    graphKinds: new Set(['instance', 'subject', 'claim', 'run', 'event', 'evidence', 'file', 'invocation', 'flight', 'flight_event']),
    graphFocusKind: null,
    flightId: null,
    flightCursor: 0,
    flightPlaying: false,
    flightSpeed: 1,
    flightDirection: 1,
    flightTimer: null,
    flightPollTimer: null,
    streamCursor: null,
    streamPlaying: false,
    streamDirection: 1,
    streamSpeed: 1,
    streamTimer: null,
    traceId: null,
    traceSpanId: null,
    clusterCursor: null,
    clusterPlaying: false,
    clusterSpeed: 1,
    clusterDirection: 1,
    clusterTimer: null,
    refreshTimer: null,
    promptDraft: promptPresets.strong,
    flightSettings: { context: 'pruned', provider: 'codex', budget: 1500, acceptance: '', reasoning: 'default' },
  };

  const $ = (selector, root = document) => root.querySelector(selector);
  const $$ = (selector, root = document) => [...root.querySelectorAll(selector)];
  const escapeHtml = (value) => String(value ?? '')
    .replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;').replaceAll("'", '&#039;');
  const human = (value) => new Intl.NumberFormat().format(value ?? 0);
  const ago = (millis) => {
    const seconds = Math.max(0, Math.floor((Date.now() - millis) / 1000));
    if (seconds < 5) return 'just now';
    if (seconds < 60) return `${seconds}s ago`;
    if (seconds < 3600) return `${Math.floor(seconds / 60)}m ago`;
    return `${Math.floor(seconds / 3600)}h ago`;
  };
  const eventSummary = (event) => {
    const payload = event.payload || {};
    return payload.statement || payload.hypothesis || payload.summary || payload.rationale ||
      (payload.checks ? `${payload.checks.length} verification check(s)` : payload.outcome || payload.kind);
  };
  const color = (kind) => ({
    instance: '#8ce0ba', subject: '#75c9a6', claim: '#a7dfc7', run: '#b9a5f8',
    event: '#8374be', evidence: '#79cdda', file: '#e8b86d', invocation: '#879087',
    flight: '#f2f3ed', flight_event: '#d6c9ff'
  }[kind] || '#879087');

  async function load(silent = false) {
    try {
      const hadData = Boolean(state.data);
      const response = await fetch('/api/snapshot', { cache: 'no-store' });
      const data = await response.json();
      if (!response.ok) throw new Error(data.error || `HTTP ${response.status}`);
      state.data = data;
      $('#connection-dot').className = 'status-dot';
      $('#connection-label').textContent = 'Local runtime';
      if (!hadData && window.matchMedia('(max-width: 760px)').matches) {
        $('#inspector').classList.add('closed');
        $('.app-shell').classList.add('inspector-closed');
      }
      if (!state.selected) {
        const active = data.runs.find((run) => !run.complete);
        state.selected = active ? `run:${active.id}` : `instance:${data.instance.id}`;
      }
      updateChrome();
      if (!silent || !hadData || state.view !== 'flight') render();
      else renderInspector();
      if (!silent) toast('Runtime snapshot refreshed');
    } catch (error) {
      $('#connection-dot').className = 'status-dot error';
      $('#connection-label').textContent = 'Runtime unavailable';
      if (!silent) toast(error.message, true);
      if (!state.data) renderError(error.message);
    }
  }

  async function pollFlights() {
    if (!state.data || state.view !== 'flight') return;
    try {
      const response = await fetch('/api/flights', { cache: 'no-store' });
      const flights = await response.json();
      if (!response.ok) throw new Error(flights.error || `HTTP ${response.status}`);
      const before = flightRevision(state.data.flights || []);
      state.data.flights = flights;
      $('#flight-count').textContent = flights.length;
      if (flightRevision(flights) !== before) renderFlightStage();
    } catch (error) {
      toast(error.message, true);
    }
  }

  function flightRevision(flights) {
    return flights.map((flight) => {
      const event = flight.events[flight.events.length - 1];
      return `${flight.id}:${flight.status}:${flight.events.length}:${event?.ordinal ?? -1}:${flight.metrics?.latency_ms ?? ''}`;
    }).join('|');
  }

  function updateChrome() {
    const data = state.data;
    $('.instance-title').textContent = data.instance.id;
    $('.instance-title').classList.remove('skeleton');
    $('.instance-meta').textContent = `${data.instance.mode} · ${data.instance.root}`;
    $('#crumb-instance').textContent = data.instance.id;
    $('#crumb-view').textContent = state.view;
    $('#run-count').textContent = data.runs.length;
    $('#flight-count').textContent = data.flights.length;
    $('#stream-count').textContent = data.temporal_events.length;
    $('#trace-count').textContent = data.traces?.traces?.length || 0;
    $('#cluster-count').textContent = data.cluster?.nodes?.length || 0;
    $('#claim-count').textContent = data.claims.length;
    $('#file-count').textContent = data.files.length;
    $('#estate-count').textContent = (data.estates || []).length;
    $('#table-count').textContent = (data.tables || []).length;
    $('#model-count').textContent = (data.models || []).reduce((count, model) => count + Object.keys(model.registry.records || {}).length + Object.keys(model.registry.relations || {}).length + Object.keys(model.registry.events || {}).length, 0);
    $('#visual-count').textContent = 5;
    $('#snapshot-age').textContent = ago(data.generated_at);
    $$('.nav-item').forEach((button) => button.classList.toggle('active', button.dataset.view === state.view || (button.dataset.view === 'visuals' && visualViews.has(state.view))));
  }

  function render() {
    if (!state.data) return;
    updateChrome();
    const renderers = { overview: renderOverview, estates: renderEstates, tables: renderTables, models: renderModels, visuals: renderVisuals, flight: renderFlight, stream: renderStream, traces: renderTraces, cluster: renderCluster, graph: renderGraph, schema: renderSchema, query: renderQuery, runs: renderRuns, claims: renderClaims, routes: renderRoutes, activity: renderActivity };
    (renderers[state.view] || renderOverview)();
    renderInspector();
  }

  function pageHead(title, description, extra = '') {
    return `<header class="page-head"><div><div class="eyebrow">CONNECTOME / ${escapeHtml(state.view.toUpperCase())}</div><h1>${escapeHtml(title)}</h1><p>${escapeHtml(description)}</p></div>${extra}</header>`;
  }

  function renderOverview() {
    const { health, runs, claims, invocations } = state.data;
    const active = runs.find((run) => !run.complete);
    const signals = [
      ['mint', 'Storage engine', health.storage_backend, `cursor ${health.runtime_cursor}`],
      ['mint', 'Current projection', health.projection_state, `watermark ${health.projection_watermark}`],
      [health.routing_generation ? 'amber' : 'red', 'Source routing', health.routing_generation ? `generation ${health.routing_generation}` : 'projection absent', `${health.indexed_symbols} symbols`],
      [active ? 'violet' : 'mint', 'Reasoning contract', active ? `${active.id} · ${active.state}` : 'no active run', `${runs.length} total run(s)`],
      [health.last_grounded_at ? 'mint' : 'amber', 'Last grounding', health.last_grounded_at ? ago(health.last_grounded_at) : 'not yet grounded', 'differential evidence'],
      [health.retention_pins ? 'violet' : 'mint', 'Snapshot retention', `${health.snapshot_leases} live lease(s)`, health.oldest_retained_cursor == null ? 'no pinned cursor' : `cursor ≥ ${health.oldest_retained_cursor}`],
      [health.vector_artifacts ? 'violet' : 'amber', 'Vector artifact catalog', `${health.vector_artifacts} durable generation(s)`, `revision ${health.vector_catalog_revision}`],
    ];
    $('#main').innerHTML = pageHead('Runtime overview', 'A live, read-only view of the instance’s memory, reasoning contract, source projection, and operational evidence.', `<span class="badge ${health.state}"><span class="status-dot ${health.state === 'ready' ? '' : health.state}"></span>${health.state}</span>`) + `
      <section class="metrics">
        ${metric('Current claims', health.current_claims, `${health.subjects} subjects`)}
        ${metric('Claim sequence', health.claim_sequence, `projection at ${health.projection_watermark}`)}
        ${metric('Indexed source', health.indexed_files, `${health.indexed_symbols} symbols`)}
        ${metric('Vector artifacts', health.vector_artifacts, `catalog revision ${health.vector_catalog_revision}`)}
      </section>
      <section class="overview-grid">
        <article class="panel"><div class="panel-head"><h2>Active reasoning run</h2><span>${active ? active.state : 'IDLE'}</span></div><div class="panel-body">${active ? timeline(active.events.slice(-7)) : empty('No active run', 'A goal begins the next externally auditable reasoning sequence.')}</div></article>
        <article class="panel"><div class="panel-head"><h2>Runtime signals</h2><span>READ ONLY</span></div><div class="panel-body signal-list">${signals.map(([tone, name, detail, value]) => `<div class="signal-row"><span class="dot ${tone}"></span><div><div class="name">${escapeHtml(name)}</div><div class="detail">${escapeHtml(detail)}</div></div><div class="value">${escapeHtml(value)}</div></div>`).join('')}</div></article>
        <article class="panel"><div class="panel-head"><h2>Recent claims</h2><span>${claims.length} CURRENT</span></div><div class="panel-body signal-list">${claims.slice(0, 6).map((claim) => `<button class="signal-row lens-row object-link" data-object="claim:${claim.id}"><span class="dot mint"></span><div><div class="name">${escapeHtml(claim.subject)} · ${escapeHtml(claim.predicate)}</div><div class="detail">${escapeHtml(claim.object)}</div></div><div class="value">${escapeHtml(claim.producer.actor)}</div></button>`).join('') || empty('No current claims', 'Record runtime knowledge through the existing claim surface.')}</div></article>
        <article class="panel"><div class="panel-head"><h2>Recent activity</h2><span>${invocations.length} RECORDED</span></div><div class="panel-body signal-list">${invocations.slice(-6).reverse().map((item) => `<button class="signal-row lens-row object-link" data-object="invocation:${item.ordinal}"><span class="dot ${item.outcome === 'ok' ? 'mint' : 'red'}"></span><div><div class="name">${escapeHtml(item.command)}</div><div class="detail">${escapeHtml(item.detail || item.trigger)}</div></div><div class="value">${item.duration_ms} ms</div></button>`).join('') || empty('No activity', 'Every operator and lifecycle call will appear here.')}</div></article>
      </section>`;
    bindObjectLinks();
  }

  function metric(label, value, meta) {
    return `<article class="metric-card"><div class="label">${escapeHtml(label)}</div><div class="value">${human(value)}</div><div class="meta">${escapeHtml(meta)}</div></article>`;
  }

  function empty(title, detail) {
    return `<div class="empty-state"><div><strong>${escapeHtml(title)}</strong><span>${escapeHtml(detail)}</span></div></div>`;
  }

  function timeline(events) {
    return `<div class="timeline">${events.map((event) => `<button class="timeline-event lens-row object-link" data-object="event:${event.run_id}:${event.ordinal}"><div><div class="event-kind">${escapeHtml(event.payload.kind)}</div><div class="event-summary">${escapeHtml(eventSummary(event))}</div><div class="event-meta">#${event.ordinal} · ${escapeHtml(event.actor)}</div></div></button>`).join('')}</div>`;
  }

  function renderEstates() {
    const estates = state.data.estates || [];
    const estate = estates[0];
    if (!estate) {
      $('#main').innerHTML = pageHead('Estates', 'Project-owned runtime boundaries and the instances observed inside them.') + empty('No estate bound', 'Connectome is not attached to a Vyrm instance.');
      return;
    }
    const nodes = state.data.cluster?.nodes || [];
    const visibleNodes = nodes.length ? nodes : [{ canonical_node_id: estate.id, state: estate.state, shard: 0, raft_node_id: 0, applied_lag: 0, latest_cursor: estate.runtime_cursor, alerts: [] }];
    $('#main').innerHTML = pageHead('Estates', 'The local project boundary, its storage ownership, and every observed runtime member. Cloud fleet control remains a later control plane.', `<span class="badge ${estate.state}">${escapeHtml(estate.control_plane)} control</span>`) + `
      <section class="metrics estate-metrics">
        ${metric('Bound estates', estates.length, 'one project boundary')}
        ${metric('Observed nodes', visibleNodes.length, nodes.length ? 'retained telemetry' : 'local process')}
        ${metric('Runtime head', estate.runtime_cursor, 'global commit cursor')}
        ${metric('Data surfaces', state.data.tables?.length || 0, `${state.data.models?.length || 0} schema scope(s)`)}
      </section>
      <section class="estate-layout">
        <article class="estate-map" aria-label="Estate topology">
          <header><div><span class="eyebrow">LOCAL ESTATE</span><h2>${escapeHtml(estate.id)}</h2></div><code>${escapeHtml(estate.mode)}</code></header>
          <div class="estate-core"><span class="estate-orbit"></span><strong>${escapeHtml(estate.id)}</strong><small>${escapeHtml(estate.storage_backend)} · cursor ${human(estate.runtime_cursor)}</small></div>
          <div class="estate-node-grid">${visibleNodes.map((node) => `<button type="button" class="estate-node ${node.alerts?.length ? 'attention' : ''}" data-open-estate-node="${escapeHtml(node.latest_sample_digest || '')}"><span></span><strong>${escapeHtml(node.canonical_node_id)}</strong><code>shard ${human(node.shard)} · raft ${human(node.raft_node_id)}</code><small>${escapeHtml(node.state)} · lag ${human(node.applied_lag)}</small></button>`).join('')}</div>
        </article>
        <aside class="estate-contracts">
          <article><span>STORAGE BOUNDARY</span><strong>${escapeHtml(estate.storage_backend)}</strong><code>${escapeHtml(estate.root)}</code><small>Member ${escapeHtml(estate.member)}</small></article>
          <article><span>PROJECT SCOPE</span><strong>${escapeHtml(estate.scope)}</strong><code>cursor ${human(estate.runtime_cursor)}</code><small>All views remain bound to this manifest.</small></article>
          <article class="future-boundary"><span>HOSTING CONTROL PLANE</span><strong>Not attached</strong><code>local-only</code><small>Reserved for the later cloud hosting tab; no remote action is implied.</small></article>
          <button type="button" class="primary-button estate-action" data-go="cluster">Open retained cluster history</button>
        </aside>
      </section>`;
    $$('[data-go]').forEach((button) => button.addEventListener('click', () => navigate(button.dataset.go)));
    $$('[data-open-estate-node]').forEach((button) => button.addEventListener('click', () => {
      if (button.dataset.openEstateNode) select(`cluster-sample:${button.dataset.openEstateNode}`);
    }));
  }

  function rowsForTable(id) {
    const data = state.data;
    return ({
      claims: data.claims,
      reasoning_runs: data.runs,
      prompt_flights: data.flights,
      temporal_events: data.temporal_events,
      causal_traces: data.traces?.traces || [],
      source_files: data.files,
      invocations: data.invocations,
      cluster_samples: data.cluster?.samples || [],
      vector_artifacts: data.vector_artifacts,
    })[id] || [];
  }

  function cellValue(value) {
    if (value == null) return '—';
    if (Array.isArray(value)) return `${value.length} item${value.length === 1 ? '' : 's'}`;
    if (typeof value === 'object') {
      const identity = value.id || value.kind || value.state || value.actor || value.digest;
      return identity ? String(identity) : JSON.stringify(value);
    }
    return String(value);
  }

  function tableObjectId(table, row) {
    if (table === 'claims') return `claim:${row.id}`;
    if (table === 'reasoning_runs') return `run:${row.id}`;
    if (table === 'source_files') return `file:${row.path}`;
    if (table === 'invocations') return `invocation:${row.ordinal}`;
    if (table === 'prompt_flights') return `flight:${row.id}`;
    if (table === 'temporal_events') return `runtime-change:${row.cursor}`;
    if (table === 'cluster_samples') return `cluster-sample:${row.digest}`;
    return '';
  }

  function renderTables() {
    const tables = state.data.tables || [];
    if (!tables.some((table) => table.id === state.tableId)) state.tableId = tables[0]?.id || null;
    const selected = tables.find((table) => table.id === state.tableId);
    const rows = selected ? rowsForTable(selected.id) : [];
    const preferred = ['id', 'kind', 'subject', 'predicate', 'state', 'status', 'scope', 'actor', 'path', 'cursor', 'ordinal', 'created_at'];
    const discovered = [...new Set(rows.slice(0, 20).flatMap((row) => Object.keys(row || {})))];
    const columns = [...preferred.filter((key) => discovered.includes(key)), ...discovered.filter((key) => !preferred.includes(key))].slice(0, 6);
    $('#main').innerHTML = pageHead('Tables', 'Logical runtime surfaces with their source of truth made explicit. Select one surface to inspect its current read-only rows.', selected ? `<span class="badge ready">known at cursor ${human(selected.known_at_cursor)}</span>` : '') + `
      <section class="table-workspace">
        <aside class="table-catalog" aria-label="Runtime tables">${tables.map((table) => `<button type="button" class="table-catalog-row ${table.id === state.tableId ? 'active' : ''}" data-table-id="${escapeHtml(table.id)}"><span class="table-glyph ${escapeHtml(table.category)}">${table.bounded ? '≈' : '▤'}</span><div><strong>${escapeHtml(table.label)}</strong><small>${escapeHtml(table.authority)} · ${table.bounded ? 'bounded response' : 'current view'}</small></div><b>${human(table.rows)}</b></button>`).join('')}</aside>
        <article class="table-preview">
          ${selected ? `<header><div><span class="eyebrow">${escapeHtml(selected.category)} / ${escapeHtml(selected.authority)}</span><h2>${escapeHtml(selected.label)}</h2><p>${escapeHtml(selected.description)}</p></div><strong>${human(rows.length)} rows loaded · ${selected.bounded ? 'bounded response' : 'current view'}</strong></header>
          <div class="table-scroll"><table class="data-table table-dataset"><thead><tr>${columns.map((column) => `<th>${escapeHtml(column.replaceAll('_', ' '))}</th>`).join('')}</tr></thead><tbody>${rows.slice(0, 100).map((row) => { const object = tableObjectId(selected.id, row); return `<tr ${object ? `data-object="${escapeHtml(object)}"` : ''}>${columns.map((column) => `<td><span class="table-cell-value">${escapeHtml(cellValue(row[column]))}</span></td>`).join('')}</tr>`; }).join('')}</tbody></table>${rows.length ? '' : empty('No rows in this surface', 'The table contract exists but has no current data.')}</div>
          <footer><span>${selected.bounded ? 'Response is retention- or limit-bounded; row count is not a lifetime total.' : 'Snapshot-consistent current rows.'}</span><button type="button" data-go="query">Open Query Lab</button></footer>` : empty('No table catalog', 'The runtime did not expose any logical data surfaces.')}
        </article>
      </section>`;
    $$('[data-table-id]').forEach((button) => button.addEventListener('click', () => { state.tableId = button.dataset.tableId; renderTables(); }));
    $$('[data-object]', $('#main')).forEach((row) => row.addEventListener('click', () => select(row.dataset.object)));
    $$('[data-go]').forEach((button) => button.addEventListener('click', () => navigate(button.dataset.go)));
  }

  function renderModels() {
    const models = state.data.models?.length ? state.data.models : (state.data.schema ? [{ scope: 'reasoning', registry: state.data.schema }] : []);
    if (!state.modelScope || !models.some((model) => model.scope === state.modelScope)) state.modelScope = models[0]?.scope || null;
    const model = models.find((candidate) => candidate.scope === state.modelScope);
    if (!model) {
      $('#main').innerHTML = pageHead(state.view === 'schema' ? 'Runtime schema' : 'Data models', 'Typed records, events, relations, and their enforcement rules.') + empty('No model installed', 'The first governed runtime write installs a scoped schema.');
      return;
    }
    const schema = model.registry;
    const records = Object.entries(schema.records || {});
    const relations = Object.entries(schema.relations || {});
    const events = Object.entries(schema.events || {});
    $('#main').innerHTML = pageHead(state.view === 'schema' ? 'Runtime schema' : 'Data models', 'Every persisted model by runtime scope, including legal relationships, required properties, and deny-by-default boundaries.', `<span class="badge ready">revision ${human(schema.revision)}</span>`) + `
      <div class="model-scope-tabs" aria-label="Data model scopes">${models.map((candidate) => `<button type="button" class="${candidate.scope === state.modelScope ? 'active' : ''}" data-model-scope="${escapeHtml(candidate.scope)}"><span>${escapeHtml(candidate.scope)}</span><b>r${human(candidate.registry.revision)}</b></button>`).join('')}</div>
      <section class="model-map" aria-label="Data model relationship map">
        <header><span>RECORD TYPES</span><span>RELATION PATHS</span><span>EVENT TYPES</span></header>
        <div class="model-column">${records.map(([kind]) => `<div class="model-node record"><i></i><strong>${escapeHtml(kind.replaceAll('_', ' '))}</strong></div>`).join('') || '<div class="model-empty">No records</div>'}</div>
        <div class="model-column relations">${relations.map(([kind, definition]) => `<div class="model-relation"><small>${escapeHtml((definition.from || []).join(' | '))}</small><strong>— ${escapeHtml(kind)} →</strong><small>${escapeHtml((definition.to || []).join(' | '))}</small></div>`).join('') || '<div class="model-empty">No relations</div>'}</div>
        <div class="model-column">${events.map(([kind, definition]) => `<div class="model-node event"><i></i><strong>${escapeHtml(kind.replaceAll('_', ' '))}</strong><small>${definition.subject_required ? 'subject required' : 'subject optional'}</small></div>`).join('') || '<div class="model-empty">No events</div>'}</div>
      </section>
      <section class="schema-contract-line" aria-label="Schema enforcement sequence">
        <div><span>01</span><strong>Registry</strong><small>revision ${human(schema.revision)}</small></div><i>→</i>
        <div><span>02</span><strong>Bind</strong><small>legal types + fields</small></div><i>→</i>
        <div><span>03</span><strong>Validate</strong><small>cardinality + identity</small></div><i>→</i>
        <div><span>04</span><strong>Deny or commit</strong><small>one ACID boundary</small></div>
      </section>
      <section class="schema-revision"><span class="eyebrow">CURRENT MIGRATION</span><strong>${escapeHtml(schema.migration)}</strong><small>Scope ${escapeHtml(model.scope)} · every later registry advances exactly one revision.</small></section>
      <section class="schema-groups">
        ${schemaGroup('Record types', 'Persistent graph objects with required, optional, and unique properties.', records, 'record')}
        ${schemaGroup('Event types', 'Immutable lifecycle facts with governed subject types and payloads.', events, 'event')}
        ${schemaGroup('Relation types', 'Directed edges with legal endpoints and temporal cardinality.', relations, 'relation')}
      </section>`;
    $$('[data-model-scope]').forEach((button) => button.addEventListener('click', () => { state.modelScope = button.dataset.modelScope; renderModels(); }));
  }

  function renderVisuals() {
    const events = state.data.temporal_events || [];
    if (state.visualCursor == null || !events.some((event) => event.cursor === state.visualCursor)) state.visualCursor = events.at(-1)?.cursor || null;
    const activeIndex = Math.max(0, events.findIndex((event) => event.cursor === state.visualCursor));
    const active = events[activeIndex];
    const windowed = events.slice(Math.max(0, events.length - 48));
    const lanes = [['reasoning', 'REASONING'], ['workflow', 'LIFECYCLE'], ['routing', 'ROUTING'], ['search', 'SEARCH'], ['storage', 'STORAGE'], ['data', 'DATA']];
    const modes = [
      ['flight', 'Prompt flight', 'Provider events, reasoning effort, tokens, and acceptance'],
      ['stream', 'Temporal flow', 'Every committed mutation with replay and audit evidence'],
      ['traces', 'Causal traces', 'Parent/child spans, critical path, and integrity diagnostics'],
      ['graph', 'Runtime graph', 'Claims, reasoning, source, and provenance neighborhoods'],
      ['cluster', 'Estate topology', 'Node history, replication lag, transport, and alerts'],
    ];
    $('#main').innerHTML = pageHead('Visuals', 'Choose one faithful runtime lens, then freeze, rewind, inspect, or open it at full depth. Every mark below comes from the current snapshot.', `<span class="badge ready">${human(events.length)} events loaded</span>`) + `
      <nav class="visual-deck" aria-label="Available runtime visualizations">${modes.map(([view, title, detail]) => `<button type="button" data-open-visual="${view}"><span>${escapeHtml(title)}</span><small>${escapeHtml(detail)}</small><b>Open →</b></button>`).join('')}</nav>
      <section class="observatory">
        <header><div><span class="eyebrow">LIVE BASELINE / GLOBAL LOG</span><h2>Runtime signal observatory</h2></div><div class="observatory-controls"><button type="button" id="visual-prev" aria-label="Previous event">◀</button><strong>${active ? `cursor ${human(active.cursor)}` : 'waiting'}</strong><button type="button" id="visual-next" aria-label="Next event">▶</button></div></header>
        ${active ? `<div class="signal-observatory">${lanes.map(([family, label]) => `<div class="observatory-lane"><label>${label}</label><div>${windowed.map((event) => event.family === family ? `<button type="button" class="observatory-packet ${event.cursor === active.cursor ? 'active' : ''}" data-visual-cursor="${event.cursor}" aria-label="cursor ${event.cursor}: ${escapeHtml(event.label)}"><i></i></button>` : '<span></span>').join('')}</div></div>`).join('')}</div>
        <article class="observatory-readout"><span class="event-kind">${escapeHtml(active.family)} / ${escapeHtml(active.action)}</span><h3>${escapeHtml(active.label)}</h3><p>${escapeHtml(active.detail)}</p><footer><code>${escapeHtml(active.scope)} · ${escapeHtml(active.commit_id.slice(0, 14))}…</code><button type="button" id="visual-inspect">Inspect mutation + audit</button></footer></article>` : empty('No runtime events', 'Committed mutations will form the first visual baseline.')}
      </section>`;
    $$('[data-open-visual]').forEach((button) => button.addEventListener('click', () => navigate(button.dataset.openVisual)));
    $$('[data-visual-cursor]').forEach((button) => button.addEventListener('click', () => { state.visualCursor = Number(button.dataset.visualCursor); renderVisuals(); }));
    $('#visual-prev')?.addEventListener('click', () => { if (events.length) { state.visualCursor = events[Math.max(0, activeIndex - 1)].cursor; renderVisuals(); } });
    $('#visual-next')?.addEventListener('click', () => { if (events.length) { state.visualCursor = events[Math.min(events.length - 1, activeIndex + 1)].cursor; renderVisuals(); } });
    $('#visual-inspect')?.addEventListener('click', () => select(`runtime-change:${active.cursor}`));
  }

  function currentFlight() {
    const flights = state.data.flights || [];
    const latest = [...flights].sort((a, b) => b.created_at - a.created_at)[0] || null;
    if (!state.flightId && latest) state.flightId = latest.id;
    return flights.find((flight) => flight.id === state.flightId) || latest;
  }

  function renderFlight() {
    const flight = currentFlight();
    const enabled = state.data.capabilities.runners_enabled;
    const providers = state.data.capabilities.providers || ['observe'];
    $('#main').innerHTML = pageHead(
      'Reasoning flight lab',
      'Run one prompt at one real provider effort. Watch the observable runtime unfold, freeze any event, and repeat the same prompt at another effort to build a trustworthy baseline.',
      `<div class="flight-head-actions"><span class="badge ${enabled ? 'ready' : 'attention'}">${enabled ? 'frontier runners armed' : 'observe-only mode'}</span></div>`
    ) + `
      <form id="flight-form" class="reasoning-lab">
        <div class="prompt-presets"><span>STARTING PROMPT</span><button type="button" data-prompt-preset="weak">Weak example</button><button type="button" data-prompt-preset="strong">Strong example</button><small>or type your own</small></div>
        <label class="single-prompt-editor">
          <textarea id="flight-prompt" rows="5" maxlength="65536" required>${escapeHtml(state.promptDraft)}</textarea>
          <div id="prompt-contract" class="prompt-contract">${promptContract(state.promptDraft)}</div>
        </label>
        <section class="reasoning-profile-picker" aria-label="Reasoning effort">
          <header><div><span class="eyebrow">REASONING PROFILE</span><strong>How much provider effort should this run request?</strong></div><small>These map to exact provider values. They do not expose private chain-of-thought.</small></header>
          <div>${Object.entries(reasoningProfiles).map(([id, profile]) => `<button type="button" data-reasoning-profile="${id}" class="${state.flightSettings.reasoning === id ? 'active' : ''}"><span>${profile.title}</span><b>${profile.effort}</b><small>${profile.detail}</small></button>`).join('')}</div>
        </section>
        <div class="flight-options single-run-options">
          <label><span>PROVIDER</span><select id="flight-provider">${providers.map((provider) => `<option value="${escapeHtml(provider)}" ${state.flightSettings.provider === provider ? 'selected' : ''}>${escapeHtml(provider === 'observe' ? 'Observe runtime only' : provider)}</option>`).join('')}</select></label>
          <label><span>CONTEXT</span><select id="flight-context"><option value="fresh" ${state.flightSettings.context === 'fresh' ? 'selected' : ''}>Fresh · zero injected</option><option value="pruned" ${state.flightSettings.context === 'pruned' ? 'selected' : ''}>Pruned · relevant only</option><option value="full" ${state.flightSettings.context === 'full' ? 'selected' : ''}>Full · bounded preflight</option></select></label>
          <label><span>CONTEXT BUDGET</span><input id="flight-budget" type="number" min="128" max="32000" value="${state.flightSettings.budget}"></label>
          <label><span>ACCEPTANCE MARKER</span><input id="flight-acceptance" value="${escapeHtml(state.flightSettings.acceptance)}" placeholder="text required in output"></label>
          <button class="launch-button" type="submit"><span>Run one flight</span><b>↗</b></button>
        </div>
        <p class="composer-note">One click creates one persisted run. Repeat the exact prompt at another profile to compare effort, reasoning tokens, cache use, tool fanout, latency, and acceptance. ${enabled ? 'Codex and Claude execute in ephemeral read-only/plan mode.' : 'Restart with --enable-runners for real Codex or Claude model events.'}</p>
      </form>
      <div id="flight-stage">${flight ? flightExperience(flight) : empty('No reasoning flights yet', 'Run one prompt to begin the observable timeline.')}</div>`;
    $('#flight-form').addEventListener('submit', launchFlight);
    $('#flight-prompt').addEventListener('input', (event) => {
      stopFlightPlayback();
      state.promptDraft = event.target.value;
      $('#prompt-contract').innerHTML = promptContract(event.target.value);
    });
    $$('[data-prompt-preset]').forEach((button) => button.addEventListener('click', () => setPromptPreset(button.dataset.promptPreset)));
    $$('[data-reasoning-profile]').forEach((button) => button.addEventListener('click', () => {
      state.flightSettings.reasoning = button.dataset.reasoningProfile;
      $$('[data-reasoning-profile]').forEach((candidate) => candidate.classList.toggle('active', candidate === button));
    }));
    $('#flight-context').addEventListener('change', (event) => { state.flightSettings.context = event.target.value; });
    $('#flight-provider').addEventListener('change', (event) => { state.flightSettings.provider = event.target.value; });
    $('#flight-budget').addEventListener('input', (event) => { state.flightSettings.budget = Number(event.target.value); });
    $('#flight-acceptance').addEventListener('input', (event) => { state.flightSettings.acceptance = event.target.value; });
    bindFlightControls(flight);
  }

  function promptContract(value) {
    const text = value.trim();
    const words = text ? text.split(/\s+/).length : 0;
    const signals = [
      ['goal', /\b(build|create|make|fix|trace|compare|explain|implement|review|measure|show|find|replace|optimi[sz]e)\b/i.test(text)],
      ['scope', /\b(runtime|prompt|context|routing|tools?|verification|outcome|file|module|api|ui|graph|storage|read-only)\b/i.test(text)],
      ['evidence', /\b(cite|digest|evidence|observe|measure|metric|test|verify|trace)\b/i.test(text)],
      ['success', /\b(accept|pass|must|report|result|done when|ensure|confirm)\b/i.test(text)],
      ['stop', /\b(stop|fail|deny|avoid|never|read-only|stale|boundary)\b/i.test(text)],
    ];
    return `<div class="contract-summary"><span>${words} words</span><b>${signals.filter(([, present]) => present).length}/5 explicit signals</b></div><div class="contract-signals">${signals.map(([name, present]) => `<span class="${present ? 'present' : ''}"><i></i>${name}</span>`).join('')}</div>`;
  }

  function setPromptPreset(preset) {
    state.promptDraft = promptPresets[preset];
    $('#flight-prompt').value = state.promptDraft;
    $('#prompt-contract').innerHTML = promptContract(state.promptDraft);
    stopFlightPlayback();
  }

  function flightExperience(flight) {
    state.flightCursor = Math.min(state.flightCursor, Math.max(0, flight.events.length - 1));
    const event = flight.events[state.flightCursor] || null;
    const stages = ['prompt', 'context', 'recall', 'routing', 'model', 'tools', 'outcome'];
    const activeStage = Math.max(0, stages.indexOf(event?.stage || 'prompt'));
    const comparable = comparableFlights(flight);
    const metrics = flight.metrics || {};
    const profile = flightProfile(flight);
    const effortState = flight.provider === 'observe' ? 'CONFIGURED' : 'REQUESTED';
    const burst = event ? signalVolume(event) : 0;
    const totalVolume = flight.events.reduce((total, item) => total + signalVolume(item), 0);
    const visibleVolume = flight.events.slice(0, state.flightCursor + 1).reduce((total, item) => total + signalVolume(item), 0);
    const payloadKeys = event && event.data && typeof event.data === 'object' ? Object.keys(event.data).slice(0, 6) : [];
    return `
      <section class="flight-switcher">
        <div><span class="eyebrow">${escapeHtml(flight.provider)} · ${profile.title} · ${effortState} ${profile.effort}</span><strong>${escapeHtml(flight.prompt)}</strong><small>Observable runtime evidence only · no hidden chain-of-thought</small></div>
        <div class="effort-run-tabs">${comparable.sort((a, b) => a.created_at - b.created_at).map((candidate) => { const candidateProfile = flightProfile(candidate); return `<button type="button" data-compare-flight="${escapeHtml(candidate.id)}" class="${candidate.id === flight.id ? 'active' : ''}"><span>${candidateProfile.title}</span><b>${candidateProfile.effort}</b><small>${escapeHtml(candidate.status)} · ${candidate.events.length} events</small></button>`; }).join('')}</div>
      </section>
      <section class="flight-metrics">
        ${flightMetric('Effort', profile.effort, flight.provider === 'observe' ? `${profile.title} · not executed` : profile.title)}
        ${flightMetric('Vyrm context', metrics.context_tokens ?? 0, 'tokens injected')}
        ${flightMetric('Input / output', metrics.input_tokens == null ? '—' : `${human(metrics.input_tokens)} / ${human(metrics.output_tokens || 0)}`, metrics.input_tokens == null ? 'unreported' : 'tokens')}
        ${flightMetric('Reasoning', metrics.reasoning_tokens ?? '—', metrics.reasoning_tokens == null ? 'provider did not report' : 'tokens')}
        ${flightMetric('Cache read', metrics.cached_input_tokens ?? '—', metrics.cached_input_tokens == null ? 'unreported' : 'tokens')}
        ${flightMetric('Events / tools', `${metrics.provider_events || 0} / ${metrics.tool_calls || 0}`, 'observable')}
        ${flightMetric('Latency', metrics.latency_ms ?? '—', metrics.latency_ms == null ? 'in flight' : 'ms')}
        ${flightMetric('Acceptance', metrics.acceptance_met == null ? '—' : metrics.acceptance_met ? 'met' : 'missed', escapeHtml(flight.status))}
      </section>
      <section class="flight-visual pos-${activeStage} ${flight.demo_role ? `demo-${escapeHtml(flight.demo_role)}` : ''}">
        <div class="visual-readout"><span>EVENT MASS <b>${human(visibleVolume)} / ${human(totalVolume)} B</b></span><span>FROZEN AT <b>${event ? `#${event.ordinal} · ${escapeHtml(event.kind.replaceAll('_', ' '))}` : 'waiting'}</b></span></div>
        <div class="flight-aurora" aria-hidden="true"><i></i><i></i><i></i></div>
        <div class="flight-rail"></div>
        <div class="flight-particle" aria-hidden="true"><i></i></div>
        <div class="flight-stages">${stages.map((stage, index) => `<button type="button" class="flight-node ${index < activeStage ? 'passed' : ''} ${index === activeStage ? 'active' : ''}" data-stage="${stage}"><span><i></i></span><b>${stage}</b><small>${flight.events.filter((item) => item.stage === stage).length}</small></button>`).join('')}</div>
        <div class="event-burst burst-intensity-${burstLevel(burst)}" aria-hidden="true">${Array.from({ length: 12 }, (_, index) => `<i class="ray-${index}"></i>`).join('')}</div>
        <div class="information-cloud" aria-hidden="true">${payloadKeys.map((key) => `<span>${escapeHtml(key.replaceAll('_', ' '))}</span>`).join('')}</div>
        ${telemetryRiver(flight)}
      </section>
      <section class="flight-console">
        <div class="playback-controls">
          <button type="button" id="flight-start" class="transport-button" title="First event">⏮ First</button>
          <button type="button" id="flight-rewind" class="transport-button" title="Rewind through time">◀ Rewind</button>
          <button type="button" id="flight-play" class="transport-button primary" title="Play or freeze">${state.flightPlaying ? '❚❚ Freeze time' : '▶ Resume time'}</button>
          <button type="button" id="flight-forward" class="transport-button" title="Fast-forward through time">Forward ▶</button>
          <button type="button" id="flight-end" class="transport-button" title="Latest event">Latest ⏭</button>
          <label class="scrubber"><span>#${event ? event.ordinal : 0}</span><input id="flight-scrub" type="range" min="0" max="${Math.max(0, flight.events.length - 1)}" value="${state.flightCursor}"><b>${flight.events.length} events</b></label>
          <select id="flight-speed" aria-label="Playback speed"><option value="0.5" ${state.flightSpeed === .5 ? 'selected' : ''}>0.5×</option><option value="1" ${state.flightSpeed === 1 ? 'selected' : ''}>1×</option><option value="2" ${state.flightSpeed === 2 ? 'selected' : ''}>2×</option><option value="4" ${state.flightSpeed === 4 ? 'selected' : ''}>4×</option><option value="8" ${state.flightSpeed === 8 ? 'selected' : ''}>8×</option></select>
        </div>
        ${event ? microEvent(event, flight) : empty('Waiting for first event', 'The flight has been created but has not emitted an observable event.')}
        <div class="event-filmstrip">${flight.events.map((item, index) => `<button type="button" data-flight-event="${index}" class="film-frame ${index === state.flightCursor ? 'active' : ''}"><span>${item.ordinal}</span><i class="stage-${escapeHtml(item.stage)}"></i><b>${escapeHtml(item.kind.replaceAll('_', ' '))}</b><small>+${human(item.elapsed_ms)} ms</small></button>`).join('')}</div>
      </section>
      ${comparison(comparable, flight)}
      ${flightHistory(comparable)}`;
  }

  function comparableFlights(flight) {
    const flights = state.data.flights || [];
    return flights.filter((candidate) => flight.comparison_id
      ? candidate.comparison_id === flight.comparison_id
      : candidate.cohort_id === flight.cohort_id);
  }

  function flightProfile(flight) {
    return reasoningProfiles[flight.reasoning_profile || 'default'] || reasoningProfiles.default;
  }

  function eventLane(event) {
    if (['context', 'recall', 'routing'].includes(event.stage)) return 'context';
    if (event.stage === 'tools') return 'tools';
    if (event.stage === 'outcome') return 'outcome';
    return 'model';
  }

  function telemetryRiver(flight) {
    const lanes = [
      ['context', 'CONTEXT + ROUTING'],
      ['model', 'MODEL ENVELOPES'],
      ['tools', 'TOOL ACTIVITY'],
      ['outcome', 'EVIDENCE + OUTCOME'],
    ];
    return `<div class="telemetry-river" aria-label="Observable event mass across runtime lanes"><div class="telemetry-axis"><span>0 ms</span><b>OBSERVABLE EVENT MASS · click any packet to freeze</b><span>+${human(flight.events.at(-1)?.elapsed_ms || 0)} ms</span></div>${lanes.map(([id, label]) => `<div class="telemetry-lane"><label>${label}</label><div class="telemetry-track">${flight.events.map((item, index) => eventLane(item) === id ? `<button type="button" data-burst-event="${index}" class="telemetry-packet burst-${burstLevel(signalVolume(item))} ${index === state.flightCursor ? 'active' : ''}" title="#${item.ordinal} ${escapeHtml(item.label)} · ${human(signalVolume(item))} B"><i></i></button>` : '<span></span>').join('')}</div></div>`).join('')}</div>`;
  }

  function flightHistory(comparable) {
    const active = new Set(comparable.map((flight) => flight.id));
    const others = [...(state.data.flights || [])]
      .filter((flight) => !active.has(flight.id))
      .sort((a, b) => b.created_at - a.created_at);
    if (!others.length) return '';
    return `<details class="flight-history"><summary>Other recorded runs <span>${others.length}</span></summary><div>${others.map((flight) => `<button type="button" data-history-flight="${escapeHtml(flight.id)}"><span>${escapeHtml(flight.demo_role || flight.context_mode)}</span><b>${escapeHtml(flight.prompt)}</b><small>${escapeHtml(flight.status)} · ${flight.events.length} events</small></button>`).join('')}</div></details>`;
  }

  function signalVolume(event) {
    return String(event?.detail || '').length + JSON.stringify(event?.data || {}).length;
  }

  function burstLevel(volume) {
    return Math.max(1, Math.min(10, Math.ceil(volume / 38)));
  }

  function flightMetric(label, value, unit) {
    return `<div><span>${escapeHtml(label)}</span><strong>${escapeHtml(value)}</strong><small>${escapeHtml(unit)}</small></div>`;
  }

  function microEvent(event, flight) {
    const before = flight.events[event.ordinal - 1];
    const delta = before ? event.elapsed_ms - before.elapsed_ms : event.elapsed_ms;
    const fields = event.data && typeof event.data === 'object' ? Object.keys(event.data) : [];
    const stageEvents = flight.events.filter((candidate) => candidate.stage === event.stage).length;
    return `<article class="micro-event">
      <header><div><span class="eyebrow">FROZEN MICRO-EVENT · ${escapeHtml(event.stage)}</span><h2>${escapeHtml(event.label)}</h2></div><div class="event-clock"><strong>+${human(event.elapsed_ms)} ms</strong><span>Δ ${human(delta)} ms</span></div></header>
      <div class="event-data-strip"><div><span>signal volume</span><strong>${human(signalVolume(event))} B</strong></div><div><span>typed fields</span><strong>${fields.length}</strong></div><div><span>stage events</span><strong>${stageEvents}</strong></div><div><span>timeline</span><strong>${event.ordinal + 1}/${flight.events.length}</strong></div></div>
      <p>${escapeHtml(event.detail)}</p>
      ${fields.length ? `<dl class="payload-breakdown">${fields.map((key) => `<div><dt>${escapeHtml(key.replaceAll('_', ' '))}</dt><dd>${escapeHtml(formatPayloadValue(event.data[key]))}</dd></div>`).join('')}</dl>` : ''}
      <details class="event-envelope"><summary>Full observable envelope <span>${human(signalVolume(event))} B captured</span></summary><pre>${escapeHtml(JSON.stringify(event.data ?? null, null, 2))}</pre></details>
      <footer><span>${escapeHtml(event.kind)}</span><span>${new Date(event.at).toLocaleTimeString()}</span><button type="button" id="inspect-flight-event">Inspect raw event</button></footer>
    </article>`;
  }

  function formatPayloadValue(value) {
    if (typeof value === 'string') return value;
    return JSON.stringify(value);
  }

  function comparison(flights, selected) {
    if (flights.length < 2) return `<section class="comparison-empty"><span class="eyebrow">SAME-PROMPT EFFORT BASELINE</span><p>Run this exact prompt again at High, Extreme, or Ultra. Vyrm will compare only observed cost and outcome evidence.</p></section>`;
    const maxima = {
      context: Math.max(...flights.map((flight) => flight.metrics.context_tokens || 0), 1),
      tools: Math.max(...flights.map((flight) => flight.metrics.tool_calls || 0), 1),
      latency: Math.max(...flights.map((flight) => flight.metrics.latency_ms || 0), 1),
    };
    const verdict = 'Requested effort is a controlled input. Token use, latency, tools, cache, acceptance, and output are evidence; more effort is never assumed to be better.';
    return `<section class="comparison-panel"><div class="panel-head"><h2>Same-prompt effort baseline</h2><span>${flights.length} OBSERVED TRACES</span></div><div class="comparison-grid">${flights.map((flight) => {
      const tokens = (flight.metrics.input_tokens ?? 0) + (flight.metrics.output_tokens ?? 0);
      const profile = flightProfile(flight);
      return `<button type="button" data-compare-flight="${escapeHtml(flight.id)}" class="comparison-arm ${flight.id === selected.id ? 'selected' : ''}"><span>${profile.title} · ${profile.effort}</span><strong>${escapeHtml(flight.context_mode)} context</strong><div class="comparison-bars"><div><label>context <b>${human(flight.metrics.context_tokens)}</b></label><i class="level-${barLevel(flight.metrics.context_tokens, maxima.context)}"></i></div><div><label>tools <b>${human(flight.metrics.tool_calls)}</b></label><i class="level-${barLevel(flight.metrics.tool_calls, maxima.tools)}"></i></div><div><label>latency <b>${flight.metrics.latency_ms == null ? '—' : `${human(flight.metrics.latency_ms)} ms`}</b></label><i class="level-${barLevel(flight.metrics.latency_ms, maxima.latency)}"></i></div></div><dl><div><dt>provider tokens</dt><dd>${flight.metrics.input_tokens == null ? '—' : human(tokens)}</dd></div><div><dt>reasoning tokens</dt><dd>${flight.metrics.reasoning_tokens == null ? '—' : human(flight.metrics.reasoning_tokens)}</dd></div><div><dt>cache read</dt><dd>${flight.metrics.cached_input_tokens == null ? '—' : human(flight.metrics.cached_input_tokens)}</dd></div><div><dt>accepted</dt><dd>${flight.metrics.acceptance_met == null ? '—' : flight.metrics.acceptance_met ? 'yes' : 'no'}</dd></div></dl></button>`;
    }).join('')}</div><p class="comparison-verdict">${escapeHtml(verdict)}</p></section>`;
  }

  function barLevel(value, maximum) {
    return Math.max(1, Math.min(10, Math.ceil((Number(value || 0) / maximum) * 10)));
  }

  async function launchFlight(event) {
    event.preventDefault();
    const button = $('.launch-button');
    button.disabled = true;
    button.querySelector('span').textContent = 'Starting flight…';
    try {
      state.promptDraft = $('#flight-prompt').value;
      state.flightSettings = {
        context: $('#flight-context').value,
        provider: $('#flight-provider').value,
        budget: Number($('#flight-budget').value),
        acceptance: $('#flight-acceptance').value,
        reasoning: state.flightSettings.reasoning,
      };
      const flight = await postFlight({
        prompt: state.promptDraft,
        provider: state.flightSettings.provider,
        context_mode: state.flightSettings.context,
        budget: state.flightSettings.budget,
        acceptance_marker: state.flightSettings.acceptance,
        reasoning_profile: state.flightSettings.reasoning,
      });
      state.flightId = flight.id;
      state.flightCursor = 0;
      state.flightPlaying = true;
      state.flightDirection = 1;
      await load(true);
      renderFlightStage();
      button.disabled = false;
      button.querySelector('span').textContent = 'Run one flight';
      const profile = reasoningProfiles[state.flightSettings.reasoning];
      toast(`${profile.title} flight started · requested ${profile.effort}`);
    } catch (error) {
      toast(error.message, true);
      button.disabled = false;
      button.querySelector('span').textContent = 'Run one flight';
    }
  }

  async function postFlight(payload) {
    const response = await fetch('/api/flights', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(payload),
    });
    const flight = await response.json();
    if (!response.ok) throw new Error(flight.error || `HTTP ${response.status}`);
    return flight;
  }

  function renderFlightStage() {
    const stage = $('#flight-stage');
    if (!stage) return renderFlight();
    const flight = currentFlight();
    stage.innerHTML = flight
      ? flightExperience(flight)
      : empty('No prompt flights yet', 'Run one prompt to create the first observable baseline.');
    bindFlightControls(flight);
  }

  function stopFlightPlayback() {
    state.flightPlaying = false;
    clearTimeout(state.flightTimer);
  }

  function bindFlightControls(flight) {
    if (!flight) return;
    $('#flight-play')?.addEventListener('click', () => {
      state.flightPlaying = !state.flightPlaying;
      renderFlightStage();
      scheduleFlightStep();
    });
    $('#flight-start')?.addEventListener('click', () => freezeAt(0));
    $('#flight-end')?.addEventListener('click', () => freezeAt(Math.max(0, flight.events.length - 1)));
    $('#flight-rewind')?.addEventListener('click', () => playDirection(-1));
    $('#flight-forward')?.addEventListener('click', () => playDirection(1));
    $('#flight-scrub')?.addEventListener('input', (event) => freezeAt(Number(event.target.value)));
    $('#flight-speed')?.addEventListener('change', (event) => { state.flightSpeed = Number(event.target.value); scheduleFlightStep(); });
    $$('[data-flight-event]').forEach((button) => button.addEventListener('click', () => freezeAt(Number(button.dataset.flightEvent))));
    $$('[data-burst-event]').forEach((button) => button.addEventListener('click', () => freezeAt(Number(button.dataset.burstEvent))));
    $$('[data-stage]').forEach((button) => button.addEventListener('click', () => {
      const index = flight.events.findIndex((event) => event.stage === button.dataset.stage);
      if (index >= 0) freezeAt(index);
    }));
    $$('[data-compare-flight]').forEach((button) => button.addEventListener('click', () => {
      state.flightId = button.dataset.compareFlight; state.flightCursor = 0; state.flightPlaying = false; renderFlightStage();
    }));
    $$('[data-history-flight]').forEach((button) => button.addEventListener('click', () => {
      state.flightId = button.dataset.historyFlight;
      state.flightCursor = 0;
      state.flightPlaying = false;
      renderFlightStage();
    }));
    $('#inspect-flight-event')?.addEventListener('click', () => select(`flight-event:${flight.id}:${state.flightCursor}`));
    scheduleFlightStep();
  }

  function playDirection(direction) {
    state.flightDirection = direction;
    state.flightPlaying = true;
    renderFlightStage();
    scheduleFlightStep();
  }

  function freezeAt(index) {
    state.flightPlaying = false;
    state.flightCursor = index;
    clearTimeout(state.flightTimer);
    renderFlightStage();
  }

  function scheduleFlightStep() {
    clearTimeout(state.flightTimer);
    if (!state.flightPlaying || state.view !== 'flight') return;
    state.flightTimer = setTimeout(() => {
      const flight = currentFlight();
      if (!flight) return;
      const next = state.flightCursor + state.flightDirection;
      if (next >= 0 && next < flight.events.length) {
        state.flightCursor = next;
        renderFlightStage();
      } else if (state.flightDirection < 0 || !['preparing', 'running'].includes(flight.status)) {
        state.flightPlaying = false;
        renderFlightStage();
      }
    }, 850 / state.flightSpeed);
  }

  function renderTraces() {
    const bundle = state.data.traces || { traces: [], events: [], diagnostics: [] };
    const traces = bundle.traces || [];
    if (!traces.length) {
      $('#main').innerHTML = pageHead(
        'Causal trace lab',
        'Reconstruct complete, incomplete, and summary spans from persisted runtime evidence.'
      ) + empty('No retained control traces', 'Run a traced query, lifecycle hook, vector operation, or provider flight to create causal evidence.');
      return;
    }
    if (!state.traceId || !traces.some((trace) => trace.trace_id === state.traceId)) {
      state.traceId = traces.at(-1).trace_id;
      state.traceSpanId = null;
    }
    const trace = traces.find((value) => value.trace_id === state.traceId);
    const critical = new Set(trace.critical_path_span_ids || []);
    const spans = trace.spans || [];
    if (!state.traceSpanId || !spans.some((span) => span.span_id === state.traceSpanId)) {
      state.traceSpanId = (trace.critical_path_span_ids || [])[0] || spans[0]?.span_id || null;
    }
    const selected = spans.find((span) => span.span_id === state.traceSpanId) || spans[0];
    const positions = new Map(spans.map((span) => [span.span_id, span]));
    const depthOf = (span) => {
      let depth = 0;
      let parent = span.parent_span_id;
      const seen = new Set([span.span_id]);
      while (parent && positions.has(parent) && !seen.has(parent)) {
        seen.add(parent); depth += 1; parent = positions.get(parent).parent_span_id;
      }
      return depth;
    };
    const ordered = [...spans].sort((left, right) => {
      const leftCursor = Math.min(...left.event_cursors);
      const rightCursor = Math.min(...right.event_cursors);
      return leftCursor - rightCursor || left.span_id.localeCompare(right.span_id);
    });
    const maxDuration = Math.max(1, ...spans.map((span) => span.duration_micros || 0));
    const traceEvents = (bundle.events || []).filter((event) => event.trace_id === trace.trace_id);
    const selectedEvents = selected
      ? traceEvents.filter((event) => event.span_id === selected.span_id)
      : [];
    const complete = spans.filter((span) => ['complete', 'summary'].includes(span.status)).length;
    const domainCount = new Set(spans.map((span) => span.domain)).size;
    $('#main').innerHTML = pageHead(
      'Causal trace lab',
      'Measured parent/child lifecycles reconstructed from the project store. Missing finishes and retained-window boundaries stay visible.',
      `<a class="secondary-button" href="/api/runtime/traces?limit=4096&classes=control" target="_blank" rel="noopener">Export control JSON</a>`
    ) + `
      <section class="trace-lab">
        <div class="trace-summary-strip">
          <div><span>TRACE STATE</span><strong class="trace-state ${escapeHtml(trace.status)}">${escapeHtml(trace.status)}</strong></div>
          <div><span>LIFECYCLES</span><strong>${complete}/${spans.length} closed</strong></div>
          <div><span>DOMAINS</span><strong>${domainCount} observed</strong></div>
          <div><span>MEASURED ROOT</span><strong>${trace.critical_path_duration_micros == null ? 'open' : `${human(trace.critical_path_duration_micros)} µs`}</strong></div>
        </div>
        <div class="trace-history" aria-label="Retained traces">
          ${traces.slice(-24).reverse().map((value) => {
            const first = value.spans[0];
            return `<button type="button" data-trace-id="${escapeHtml(value.trace_id)}" class="${value.trace_id === trace.trace_id ? 'active' : ''}"><i class="trace-state ${escapeHtml(value.status)}"></i><span>${escapeHtml(first?.name || 'trace')}</span><code>${escapeHtml(value.trace_id.slice(0, 8))}</code><b>${value.spans.length}</b></button>`;
          }).join('')}
        </div>
        <section class="causal-stage">
          <header><div><span class="eyebrow">CAUSAL TOPOLOGY</span><h2>${escapeHtml(trace.trace_id)}</h2></div><span>${escapeHtml(bundle.critical_path_basis || '')}</span></header>
          <div class="critical-route"><label>MEASURED CRITICAL CANDIDATE</label>${(trace.critical_path_span_ids || []).map((id, index) => {
            const span = positions.get(id);
            return `${index ? '<i>→</i>' : ''}<button type="button" data-trace-span="${escapeHtml(id)}">${escapeHtml(span?.name || id)}</button>`;
          }).join('') || '<span>Unavailable until a measured root is retained.</span>'}</div>
          <div class="causal-lanes">
            ${ordered.map((span) => {
              const depth = depthOf(span);
              const width = span.duration_micros == null ? 12 : Math.max(12, Math.round((span.duration_micros / maxDuration) * 100));
              return `<button type="button" data-trace-span="${escapeHtml(span.span_id)}" class="causal-span ${span.span_id === selected?.span_id ? 'active' : ''} ${critical.has(span.span_id) ? 'critical' : ''}" style="--trace-depth:${depth};--trace-width:${width}%">
                <i></i><span><small>${escapeHtml(span.domain)} · ${escapeHtml(span.status)}</small><strong>${escapeHtml(span.name)}</strong><code>${escapeHtml(span.span_id)}</code></span><b>${span.duration_micros == null ? 'open' : `${human(span.duration_micros)} µs`}</b>
              </button>`;
            }).join('')}
          </div>
        </section>
        ${selected ? `<article class="trace-inspection">
          <header><div><span class="eyebrow">FROZEN SPAN</span><h2>${escapeHtml(selected.name)}</h2></div><span class="badge ${selected.status === 'incomplete' || selected.status === 'invalid' ? 'error' : 'ready'}">${escapeHtml(selected.status)}</span></header>
          <div class="event-data-strip"><div><span>domain</span><strong>${escapeHtml(selected.domain)}</strong></div><div><span>outcome</span><strong>${escapeHtml(selected.outcome)}</strong></div><div><span>data class</span><strong>${escapeHtml(selected.data_class)}</strong></div><div><span>children</span><strong>${selected.child_span_ids.length}</strong></div></div>
          <div class="trace-event-list">${selectedEvents.map((event) => `<button type="button" data-trace-cursor="${event.cursor}"><span>cursor ${event.cursor}</span><strong>${escapeHtml(event.phase)}</strong><b>${escapeHtml(event.outcome)}</b><code>${escapeHtml(event.change_digest.slice(0, 12))}…</code></button>`).join('')}</div>
          ${selected.diagnostics.length ? `<ul>${selected.diagnostics.map((value) => `<li>${escapeHtml(value)}</li>`).join('')}</ul>` : ''}
        </article>` : ''}
        ${(trace.diagnostics.length || bundle.diagnostics.length) ? `<section class="trace-diagnostics"><strong>Integrity diagnostics</strong>${[...trace.diagnostics, ...bundle.diagnostics].map((value) => `<p>${escapeHtml(value)}</p>`).join('')}</section>` : ''}
      </section>`;
    $$('[data-trace-id]').forEach((button) => button.addEventListener('click', () => {
      state.traceId = button.dataset.traceId; state.traceSpanId = null; renderTraces();
    }));
    $$('[data-trace-span]').forEach((button) => button.addEventListener('click', () => {
      state.traceSpanId = button.dataset.traceSpan; renderTraces();
    }));
    $$('[data-trace-cursor]').forEach((button) => button.addEventListener('click', () => select(`runtime-change:${button.dataset.traceCursor}`)));
  }

  function renderStream() {
    const events = state.data.temporal_events || [];
    if (!events.length) {
      $('#main').innerHTML = pageHead('Temporal evidence stream', 'Freeze and inspect persisted runtime mutations across every scope.')
        + empty('No runtime mutations', 'The authoritative changefeed has not committed an event yet.');
      return;
    }
    let index = state.streamCursor == null
      ? events.length - 1
      : events.findIndex((event) => event.cursor === state.streamCursor);
    if (index < 0) index = Math.max(0, events.length - 1);
    const current = events[index];
    state.streamCursor = current.cursor;
    const windowSize = window.matchMedia('(max-width: 760px)').matches ? 72 : 160;
    const windowStart = Math.max(0, Math.min(index - Math.floor(windowSize / 2), events.length - windowSize));
    const visualEvents = events.slice(windowStart, windowStart + windowSize);
    const lanes = [
      ['reasoning', 'REASONING CONTRACT'],
      ['routing', 'CONTEXT + ROUTING'],
      ['workflow', 'WORKFLOW POLICY'],
      ['model', 'MODEL + FLIGHTS'],
      ['search', 'VECTOR + SEARCH'],
      ['storage', 'STORAGE + DATA'],
    ];
    $('#main').innerHTML = pageHead(
      'Temporal evidence stream',
      'One global cursor across every runtime scope. Each mark is a persisted mutation with its commit, digest, and audit envelope attached.',
      `<span class="badge ready">head ${human(state.data.health.runtime_cursor)}</span>`
    ) + `
      <section class="stream-shell">
        <div class="stream-controls" aria-label="Temporal playback">
          <button type="button" id="stream-start" class="transport-button" title="First mutation">⏮ First</button>
          <button type="button" id="stream-rewind" class="transport-button" title="Rewind mutations">◀ Rewind</button>
          <button type="button" id="stream-play" class="transport-button primary" title="Play or freeze stream">${state.streamPlaying ? '❚❚ Freeze time' : '▶ Resume time'}</button>
          <button type="button" id="stream-forward" class="transport-button" title="Fast-forward mutations">Forward ▶</button>
          <button type="button" id="stream-end" class="transport-button" title="Latest mutation">Latest ⏭</button>
          <label class="scrubber"><span>cursor ${current.cursor}</span><input id="stream-scrub" type="range" min="0" max="${events.length - 1}" value="${index}"><b>${events.length} persisted mutations</b></label>
          <select id="stream-speed" aria-label="Temporal playback speed"><option value="0.5" ${state.streamSpeed === .5 ? 'selected' : ''}>0.5×</option><option value="1" ${state.streamSpeed === 1 ? 'selected' : ''}>1×</option><option value="2" ${state.streamSpeed === 2 ? 'selected' : ''}>2×</option><option value="4" ${state.streamSpeed === 4 ? 'selected' : ''}>4×</option><option value="8" ${state.streamSpeed === 8 ? 'selected' : ''}>8×</option></select>
        </div>
        <div class="stream-river" aria-label="Persisted runtime mutations by evidence family">
          <div class="stream-axis"><span>cursor ${visualEvents[0].cursor}</span><b>GLOBAL AUTHORITATIVE CHANGEFEED · ${windowStart + 1}–${windowStart + visualEvents.length} OF ${events.length}</b><span>cursor ${visualEvents.at(-1).cursor}</span></div>
          ${lanes.map(([lane, label]) => `<div class="stream-lane"><label>${label}</label><div class="stream-track">${visualEvents.map((event, visibleIndex) => streamLane(event.family) === lane ? `<button type="button" data-stream-event="${windowStart + visibleIndex}" class="stream-packet family-${escapeHtml(lane)} ${event.cursor === current.cursor ? 'active' : ''}" aria-label="cursor ${event.cursor}: ${escapeHtml(event.label)}"><i></i></button>` : '<span></span>').join('')}</div></div>`).join('')}
        </div>
        ${streamMicroEvent(current, index, events.length)}
        <div class="stream-filmstrip">${events.map((event, eventIndex) => `<button type="button" data-stream-frame="${eventIndex}" class="stream-frame ${event.cursor === current.cursor ? 'active' : ''}"><span>${event.cursor}</span><i class="family-${escapeHtml(streamLane(event.family))}"></i><b>${escapeHtml(event.label)}</b><small>${escapeHtml(event.scope)}</small></button>`).join('')}</div>
      </section>`;
    bindStreamControls(events, index);
  }

  function streamLane(family) {
    if (['memory', 'data', 'storage'].includes(family)) return 'storage';
    return family;
  }

  function streamMicroEvent(event, index, total) {
    const audit = event.audit;
    return `<article class="stream-event-detail">
      <header><div><span class="eyebrow">FROZEN MUTATION · ${escapeHtml(event.family)}</span><h2>${escapeHtml(event.label)}</h2></div><div class="event-clock"><strong>cursor ${event.cursor}</strong><span>commit mutation ${event.commit_ordinal + 1}</span></div></header>
      <div class="event-data-strip"><div><span>scope</span><strong>${escapeHtml(event.scope)}</strong></div><div><span>action</span><strong>${escapeHtml(event.action)}</strong></div><div><span>timeline</span><strong>${index + 1}/${total}</strong></div><div><span>audit</span><strong>${audit ? audit.decision : 'missing'}</strong></div></div>
      <p>${escapeHtml(event.detail)}</p>
      <dl class="payload-breakdown"><div><dt>actor</dt><dd>${escapeHtml(event.actor)}</dd></div><div><dt>commit</dt><dd>${escapeHtml(event.commit_id)}</dd></div><div><dt>change digest</dt><dd>${escapeHtml(event.digest)}</dd></div><div><dt>audit digest</dt><dd>${escapeHtml(audit?.digest || 'not found')}</dd></div></dl>
      <footer><span>${new Date(event.at).toLocaleString()}</span><span>${audit ? 'hash-chained audit attached' : 'audit unavailable'}</span><button type="button" id="inspect-stream-event">Inspect mutation + audit</button></footer>
    </article>`;
  }

  function bindStreamControls(events, index) {
    $('#stream-start')?.addEventListener('click', () => freezeStreamAt(events, 0));
    $('#stream-end')?.addEventListener('click', () => freezeStreamAt(events, events.length - 1));
    $('#stream-rewind')?.addEventListener('click', () => playStream(-1));
    $('#stream-forward')?.addEventListener('click', () => playStream(1));
    $('#stream-play')?.addEventListener('click', () => {
      state.streamPlaying = !state.streamPlaying;
      renderStream();
      scheduleStreamStep();
    });
    $('#stream-scrub')?.addEventListener('input', (event) => freezeStreamAt(events, Number(event.target.value)));
    $('#stream-speed')?.addEventListener('change', (event) => { state.streamSpeed = Number(event.target.value); scheduleStreamStep(); });
    $$('[data-stream-event], [data-stream-frame]').forEach((button) => button.addEventListener('click', () => {
      const target = Number(button.dataset.streamEvent ?? button.dataset.streamFrame);
      freezeStreamAt(events, target);
    }));
    $('#inspect-stream-event')?.addEventListener('click', () => select(`runtime-change:${events[index].cursor}`));
    scheduleStreamStep();
  }

  function freezeStreamAt(events, index) {
    state.streamPlaying = false;
    state.streamCursor = events[Math.max(0, Math.min(events.length - 1, index))].cursor;
    clearTimeout(state.streamTimer);
    renderStream();
  }

  function playStream(direction) {
    state.streamDirection = direction;
    state.streamPlaying = true;
    renderStream();
    scheduleStreamStep();
  }

  function scheduleStreamStep() {
    clearTimeout(state.streamTimer);
    if (!state.streamPlaying || state.view !== 'stream') return;
    state.streamTimer = setTimeout(() => {
      const events = state.data.temporal_events || [];
      const current = events.findIndex((event) => event.cursor === state.streamCursor);
      const next = current + state.streamDirection;
      if (next >= 0 && next < events.length) {
        state.streamCursor = events[next].cursor;
        renderStream();
      } else {
        state.streamPlaying = false;
        renderStream();
      }
    }, 850 / state.streamSpeed);
  }

  function renderSchema() {
    const schema = state.data.schema;
    if (!schema) {
      $('#main').innerHTML = pageHead('Runtime schema', 'The persisted contract governing records, relations, properties, and events for this instance.')
        + empty('Schema migration pending', 'This legacy scope will install its versioned registry atomically with its next reasoning or prompt-flight write.');
      return;
    }
    const records = Object.entries(schema.records || {});
    const relations = Object.entries(schema.relations || {});
    const events = Object.entries(schema.events || {});
    const total = records.length + relations.length + events.length;
    $('#main').innerHTML = pageHead(
      'Runtime schema',
      'The enforceable object contract behind the graph. Unknown types, wrong properties, illegal endpoints, uniqueness collisions, and cardinality violations are denied before commit.',
      `<span class="badge ready">revision ${human(schema.revision)}</span>`
    ) + `
      <section class="schema-contract-line" aria-label="Schema enforcement sequence">
        <div><span>01</span><strong>Registry</strong><small>revision ${human(schema.revision)}</small></div><i>→</i>
        <div><span>02</span><strong>Validate</strong><small>${human(total)} governed types</small></div><i>→</i>
        <div><span>03</span><strong>Deny or commit</strong><small>atomic with runtime data</small></div><i>→</i>
        <div><span>04</span><strong>Hash-chain</strong><small>migration remains replayable</small></div>
      </section>
      <section class="schema-revision"><span class="eyebrow">CURRENT MIGRATION</span><strong>${escapeHtml(schema.migration)}</strong><small>Every later registry must advance exactly one revision.</small></section>
      <section class="schema-groups">
        ${schemaGroup('Record types', 'Persistent graph objects with required, optional, and unique properties.', records, 'record')}
        ${schemaGroup('Event types', 'Immutable lifecycle facts with governed subject types and payloads.', events, 'event')}
        ${schemaGroup('Relation types', 'Directed edges with legal endpoints and temporal cardinality.', relations, 'relation')}
      </section>`;
  }

  function schemaGroup(title, detail, entries, category) {
    return `<article class="schema-group"><header><div><h2>${escapeHtml(title)}</h2><p>${escapeHtml(detail)}</p></div><span>${entries.length}</span></header><div class="schema-type-list">${entries.length ? entries.map(([kind, definition]) => schemaType(kind, definition, category)).join('') : '<div class="schema-none">No types registered in this category.</div>'}</div></article>`;
  }

  function schemaType(kind, definition, category) {
    const properties = Object.entries(definition.properties || {});
    const unique = new Set(definition.unique_properties || []);
    let relationship = '';
    if (category === 'event') {
      const subjects = definition.subject_types || [];
      relationship = `<div class="schema-endpoints"><span>subject</span><b>${definition.subject_required ? 'required' : 'optional'}</b><em>${subjects.length ? subjects.join(' · ') : 'any registered type'}</em></div>`;
    }
    if (category === 'relation') {
      relationship = `<div class="schema-relation-path"><b>${escapeHtml((definition.from || []).join(' | '))}</b><span>— ${escapeHtml(kind)} →</span><b>${escapeHtml((definition.to || []).join(' | '))}</b></div><div class="schema-limits"><span>pair ${definition.unique_pair ? 'unique' : 'repeatable'}</span><span>out ${definition.max_outgoing ?? '∞'}</span><span>in ${definition.max_incoming ?? '∞'}</span></div>`;
    }
    return `<details class="schema-type" ${properties.length <= 5 ? 'open' : ''}><summary><span class="schema-kind ${category}"></span><strong>${escapeHtml(kind.replaceAll('_', ' '))}</strong><small>${properties.length} properties</small></summary>${relationship}<div class="schema-properties">${properties.length ? properties.map(([name, rule]) => `<div><code>${escapeHtml(name)}</code><span>${escapeHtml(rule.value_type)}</span><b>${rule.required ? 'required' : 'optional'}${unique.has(name) ? ' · unique' : ''}</b></div>`).join('') : '<div><code>no properties</code><span>closed</span><b>additional denied</b></div>'}</div><footer>${definition.allow_additional_properties ? 'Additional properties allowed' : 'Undeclared properties denied'}</footer></details>`;
  }

  function renderQuery() {
    const firstType = Object.keys(state.data.schema?.records || {})[0] || 'reasoning_run';
    const sample = `FROM record:${firstType} AT VALID ${Date.now()} KNOWN HEAD PROJECT * LIMIT 25 EXPLAIN CONTRACT`;
    $('#main').innerHTML = pageHead('vyrmQL contract lab', 'Run an explicit bi-temporal query and inspect the binding, chosen physical path, rejected alternatives, budgets, and exact result.') + `
      <form id="query-form" class="query-composer">
        <textarea id="query-source" spellcheck="false" aria-label="vyrmQL query">${escapeHtml(sample)}</textarea>
        <div><span>Read-only · scope instance:default · exact path required</span><button class="primary-button">Plan and execute</button></div>
      </form>
      <div id="query-result">${empty('Ready to inspect', 'The planner will expose its evidence contract before showing deterministic batches.')}</div>`;
    $('#query-form').addEventListener('submit', async (event) => {
      event.preventDefault();
      const target = $('#query-result');
      target.innerHTML = empty('Capturing read stamp…', 'Parsing, binding, planning, then executing against one immutable manifest.');
      try {
        const response = await fetch(`/api/runtime/query?ql=${encodeURIComponent($('#query-source').value)}`);
        const value = await response.json();
        if (!response.ok) throw new Error(value.error || `HTTP ${response.status}`);
        const contract = value.plan.explanation.contract;
        const rows = value.execution.batches.flatMap((batch) => batch.rows);
        target.innerHTML = `
          <section class="query-contract-grid">
            <article><span>READ MANIFEST</span><strong>${escapeHtml(contract.read_manifest.slice(0, 14))}…</strong><small>cursor ${human(contract.known_at_cursor)} · schema r${human(contract.schema_revision)}</small></article>
            <article><span>SEMANTICS</span><strong>${contract.exact ? 'Exact' : 'Approximate'}</strong><small>valid ${human(contract.valid_at)} · ${escapeHtml(contract.deterministic_order)}</small></article>
            <article><span>EXECUTION</span><strong>${human(value.execution.returned_rows)} rows</strong><small>${human(value.execution.scanned_changes)} result positions · ${human(value.execution.output_bytes)} bytes</small></article>
            <article><span>READ PROOF</span><strong>${escapeHtml(value.execution.stamp_validation.replaceAll('_', ' '))}</strong><small>${human(value.execution.stamp_validation_max_changes)} change reads · ${human(value.execution.stamp_validation_proof_nodes)} proof nodes</small></article>
            <article><span>BOUNDARY</span><strong>${escapeHtml(contract.scope)}</strong><small>network ${contract.network_required ? 'yes' : 'no'} · GPU ${contract.gpu_required ? 'yes' : 'no'}</small></article>
          </section>
          <section class="query-paths">${value.plan.explanation.candidates.map((candidate) => `<article class="${candidate.selected ? 'selected' : 'rejected'}"><span>${candidate.selected ? 'SELECTED' : 'REJECTED'}</span><strong>${escapeHtml(candidate.name.replaceAll('_', ' '))}</strong><p>${escapeHtml(candidate.reason)}</p></article>`).join('')}</section>
          <details class="query-plan" open><summary>Canonical query and operator pipeline</summary><code>${escapeHtml(value.canonical)}</code><pre>${escapeHtml(JSON.stringify(value.plan.operators, null, 2))}</pre></details>
          <section class="query-rows"><header><strong>Deterministic result</strong><span>${value.execution.truncated ? 'truncated by declared budget' : 'complete within budget'}</span></header>${rows.length ? rows.map((row) => `<article><code>${escapeHtml(row.identity)}</code><pre>${escapeHtml(JSON.stringify(row.values, null, 2))}</pre></article>`).join('') : empty('No rows at this time', 'The query executed successfully but no identity satisfied its temporal and filter contract.')}</section>`;
      } catch (error) {
        target.innerHTML = empty('Query denied', error.message);
      }
    });
  }

  function renderCluster() {
    const history = state.data.cluster || { baseline_samples: [], samples: [], nodes: [], alerts: [], total_samples: 0 };
    const samples = history.samples || [];
    if (!samples.length) {
      clearTimeout(state.clusterTimer);
      state.clusterPlaying = false;
      $('#main').innerHTML = pageHead('Cluster flight recorder', 'Retained project-node observations with exact cluster, shard, process-reset, runtime cursor, and audit coordinates.')
        + empty('No retained node observations', 'Import a validated control-v4 node status through POST /api/cluster/samples. Live process counters are never presented as history until that observation is hash-linked and committed.');
      return;
    }
    let index = state.clusterCursor == null
      ? samples.length - 1
      : samples.findIndex((sample) => sample.digest === state.clusterCursor);
    if (index < 0) index = samples.length - 1;
    const sample = samples[index];
    state.clusterCursor = sample.digest;
    const visibleNodes = clusterNodesAt(history.baseline_samples || [], samples, index);
    const delta = sample.delta || {};
    const alertTone = sample.alerts.some((alert) => alert.severity === 'critical') ? 'error'
      : sample.alerts.some((alert) => alert.severity === 'warning') ? 'attention' : 'ready';
    const operationTotal = Number(delta.transport_allowed || 0) + Number(delta.transport_denied || 0) + Number(delta.transport_failed || 0);
    $('#main').innerHTML = pageHead(
      'Cluster flight recorder',
      'Freeze, rewind, and compare retained node observations. Every value resolves to a validated status digest, runtime cursor, commit, and hash-chained audit.',
      `<span class="badge ${alertTone}">${sample.process_reset ? 'process reset' : `${sample.alerts.length} alert(s)`}</span>`,
    ) + `
      <section class="cluster-summary-strip">
        <div><span>NODES AT CURSOR</span><strong>${human(visibleNodes.length)}</strong><small>${escapeHtml(sample.status.cluster)} · shard ${human(sample.status.shard)}</small></div>
        <div><span>RPC OUTCOMES Δ</span><strong>${human(operationTotal)}</strong><small>${human(delta.transport_denied || 0)} denied · ${human(delta.transport_failed || 0)} failed</small></div>
        <div><span>ARTIFACTS Δ</span><strong>${human(delta.artifact_completed || 0)}</strong><small>${formatBytes(delta.artifact_gc_reclaimed_bytes || 0)} reclaimed</small></div>
        <div><span>TRACE COMMITS Δ</span><strong>${human(delta.trace_commit_acknowledgements || 0)}</strong><small>${human(delta.trace_cursor_conflicts || 0)} cursor conflicts</small></div>
      </section>
      <section class="cluster-shell">
        <div class="cluster-controls" aria-label="Cluster history playback">
          <button type="button" id="cluster-start" class="transport-button" title="First cluster sample">⏮ First</button>
          <button type="button" id="cluster-rewind" class="transport-button" title="Rewind cluster samples">◀ Rewind</button>
          <button type="button" id="cluster-play" class="play-button" title="Play or freeze cluster history">${state.clusterPlaying ? 'Freeze time' : 'Resume time'}</button>
          <button type="button" id="cluster-forward" class="transport-button" title="Fast-forward cluster samples">Forward ▶</button>
          <button type="button" id="cluster-end" class="transport-button" title="Latest cluster sample">Latest ⏭</button>
          <input id="cluster-scrub" type="range" min="0" max="${samples.length - 1}" value="${index}" aria-label="Cluster history cursor">
          <select id="cluster-speed" aria-label="Cluster playback speed"><option value="1" ${state.clusterSpeed === 1 ? 'selected' : ''}>1×</option><option value="2" ${state.clusterSpeed === 2 ? 'selected' : ''}>2×</option><option value="4" ${state.clusterSpeed === 4 ? 'selected' : ''}>4×</option><option value="8" ${state.clusterSpeed === 8 ? 'selected' : ''}>8×</option></select>
          <span class="cluster-clock">sample ${index + 1}/${samples.length} · runtime cursor ${human(sample.cursor)}</span>
        </div>
        <div class="cluster-topology" role="group" aria-label="Cluster topology at selected sample">
          <header><span>TOPOLOGY AT CURSOR ${human(sample.cursor)}</span><b>${escapeHtml(new Date(sample.status.telemetry.observed_at).toLocaleString())}</b></header>
          <div class="cluster-node-grid">${visibleNodes.map((node) => clusterNode(node, sample.digest)).join('')}</div>
        </div>
        <article class="cluster-event-detail ${alertTone}">
          <header><div><span class="eyebrow">FROZEN NODE OBSERVATION · SEQUENCE ${human(sample.sequence)}</span><h2>${escapeHtml(sample.status.canonical_node_id)}</h2></div><div class="event-clock"><strong>${escapeHtml(sample.status.state)}</strong><span>term ${human(sample.status.current_term)} · leader ${sample.status.current_leader ?? 'none'}</span></div></header>
          <div class="event-data-strip"><div><span>applied / log</span><strong>${human(sample.status.last_applied_index ?? 0)} / ${human(sample.status.last_log_index ?? 0)}</strong></div><div><span>request bytes Δ</span><strong>${formatBytes(delta.transport_request_bytes || 0)}</strong></div><div><span>response bytes Δ</span><strong>${formatBytes(delta.transport_response_bytes || 0)}</strong></div><div><span>work duration Δ</span><strong>${formatMicros(delta.transport_duration_micros || 0)}</strong></div></div>
          <div class="cluster-alert-list">${sample.alerts.length ? sample.alerts.map((alert) => `<div class="cluster-alert ${escapeHtml(alert.severity)}"><b>${escapeHtml(alert.code.replaceAll('_', ' '))}</b><span>${human(alert.value)}</span><p>${escapeHtml(alert.detail)}</p></div>`).join('') : '<div class="cluster-clear">No derived alert in this observation interval.</div>'}</div>
          <footer><span>status ${escapeHtml(sample.source_status_digest.slice(0, 14))}…</span><span>previous ${sample.previous_sample_digest ? `${escapeHtml(sample.previous_sample_digest.slice(0, 14))}…` : 'chain origin'}</span><button type="button" id="inspect-cluster-sample">Inspect raw status + audit</button></footer>
        </article>
        <div class="cluster-filmstrip" aria-label="Retained cluster observation timeline">${samples.map((item, sampleIndex) => `<button type="button" class="cluster-frame ${sampleIndex === index ? 'active' : ''} ${item.process_reset ? 'reset' : ''}" data-cluster-sample="${sampleIndex}"><span>#${human(item.sequence)}</span><i class="${clusterSampleTone(item)}"></i><b>${escapeHtml(item.status.canonical_node_id)}</b><small>c${human(item.cursor)} · ${item.alerts.length} alert(s)</small></button>`).join('')}</div>
      </section>`;
    bindClusterControls(samples, index);
  }

  function clusterNodesAt(baseline, samples, index) {
    const latest = new Map(baseline.map((sample) => [sample.node_key, sample]));
    samples.slice(0, index + 1).forEach((sample) => latest.set(sample.node_key, sample));
    return [...latest.values()].sort((left, right) => left.status.shard - right.status.shard || left.status.raft_node_id - right.status.raft_node_id);
  }

  function clusterNode(sample, selectedDigest) {
    const status = sample.status;
    const lag = Math.max(0, Number(status.last_log_index || 0) - Number(status.last_applied_index || 0));
    const leader = status.current_leader === status.raft_node_id;
    return `<button type="button" class="cluster-node ${leader ? 'leader' : ''} ${sample.digest === selectedDigest ? 'selected' : ''}" data-cluster-digest="${escapeHtml(sample.digest)}"><span class="cluster-node-orbit ${sample.alerts.some((alert) => alert.severity === 'critical' || alert.severity === 'warning') ? 'attention' : ''}"></span><strong>${escapeHtml(status.canonical_node_id)}</strong><code>raft ${human(status.raft_node_id)} · shard ${human(status.shard)}</code><small>${leader ? 'LEADER' : `leader → ${status.current_leader ?? 'none'}`} · lag ${human(lag)}</small></button>`;
  }

  function clusterSampleTone(sample) {
    if (sample.alerts.some((alert) => alert.severity === 'critical')) return 'critical';
    if (sample.alerts.some((alert) => alert.severity === 'warning')) return 'warning';
    return sample.process_reset ? 'reset' : 'healthy';
  }

  function bindClusterControls(samples, index) {
    $('#cluster-start')?.addEventListener('click', () => freezeClusterAt(samples, 0));
    $('#cluster-end')?.addEventListener('click', () => freezeClusterAt(samples, samples.length - 1));
    $('#cluster-rewind')?.addEventListener('click', () => playCluster(-1));
    $('#cluster-forward')?.addEventListener('click', () => playCluster(1));
    $('#cluster-play')?.addEventListener('click', () => {
      state.clusterPlaying = !state.clusterPlaying;
      renderCluster();
      scheduleClusterStep();
    });
    $('#cluster-scrub')?.addEventListener('input', (event) => freezeClusterAt(samples, Number(event.target.value)));
    $('#cluster-speed')?.addEventListener('change', (event) => { state.clusterSpeed = Number(event.target.value); scheduleClusterStep(); });
    $$('[data-cluster-sample]').forEach((button) => button.addEventListener('click', () => freezeClusterAt(samples, Number(button.dataset.clusterSample))));
    $$('[data-cluster-digest]').forEach((button) => button.addEventListener('click', () => {
      const target = samples.findIndex((sample) => sample.digest === button.dataset.clusterDigest);
      if (target >= 0) freezeClusterAt(samples, target);
      else select(`cluster-sample:${button.dataset.clusterDigest}`);
    }));
    $('#inspect-cluster-sample')?.addEventListener('click', () => select(`cluster-sample:${samples[index].digest}`));
    scheduleClusterStep();
  }

  function freezeClusterAt(samples, index) {
    state.clusterPlaying = false;
    state.clusterCursor = samples[Math.max(0, Math.min(samples.length - 1, index))].digest;
    clearTimeout(state.clusterTimer);
    renderCluster();
  }

  function playCluster(direction) {
    state.clusterPlaying = true;
    const samples = state.data.cluster?.samples || [];
    const current = samples.findIndex((sample) => sample.digest === state.clusterCursor);
    if (direction < 0 && current <= 0) state.clusterCursor = samples.at(-1)?.digest || null;
    if (direction > 0 && current >= samples.length - 1) state.clusterCursor = samples[0]?.digest || null;
    state.clusterDirection = direction;
    renderCluster();
    scheduleClusterStep();
  }

  function scheduleClusterStep() {
    clearTimeout(state.clusterTimer);
    if (!state.clusterPlaying || state.view !== 'cluster') return;
    state.clusterTimer = setTimeout(() => {
      const samples = state.data.cluster?.samples || [];
      const current = samples.findIndex((sample) => sample.digest === state.clusterCursor);
      const next = current + (state.clusterDirection || 1);
      if (next >= 0 && next < samples.length) {
        state.clusterCursor = samples[next].digest;
        renderCluster();
      } else {
        state.clusterPlaying = false;
        renderCluster();
      }
    }, 850 / state.clusterSpeed);
  }

  function formatBytes(value) {
    const bytes = Number(value || 0);
    if (bytes < 1024) return `${human(bytes)} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KiB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)} MiB`;
  }

  function formatMicros(value) {
    const micros = Number(value || 0);
    if (micros < 1000) return `${human(micros)} µs`;
    if (micros < 1000000) return `${(micros / 1000).toFixed(1)} ms`;
    return `${(micros / 1000000).toFixed(2)} s`;
  }

  function renderGraph() {
    const kinds = ['subject', 'claim', 'run', 'event', 'evidence', 'file', 'invocation', 'flight', 'flight_event'];
    $('#main').innerHTML = pageHead('Runtime graph', 'Traverse local evidence neighborhoods by default. Switch to global only when orientation matters more than detail.') + `
      <div class="toolbar"><div class="segmented"><button data-scope="local" class="${state.graphScope === 'local' ? 'active' : ''}">Local</button><button data-scope="global" class="${state.graphScope === 'global' ? 'active' : ''}">Global</button></div>${kinds.map((kind) => `<label class="filter-chip"><input type="checkbox" data-kind="${kind}" ${state.graphKinds.has(kind) ? 'checked' : ''}><span class="dot" style="background:${color(kind)}"></span>${kind}</label>`).join('')}</div>
      <div id="graph-shell" class="graph-shell"><svg id="graph" role="img" aria-label="Runtime object graph"></svg><div class="graph-legend"><span>Scroll to zoom</span><span>Drag canvas to pan</span><span>Select for local graph</span></div><div id="graph-tip" class="graph-tip"></div></div>`;
    $$('[data-scope]').forEach((button) => button.addEventListener('click', () => { state.graphScope = button.dataset.scope; renderGraph(); }));
    $$('.filter-chip input').forEach((input) => input.addEventListener('change', () => { input.checked ? state.graphKinds.add(input.dataset.kind) : state.graphKinds.delete(input.dataset.kind); drawGraph(); }));
    drawGraph();
  }

  function graphSubset() {
    const allNodes = state.data.graph.nodes.filter((node) => node.kind === 'instance' || state.graphKinds.has(node.kind));
    const allowed = new Set(allNodes.map((node) => node.id));
    let edges = state.data.graph.edges.filter((edge) => allowed.has(edge.from) && allowed.has(edge.to));
    if (state.graphScope === 'global' || !state.selected || !allowed.has(state.selected)) return { nodes: allNodes.slice(0, 180), edges };
    const local = new Set([state.selected]);
    for (let depth = 0; depth < 3; depth += 1) {
      edges.forEach((edge) => {
        if (local.has(edge.from)) local.add(edge.to);
        if (local.has(edge.to)) local.add(edge.from);
      });
    }
    const nodes = allNodes.filter((node) => local.has(node.id));
    edges = edges.filter((edge) => local.has(edge.from) && local.has(edge.to));
    return { nodes, edges };
  }

  function drawGraph() {
    const svg = $('#graph');
    if (!svg) return;
    const shell = $('#graph-shell');
    const width = shell.clientWidth || 900;
    const height = shell.clientHeight || 600;
    svg.setAttribute('viewBox', `0 0 ${width} ${height}`);
    const graph = graphSubset();
    const positions = layout(graph.nodes, width, height);
    const ns = 'http://www.w3.org/2000/svg';
    svg.replaceChildren();
    const viewport = document.createElementNS(ns, 'g');
    svg.append(viewport);
    const edgeGroup = document.createElementNS(ns, 'g');
    viewport.append(edgeGroup);
    graph.edges.forEach((edge) => {
      const from = positions.get(edge.from), to = positions.get(edge.to);
      if (!from || !to) return;
      const line = document.createElementNS(ns, 'line');
      line.setAttribute('x1', from.x); line.setAttribute('y1', from.y);
      line.setAttribute('x2', to.x); line.setAttribute('y2', to.y);
      line.setAttribute('class', `graph-edge ${state.selected === edge.from || state.selected === edge.to ? 'highlight' : ''}`);
      edgeGroup.append(line);
    });
    const nodeGroup = document.createElementNS(ns, 'g');
    viewport.append(nodeGroup);
    graph.nodes.forEach((node) => {
      const p = positions.get(node.id);
      const group = document.createElementNS(ns, 'g');
      group.setAttribute('class', `graph-node ${state.selected === node.id ? 'selected' : ''}`);
      group.setAttribute('transform', `translate(${p.x} ${p.y})`);
      const circle = document.createElementNS(ns, 'circle');
      circle.setAttribute('r', node.kind === 'instance' ? 13 : node.kind === 'run' ? 10 : 7);
      circle.setAttribute('fill', color(node.kind));
      const text = document.createElementNS(ns, 'text');
      text.setAttribute('x', 11); text.setAttribute('y', 4);
      text.textContent = node.label.length > 24 ? `${node.label.slice(0, 22)}…` : node.label;
      group.append(circle, text);
      group.addEventListener('click', () => { state.selected = node.id; renderInspector(); if (state.graphScope === 'local') drawGraph(); });
      group.addEventListener('mouseenter', (event) => showTip(event, node));
      group.addEventListener('mouseleave', hideTip);
      nodeGroup.append(group);
    });
    enablePanZoom(svg, viewport);
  }

  function layout(nodes, width, height) {
    const positions = new Map();
    const buckets = Object.groupBy ? Object.groupBy(nodes, (node) => node.kind) : nodes.reduce((acc, node) => ((acc[node.kind] ||= []).push(node), acc), {});
    const center = { x: width / 2, y: height / 2 };
    const rings = { instance: 0, run: .20, event: .34, evidence: .47, subject: .45, claim: .58, flight: .24, flight_event: .39, file: .72, invocation: .83 };
    Object.entries(buckets).forEach(([kind, list]) => {
      const radius = Math.min(width, height) * (rings[kind] ?? .65);
      list.forEach((node, index) => {
        const offset = { run: -.8, event: -.35, evidence: .1, subject: 2.7, claim: 2.25, file: .6, invocation: 1.35 }[kind] ?? 0;
        const angle = offset + (Math.PI * 2 * index / Math.max(1, list.length));
        positions.set(node.id, { x: center.x + Math.cos(angle) * radius, y: center.y + Math.sin(angle) * radius });
      });
    });
    return positions;
  }

  function enablePanZoom(svg, viewport) {
    let scale = 1, x = 0, y = 0, dragging = false, last = null;
    const apply = () => viewport.setAttribute('transform', `translate(${x} ${y}) scale(${scale})`);
    svg.addEventListener('wheel', (event) => { event.preventDefault(); scale = Math.max(.35, Math.min(3, scale * (event.deltaY > 0 ? .9 : 1.1))); apply(); }, { passive: false });
    svg.addEventListener('pointerdown', (event) => { if (event.target === svg) { dragging = true; last = event; svg.setPointerCapture(event.pointerId); } });
    svg.addEventListener('pointermove', (event) => { if (!dragging) return; x += event.clientX - last.clientX; y += event.clientY - last.clientY; last = event; apply(); });
    svg.addEventListener('pointerup', () => { dragging = false; });
  }

  function showTip(event, node) {
    const tip = $('#graph-tip');
    tip.innerHTML = `<strong>${escapeHtml(node.label)}</strong><span>${escapeHtml(node.kind)} · ${escapeHtml(node.detail)}</span>`;
    tip.style.display = 'block'; tip.style.left = `${event.offsetX + 14}px`; tip.style.top = `${event.offsetY + 14}px`;
  }
  function hideTip() { const tip = $('#graph-tip'); if (tip) tip.style.display = 'none'; }

  function renderRuns() {
    const runs = [...state.data.runs].reverse();
    $('#main').innerHTML = pageHead('Reasoning runs', 'The externally auditable goal → plan → attempt → observation → decision → verification → outcome contract.') + `<section class="run-list">${runs.map((run) => `<article class="run-card"><button class="run-summary lens-row object-link" data-object="run:${escapeHtml(run.id)}"><div><span class="badge ${run.complete ? 'ready' : 'attention'}">${run.complete ? 'complete' : 'active'}</span><h3>${escapeHtml(run.id)}</h3><p>${escapeHtml(run.state)} · ${run.events.length} recorded transition(s)</p></div></button><div class="run-events">${timeline(run.events)}</div></article>`).join('') || empty('No reasoning runs', 'Runs appear when the typed contract records its first goal.')}</section>`;
    bindObjectLinks();
  }

  function renderClaims() {
    const claims = state.data.claims;
    $('#main').innerHTML = pageHead('Current claims', 'Bi-temporal assertions currently in force, with provenance and complete content identity.') + `<table class="data-table"><thead><tr><th>SUBJECT / PREDICATE</th><th>OBJECT</th><th>PRODUCER</th><th>VALID FROM</th><th>IDENTITY</th></tr></thead><tbody>${claims.map((claim) => `<tr data-object="claim:${claim.id}"><td class="object-cell"><strong>${escapeHtml(claim.subject)}</strong><span class="mono">${escapeHtml(claim.predicate)}</span></td><td><span class="truncate">${escapeHtml(claim.object)}</span></td><td>${escapeHtml(claim.producer.actor)}</td><td class="mono">${claim.valid_from}</td><td><code>${claim.id.slice(0, 10)}…</code></td></tr>`).join('')}</tbody></table>${claims.length ? '' : empty('No current claims', 'The authoritative log has no claims in force at this instant.')}`;
    $$('[data-object]', $('#main')).forEach((row) => row.addEventListener('click', () => select(row.dataset.object)));
  }

  function renderRoutes() {
    $('#main').innerHTML = pageHead('Source routes', 'Query the persisted routing projection. Results are complete files with visible ranking evidence, never detached fragments.') + `<form id="route-form" class="route-field"><input id="route-query" placeholder="Symbol, module, or concept" autocomplete="off"><button class="primary-button">Route source</button></form><div id="route-results" class="route-results">${empty('Enter a source query', `${state.data.health.indexed_files} files and ${state.data.health.indexed_symbols} symbols are indexed.`)}</div>`;
    const global = $('#global-search').value.trim();
    if (global) $('#route-query').value = global;
    $('#route-form').addEventListener('submit', async (event) => { event.preventDefault(); await route($('#route-query').value); });
  }

  async function route(query) {
    if (!query.trim()) return;
    const target = $('#route-results');
    target.innerHTML = empty('Routing…', 'Reading the persisted source projection.');
    try {
      const response = await fetch(`/api/route?query=${encodeURIComponent(query)}&limit=10`);
      const results = await response.json();
      if (!response.ok) throw new Error(results.error || `HTTP ${response.status}`);
      target.innerHTML = results.length ? results.map((item, index) => `<button class="route-result lens-row object-link" data-object="file:${escapeHtml(item.path)}"><span class="route-rank">${index + 1}</span><div><strong>${escapeHtml(item.path)}</strong><p>${escapeHtml(renderJustification(item.justification))}</p></div><div class="route-score">${item.score.toFixed(1)}<br>${item.lines} lines</div></button>`).join('') : empty('No related files', 'The current projection found no definition or reference evidence for this query.');
      bindObjectLinks();
    } catch (error) { target.innerHTML = empty('Route unavailable', error.message); }
  }

  function renderJustification(value) {
    const parts = [];
    if (value.defines?.length) parts.push(`defines ${value.defines.join(', ')}`);
    if (value.reference_lines) parts.push(`${value.reference_lines} reference line(s)`);
    if (value.imports_a_definer) parts.push('imports a defining module');
    return parts.join(' · ') || 'related source evidence';
  }

  function renderActivity() {
    const items = [...state.data.invocations].reverse();
    $('#main').innerHTML = pageHead('Runtime activity', 'Every operator and lifecycle invocation, including failures and recall-effectiveness evidence.') + `<table class="data-table"><thead><tr><th>#</th><th>COMMAND</th><th>TRIGGER</th><th>OUTCOME</th><th>DURATION</th><th>DETAIL</th></tr></thead><tbody>${items.map((item) => `<tr data-object="invocation:${item.ordinal}"><td class="mono">${item.ordinal}</td><td class="object-cell"><strong>${escapeHtml(item.command)}</strong><span class="mono">${new Date(item.at).toLocaleString()}</span></td><td>${escapeHtml(item.trigger)}</td><td><span class="badge ${item.outcome === 'ok' ? 'ready' : 'error'}">${item.outcome}</span></td><td class="mono">${item.duration_ms} ms</td><td><span class="truncate">${escapeHtml(item.detail || '—')}</span></td></tr>`).join('')}</tbody></table>${items.length ? '' : empty('No activity recorded', 'Runtime invocations will appear chronologically here.')}`;
    $$('[data-object]', $('#main')).forEach((row) => row.addEventListener('click', () => select(row.dataset.object)));
  }

  function findObject(id) {
    if (!id || !state.data) return null;
    if (id.startsWith('claim:')) return state.data.claims.find((item) => `claim:${item.id}` === id);
    if (id.startsWith('run:')) return state.data.runs.find((item) => `run:${item.id}` === id);
    if (id.startsWith('event:')) return state.data.runs.flatMap((run) => run.events).find((item) => `event:${item.run_id}:${item.ordinal}` === id);
    if (id.startsWith('file:')) return state.data.files.find((item) => `file:${item.path}` === id) || state.data.graph.nodes.find((item) => item.id === id);
    if (id.startsWith('invocation:')) return state.data.invocations.find((item) => `invocation:${item.ordinal}` === id);
    if (id.startsWith('runtime-change:')) return state.data.temporal_events.find((item) => `runtime-change:${item.cursor}` === id);
    if (id.startsWith('cluster-sample:')) return [...(state.data.cluster?.baseline_samples || []), ...(state.data.cluster?.samples || [])].find((item) => `cluster-sample:${item.digest}` === id) || null;
    if (id.startsWith('flight-event:')) {
      const [, , flightId, ordinal] = id.match(/^(flight-event):(.*):(\d+)$/) || [];
      const flight = state.data.flights.find((item) => item.id === flightId);
      return flight?.events[Number(ordinal)] || null;
    }
    return state.data.graph.nodes.find((item) => item.id === id);
  }

  function renderInspector() {
    const object = findObject(state.selected);
    if (!object) return;
    const graphNode = state.data.graph.nodes.find((node) => node.id === state.selected);
    const title = graphNode?.label || object.label || object.id || object.command || object.path || object.payload?.kind || 'Object';
    $('#inspector-title').textContent = title;
    const properties = inspectorProperties(object, graphNode);
    $('#inspector-body').innerHTML = `<dl class="property-list">${properties.map(([name, value, cls = '']) => `<div class="property"><dt>${escapeHtml(name)}</dt><dd class="${cls}">${escapeHtml(value)}</dd></div>`).join('')}</dl><details><summary class="eyebrow">RAW OBJECT</summary><pre class="json-block">${escapeHtml(JSON.stringify(object, null, 2))}</pre></details>`;
  }

  function inspectorProperties(object, node) {
    if (object.subject) return [['Type', 'claim'], ['Subject', object.subject], ['Predicate', object.predicate], ['Object', object.object], ['Producer', object.producer.actor], ['Valid interval', `${object.valid_from} → ${object.valid_to ?? 'open'}`], ['Transaction time', object.tx_time], ['Digest', object.id, 'digest']];
    if (object.events) return [['Type', 'reasoning run'], ['Run', object.id], ['State', object.state], ['Complete', object.complete], ['Transitions', object.events.length]];
    if (object.payload) return [['Type', 'reasoning event'], ['Transition', object.payload.kind], ['Run', object.run_id], ['Ordinal', object.ordinal], ['Actor', object.actor], ['Digest', object.digest, 'digest'], ['Summary', eventSummary(object)]];
    if (object.command) return [['Type', 'invocation'], ['Command', object.command], ['Ordinal', object.ordinal], ['Trigger', object.trigger], ['Outcome', object.outcome], ['Duration', `${object.duration_ms} ms`], ['Detail', object.detail || '—']];
    if (object.node_key && object.source_status_digest) return [['Type', 'retained cluster observation'], ['Node', object.status.canonical_node_id], ['Cluster / shard', `${object.status.cluster} / ${object.status.shard}`], ['Sample sequence', object.sequence], ['Runtime cursor', object.cursor], ['Process reset', object.process_reset], ['Status digest', object.source_status_digest, 'digest'], ['Sample digest', object.digest, 'digest'], ['Previous sample', object.previous_sample_digest || 'chain origin', 'digest'], ['Commit', object.commit_id], ['Audit digest', object.audit_digest || 'missing', 'digest'], ['Alerts', object.alerts.length]];
    if (object.commit_id && object.cursor != null) return [['Type', 'runtime mutation'], ['Cursor', object.cursor], ['Family', object.family], ['Action', object.action], ['Scope', object.scope], ['Actor', object.actor], ['Commit', object.commit_id], ['Change digest', object.digest, 'digest'], ['Audit digest', object.audit?.digest || 'missing', 'digest'], ['Detail', object.detail]];
    if (object.stage && object.kind) return [['Type', 'prompt flight event'], ['Stage', object.stage], ['Kind', object.kind], ['Ordinal', object.ordinal], ['Elapsed', `${object.elapsed_ms} ms`], ['Label', object.label], ['Detail', object.detail]];
    if (object.path) return [['Type', 'indexed file'], ['Path', object.path], ['Language', object.language], ['Lines', object.lines], ['Symbols', object.symbols], ['Terms', object.terms]];
    return [['Type', node?.kind || object.kind || 'object'], ['Identity', node?.id || object.id], ['State', node?.state || object.state || '—'], ['Detail', node?.detail || object.detail || '—']];
  }

  function select(id) {
    state.selected = id;
    renderInspector();
    document.body.classList.remove('inspector-hidden');
    $('#inspector').classList.remove('closed');
    $('.app-shell').classList.remove('inspector-closed');
  }

  function bindObjectLinks() {
    $$('.object-link').forEach((element) => element.addEventListener('click', () => select(element.dataset.object)));
  }

  function navigate(view, kind = null) {
    state.view = view;
    if (location.hash !== `#${view}`) history.pushState(null, '', `#${view}`);
    state.graphFocusKind = kind;
    if (kind) state.graphKinds = new Set(['instance', kind, ...(kind === 'run' ? ['event'] : kind === 'claim' ? ['subject'] : [])]);
    render();
    $('#main').focus();
  }

  function renderError(message) {
    $('#main').innerHTML = pageHead('Runtime unavailable', 'The workbench could not read this local instance.') + empty('Connection failed', message);
  }

  function toast(message, error = false) {
    const element = $('#toast');
    element.textContent = message;
    element.style.borderColor = error ? '#693d38' : '';
    element.classList.add('show');
    clearTimeout(toast.timer);
    toast.timer = setTimeout(() => element.classList.remove('show'), 2200);
  }

  $$('.nav-item').forEach((button) => button.addEventListener('click', () => navigate(button.dataset.view)));
  $$('.lens-row[data-view]').forEach((button) => button.addEventListener('click', () => navigate(button.dataset.view, button.dataset.kind)));
  $('#refresh-button').addEventListener('click', () => load());
  $('#inspector-toggle').addEventListener('click', () => { $('#inspector').classList.toggle('closed'); $('.app-shell').classList.toggle('inspector-closed'); });
  $('#inspector-close').addEventListener('click', () => { $('#inspector').classList.add('closed'); $('.app-shell').classList.add('inspector-closed'); });
  $('#global-search').addEventListener('keydown', (event) => {
    if (event.key === 'Enter') { navigate('routes'); setTimeout(() => route(event.target.value), 0); }
    if (event.key === 'Escape') { event.target.value = ''; event.target.blur(); }
  });
  document.addEventListener('keydown', (event) => {
    if (event.target.matches('input, textarea, select')) return;
    const key = event.key.toLowerCase();
    if (key === '/') { event.preventDefault(); $('#global-search').focus(); }
    if (key === 'g') navigate('graph');
    if (key === 's') navigate('schema');
    if (key === 'q') navigate('query');
    if (key === 'f') navigate('flight');
    if (key === 't') navigate('stream');
    if (key === 'k') navigate('cluster');
    if (key === 'r') navigate('runs');
    if (key === 'c') navigate('claims');
    if (key === 'a') navigate('activity');
    if (key === '1') navigate('overview');
  });
  window.addEventListener('hashchange', () => {
    const view = location.hash.slice(1);
    if (views.has(view) && view !== state.view) { state.view = view; render(); }
  });

  load(true);
  state.refreshTimer = setInterval(() => load(true), 5000);
  state.flightPollTimer = setInterval(pollFlights, 750);
  setInterval(() => { if (state.data) $('#snapshot-age').textContent = ago(state.data.generated_at); }, 1000);
})();
