(() => {
  const panel = document.querySelector('.graph-panel');
  if (!panel) return;
  const button = document.getElementById('build-graph');
  const status = document.getElementById('graph-status');
  const stage = document.getElementById('graph-stage');
  const canvas = document.getElementById('graph-canvas');
  const detail = document.getElementById('graph-node-detail');
  const legend = document.getElementById('graph-legend-items');
  const csrf = () => document.querySelector('meta[name="csrf-token"]')?.content || '';
  const colors = {
    ROUTE: '#ffb454', ENDPOINT: '#ff7b72', CLASS: '#58a6ff', METHOD: '#79c0ff',
    FUNCTION: '#56d364', MODULE: '#d2a8ff', FILE: '#8b949e', TEST: '#f2cc60',
    EXTERNAL_SERVICE: '#f778ba', DATABASE_MODEL: '#39d0d8', JOB: '#ffa657', DEFAULT: '#8ee8c1'
  };
  const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
  const request = async (url, options = {}) => {
    const response = await fetch(url, {cache: 'no-store', ...options, headers: {'X-CSRF-Token': csrf(), ...(options.headers || {})}});
    const data = await response.json();
    if (!response.ok) throw new Error(data?.detail || data?.error?.message || data?.status || 'Graph request failed');
    return data;
  };
  button.addEventListener('click', async () => {
    button.disabled = true;
    status.className = 'alert info';
    status.textContent = 'Requesting structured analysis from the local Worker…';
    try {
      const start = await request(`/projects/${panel.dataset.projectId}/graph-session`, {method: 'POST'});
      let payload;
      for (let attempt = 0; attempt < 180; attempt += 1) {
        await sleep(1000);
        payload = await request(`/projects/${panel.dataset.projectId}/graph-session/${start.request_id}`);
        if (payload.status === 'SUCCEEDED') break;
        if (['FAILED', 'EXPIRED', 'CANCELLED'].includes(payload.status)) {
          throw new Error(`Graph job ended as ${payload.status}`);
        }
        status.textContent = `Worker is processing the graph… (${payload.status})`;
      }
      if (!payload || payload.status !== 'SUCCEEDED') throw new Error('Graph session timed out');
      const graph = payload.graph?.result || payload.graph;
      renderGraph(graph);
      stage.classList.remove('hidden');
      status.className = payload.freshness === 'CURRENT' ? 'alert info' : 'alert warning';
      status.textContent = `${payload.freshness} · Revision ${payload.source_revision_id.slice(0, 12)} · ${graph.nodes.length}/${graph.node_count_total} Nodes · ${graph.edges.length}/${graph.edge_count_total} Edges · bounded ephemeral view`;
    } catch (error) {
      status.className = 'alert danger';
      status.textContent = `Unable to build visualization: ${error.message}`;
    } finally { button.disabled = false; }
  });

  function renderGraph(graph) {
    const context = canvas.getContext('2d');
    const nodes = graph.nodes.map((node, index) => {
      const phi = Math.acos(1 - 2 * (index + .5) / graph.nodes.length);
      const theta = Math.PI * (1 + Math.sqrt(5)) * index;
      const radius = 180 + (index % 7) * 7;
      return {...node, x: radius * Math.sin(phi) * Math.cos(theta), y: radius * Math.cos(phi), z: radius * Math.sin(phi) * Math.sin(theta)};
    });
    const byId = new Map(nodes.map(node => [node.id, node]));
    const edges = graph.edges.filter(edge => byId.has(edge.source) && byId.has(edge.target));
    const kinds = [...new Set(nodes.map(node => node.kind))].sort();
    legend.replaceChildren(...kinds.map(kind => {
      const row = document.createElement('span');
      const dot = document.createElement('i'); dot.style.background = colors[kind] || colors.DEFAULT;
      row.append(dot, document.createTextNode(kind)); return row;
    }));
    let angleX = -.22, angleY = .45, zoom = 1, dragging = false, previous = null, projected = [];
    const resize = () => {
      const box = canvas.getBoundingClientRect();
      const ratio = Math.min(window.devicePixelRatio || 1, 2);
      canvas.width = Math.max(640, box.width * ratio); canvas.height = Math.max(460, box.height * ratio);
      context.setTransform(ratio, 0, 0, ratio, 0, 0); draw();
    };
    const project = node => {
      const cy = Math.cos(angleY), sy = Math.sin(angleY), cx = Math.cos(angleX), sx = Math.sin(angleX);
      const x1 = node.x * cy - node.z * sy, z1 = node.x * sy + node.z * cy;
      const y1 = node.y * cx - z1 * sx, z2 = node.y * sx + z1 * cx;
      const perspective = 620 / (620 + z2 * zoom);
      return {node, x: canvas.clientWidth / 2 + x1 * zoom * perspective, y: canvas.clientHeight / 2 + y1 * zoom * perspective, z: z2, scale: perspective};
    };
    const draw = () => {
      context.clearRect(0, 0, canvas.clientWidth, canvas.clientHeight);
      const glow = context.createRadialGradient(canvas.clientWidth / 2, canvas.clientHeight / 2, 10, canvas.clientWidth / 2, canvas.clientHeight / 2, canvas.clientWidth / 2);
      glow.addColorStop(0, 'rgba(28,116,86,.22)'); glow.addColorStop(1, 'rgba(7,20,17,0)');
      context.fillStyle = glow; context.fillRect(0, 0, canvas.clientWidth, canvas.clientHeight);
      projected = nodes.map(project).sort((a, b) => a.z - b.z);
      const projectedById = new Map(projected.map(item => [item.node.id, item]));
      edges.forEach(edge => {
        const a = projectedById.get(edge.source), b = projectedById.get(edge.target); if (!a || !b) return;
        context.beginPath(); context.moveTo(a.x, a.y); context.lineTo(b.x, b.y);
        context.strokeStyle = edge.origin === 'RUNTIME_OBSERVED' ? 'rgba(255,180,84,.72)' : `rgba(122,196,169,${Math.max(.08, edge.confidence * .28)})`;
        context.lineWidth = edge.origin === 'RUNTIME_OBSERVED' ? 1.8 : .7; context.stroke();
      });
      projected.forEach(item => {
        const radius = (item.node.entry_point ? 5.4 : 3.2) * item.scale;
        context.beginPath(); context.arc(item.x, item.y, radius, 0, Math.PI * 2);
        context.fillStyle = colors[item.node.kind] || colors.DEFAULT;
        context.shadowColor = context.fillStyle; context.shadowBlur = item.node.entry_point ? 12 : 5; context.fill(); context.shadowBlur = 0;
      });
    };
    canvas.onpointerdown = event => { dragging = true; previous = event; canvas.setPointerCapture(event.pointerId); };
    canvas.onpointermove = event => { if (!dragging) return; angleY += (event.clientX - previous.clientX) * .006; angleX += (event.clientY - previous.clientY) * .006; previous = event; draw(); };
    canvas.onpointerup = () => { dragging = false; };
    canvas.onwheel = event => { event.preventDefault(); zoom = Math.max(.45, Math.min(2.5, zoom * (event.deltaY > 0 ? .9 : 1.1))); draw(); };
    canvas.onclick = event => {
      if (dragging) return;
      const box = canvas.getBoundingClientRect(), x = event.clientX - box.left, y = event.clientY - box.top;
      const selected = [...projected].reverse().find(item => Math.hypot(item.x - x, item.y - y) < 9);
      if (selected) detail.textContent = `${selected.node.kind} · ${selected.node.qualified_name} · ${selected.node.path}:${selected.node.line} · ${selected.node.language}`;
    };
    window.addEventListener('resize', resize, {passive: true}); resize();
  }
})();
