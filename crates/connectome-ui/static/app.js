(() => {
  'use strict';
  const $ = (selector) => document.querySelector(selector);
  const esc = (value) => String(value ?? '').replace(/[&<>"']/g, (char) => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[char]));
  const json = (value) => esc(JSON.stringify(value, null, 2));
  const state = { view: 'overview', snapshot: null, capabilities: null };

  async function fetchJson(url, options) {
    const response = await fetch(url, { cache: 'no-store', ...options });
    const value = await response.json();
    if (!response.ok) throw new Error(value.error || `${response.status} ${response.statusText}`);
    return value;
  }

  async function refresh() {
    setStatus('waiting', 'Reading verified snapshot');
    try {
      const [snapshot, capabilities] = await Promise.all([
        fetchJson('/api/snapshot'),
        fetchJson('/api/runtime/capabilities')
      ]);
      state.snapshot = snapshot;
      state.capabilities = capabilities;
      const instance = snapshot.instance?.id || 'rrd';
      $('#instance').innerHTML = `<span>INSTANCE</span><strong>${esc(instance)}</strong><small>${esc(snapshot.scope)} · ${esc(snapshot.readiness.backend)}</small>`;
      $('#crumb-instance').textContent = instance;
      $('#model-count').textContent = snapshot.models?.models?.length || 0;
      $('#graph-count').textContent = (snapshot.graph?.records?.length || 0) + (snapshot.graph?.relations?.length || 0);
      $('#change-count').textContent = snapshot.changes?.changes?.length || 0;
      $('#capability-count').textContent = snapshot.product_capabilities?.capabilities?.length || 0;
      $('#stamp').textContent = `cursor ${snapshot.read.runtime_cursor} · assembly ${snapshot.read.assembly_attempts}`;
      setStatus('ready', 'RRD connected');
      render();
    } catch (error) {
      setStatus('error', 'RRD unavailable');
      $('#main').innerHTML = `<div class="empty"><div><h2>Snapshot failed</h2><p>${esc(error.message)}</p></div></div>`;
    }
  }

  function setStatus(kind, text) {
    $('#status-dot').className = `dot ${kind === 'ready' ? '' : kind}`;
    $('#status').textContent = text;
  }

  function head(title, text, badge = '') {
    return `<div class="page-head"><div><span class="eyebrow">AUTHORITATIVE RRD VIEW</span><h1>${esc(title)}</h1><p>${esc(text)}</p></div>${badge}</div>`;
  }

  function metric(label, value, note) {
    return `<article class="card metric"><span>${esc(label)}</span><strong>${esc(value)}</strong><small>${esc(note)}</small></article>`;
  }

  function render() {
    if (!state.snapshot) return;
    $('#crumb-view').textContent = state.view;
    const renderer = ({overview, models, graph, activity, capabilities, query, raw})[state.view] || overview;
    $('#main').innerHTML = renderer();
    if (state.view === 'query') bindQuery();
  }

  function overview() {
    const s = state.snapshot;
    const sections = s.sections || [];
    return head('One engine, one read stamp', 'Every panel below comes from POST /v1/diagnostics/read through rrd-client; Connectome does not open storage.', '<span class="badge good">validated</span>') +
      `<section class="metrics">${metric('Runtime cursor', s.read.runtime_cursor, 'authoritative head')}${metric('Claim sequence', s.read.claim_sequence, 'bitemporal memory')}${metric('Models', s.models?.models?.length || 0, `schema rev ${s.read.schema_revision ?? 'none'}`)}${metric('Artifacts', s.vector_artifacts?.artifacts?.length || 0, `catalogue rev ${s.vector_artifacts?.revision || 0}`)}</section>` +
      `<section class="grid"><article class="panel"><header><h2>Verified coordinates</h2><span>${esc(s.readiness.backend)}</span></header><div class="kv">${Object.entries(s.read).map(([key,value]) => `<div><span>${esc(key)}</span><strong>${esc(value)}</strong></div>`).join('')}</div></article>` +
      `<article class="panel"><header><h2>Coverage disclosures</h2><span>${sections.length} sections</span></header><div class="panel-body">${sections.map(section => `<div class="change"><header><code>${esc(section.id)}</code><span class="badge ${section.coverage === 'complete' ? 'good' : 'partial'}">${esc(section.coverage)}</span></header><small>${esc(section.authority)} · ${section.row_count} rows · cursor ${section.known_at_cursor}</small></div>`).join('') || '<div class="empty">No sections returned</div>'}</div></article></section>`;
  }

  function models() {
    const rows = state.snapshot.models?.models || [];
    return head('Logical models', 'Schema-derived record, relation, and event models at the verified schema revision.') + `<table><thead><tr><th>Kind</th><th>ID</th><th>Properties</th><th>Required</th><th>Constraints</th><th>Schemaless</th></tr></thead><tbody>${rows.map(model => `<tr><td>${esc(model.kind)}</td><td><code>${esc(model.id)}</code></td><td>${model.property_count}</td><td>${model.required_property_count}</td><td>${model.constraint_count}</td><td>${model.allow_additional_properties ? 'yes' : 'no'}</td></tr>`).join('')}</tbody></table>`;
  }

  function graph() {
    const graph = state.snapshot.graph || {records:[],relations:[]};
    const nodes = graph.records || [];
    const edges = graph.relations || [];
    return head('Temporal graph', `Valid at ${graph.valid_at_unix_ms}; known at cursor ${graph.known_at_cursor}.`, `<span class="badge">${nodes.length} records · ${edges.length} relations</span>`) + `<section class="graph">${nodes.map(node => `<article class="node"><code>${esc(node.reference.kind)}</code><strong>${esc(node.reference.id)}</strong><pre>${json(node.properties)}</pre></article>`).join('') || '<div class="empty">No visible graph records</div>'}</section><h2>Directed typed relations</h2><table><thead><tr><th>Relation</th><th>From</th><th>To</th><th>Properties</th></tr></thead><tbody>${edges.map(edge => `<tr><td><code>${esc(edge.reference.kind)}:${esc(edge.reference.id)}</code></td><td>${esc(edge.from.kind)}:${esc(edge.from.id)}</td><td>${esc(edge.to.kind)}:${esc(edge.to.id)}</td><td><code>${json(edge.properties)}</code></td></tr>`).join('')}</tbody></table>`;
  }

  function activity() {
    const page = state.snapshot.changes || {changes:[]};
    return head('Committed activity', `Validated ${page.validation?.method || 'changefeed'} page through cursor ${page.through_cursor || 0}; head ${page.head_cursor || 0}.`, page.has_more ? '<span class="badge partial">bounded page</span>' : '<span class="badge good">through head</span>') + `<article class="panel"><div class="panel-body">${(page.changes || []).slice().reverse().map(change => `<section class="change"><header><code>#${change.cursor} · ${esc(change.actor)}</code><span>${esc(change.scope)}</span></header><small>${new Date(change.at_unix_ms).toISOString()} · commit ${esc(change.commit_sha256.slice(0,12))}</small><pre>${json(change.mutation)}</pre></section>`).join('') || '<div class="empty">No retained changes in this scope</div>'}</div></article>`;
  }

  function capabilities() {
    const catalogue = state.snapshot.product_capabilities?.capabilities || [];
    const tools = state.snapshot.runtime_tools?.tools || [];
    return head('Surface dispositions', 'RRD owns this catalogue. Available, experimental, and planned labels are rendered without Connectome inventing maturity claims.', `<span class="badge">${tools.length} governed tools</span>`) + `<section class="cap-grid">${catalogue.map(cap => `<article class="cap"><header><div><span class="eyebrow">${esc(cap.category)}</span><h2>${esc(cap.label)}</h2></div><code>${esc(cap.id)}</code></header><p>${esc(cap.summary)}</p><div class="bindings">${cap.bindings.map(binding => `<span class="${esc(binding.disposition)}">${esc(binding.surface)} · ${esc(binding.disposition)}</span>`).join('')}</div></article>`).join('')}</section><h2>Runtime tools</h2><table><thead><tr><th>Tool</th><th>Mutation</th><th>Action</th><th>Description</th></tr></thead><tbody>${tools.map(tool => `<tr><td><code>${esc(tool.name)}</code></td><td>${tool.mutation ? 'yes' : 'no'}</td><td>${esc(tool.action)}</td><td>${esc(tool.description)}</td></tr>`).join('')}</tbody></table>`;
  }

  function query() {
    return head('RRFlowQL query', 'Queries are scope-bound by the gateway and executed through POST /v1/query with RRD policy, budgets, and audit.') + `<section class="panel query"><div class="panel-body"><textarea id="query-source" spellcheck="false">FROM record:document KNOWN HEAD PROJECT id EXPLAIN CONTRACT</textarea><div class="query-actions"><button class="primary" id="run-query">Run query</button></div><div id="query-result" class="query-result"><div class="empty">No query executed</div></div></div></section>`;
  }

  function raw() {
    return head('Raw diagnostic contract', 'Lossless JSON returned by the validated Rust client.') + `<pre class="raw">${json(state.snapshot)}</pre>`;
  }

  function bindQuery() {
    $('#run-query').addEventListener('click', async () => {
      const result = $('#query-result');
      result.innerHTML = '<div class="loading">Executing…</div>';
      try {
        const value = await fetchJson('/api/runtime/query', {method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({query:$('#query-source').value})});
        result.innerHTML = `<pre class="raw">${json(value)}</pre>`;
      } catch (error) {
        result.innerHTML = `<div class="empty">${esc(error.message)}</div>`;
      }
    });
  }

  document.querySelectorAll('nav button').forEach(button => button.addEventListener('click', () => {
    document.querySelectorAll('nav button').forEach(item => item.classList.toggle('active', item === button));
    state.view = button.dataset.view;
    render();
  }));
  $('#refresh').addEventListener('click', refresh);
  document.addEventListener('keydown', event => {
    if (/^[1-6]$/.test(event.key) && !/INPUT|TEXTAREA/.test(event.target.tagName)) document.querySelectorAll('nav button')[Number(event.key)-1]?.click();
  });
  refresh();
})();
