'use strict';
const $ = (s, r = document) => r.querySelector(s);
const esc = (s) => String(s ?? '').replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));
const api = async (path, opts) => {
  const r = await fetch(path, opts); const text = await r.text(); let data = null;
  try { data = JSON.parse(text); } catch (_) {}
  if (!r.ok) throw new Error((data && data.error) || r.status + ' ' + text.slice(0, 120));
  return data;
};
const post = (p, b) => api(p, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(b || {}) });
const OPERATOR = 'hatchery.operator';
const fmtTime = (ms) => new Date(ms).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' });
function toast(t) { document.querySelectorAll('.toast').forEach((x) => x.remove()); const d = document.createElement('div'); d.className = 'toast'; d.textContent = t; document.body.appendChild(d); setTimeout(() => d.remove(), 7000); }

// ---- avatars (soft blob with two eyes, colour from the name)
const PALETTE = ['#1677ff', '#ff7a1a', '#ff3d8b', '#8b5cf6', '#14b8a6', '#a855f7', '#22c55e'];
function colorOf(name) { let h = 0; for (const c of name) h = (h * 31 + c.charCodeAt(0)) >>> 0; return PALETTE[h % PALETTE.length]; }
function avatar(name, cls = '', online) {
  const c = colorOf(name);
  return `<div class="av ${cls}"><svg viewBox="0 0 64 64" width="100%" height="100%"><path d="M32 4c16 0 28 10 28 26 0 18-10 30-28 30S4 48 4 30C4 14 16 4 32 4z" fill="${c}"/><ellipse cx="23" cy="28" rx="4" ry="6" fill="#0b0b0d" transform="rotate(-12 23 28)"/><ellipse cx="41" cy="28" rx="4" ry="6" fill="#0b0b0d" transform="rotate(-12 41 28)"/></svg>${online ? '<span class="on"></span>' : ''}</div>`;
}

// ---- state
const S = { mode: 'chat', widgets: {}, seenAnswers: new Set(), lastFrame: 0, promptAt: 0, lastAnsi: '', nodes: [], chat: '*', mail: {}, local: {}, session: null, sessionNode: null, sessionProv: null, target: 'mail', working: {}, panel: false, tab: 'activity', lastStatus: {}, wake: null, library: [] };
const chatName = (id) => (id === '*' ? 'Hatchery HQ' : id);
const addLocal = (chat, item) => { (S.local[chat] ||= []).push({ ts: Date.now(), ...item }); renderChat(); };

// ---- sidebar
function renderSidebar() {
  $('#meav').innerHTML = avatar('Hatchery HQ', 'big');
  const q = $('#search').value.toLowerCase();
  const items = [{ id: '*', name: 'Hatchery HQ', status: null, online: true, kind: 'operator' }, ...S.nodes.map((n) => ({ id: n.node_id, name: n.node_id, status: n.status, online: n.online, kind: n.kind }))]
    .filter((i) => !q || i.name.toLowerCase().includes(q));
  $('#chats').innerHTML = '';
  for (const i of items) {
    const msgs = S.mail[i.id] || []; const last = msgs[msgs.length - 1];
    const preview = last ? last.body : i.id === '*' ? 'All inbound mail and events' : `${i.kind} node · ${i.online ? 'online' : 'offline'}`;
    const b = document.createElement('button'); b.className = 'chatitem' + (S.chat === i.id ? ' on' : ''); b.dataset.id = i.id;
    b.innerHTML = avatar(i.name, '', i.online && i.id !== '*') + `<div class="t"><div class="n">${esc(i.name)}${i.status ? `<span class="st ${esc(i.status)}">${esc(i.status)}</span>` : ''}</div><div class="p">${esc(preview)}</div></div>`;
    b.onclick = () => openChat(i.id);
    $('#chats').appendChild(b);
  }
}
$('#searchbtn').onclick = () => { const s = $('#search'); s.hidden = !s.hidden; if (!s.hidden) s.focus(); };
$('#search').oninput = renderSidebar;

// ---- chat
function openChat(id) {
  S.chat = id; S.target = id === '*' ? 'room' : S.session && S.sessionNode === id ? 'agent' : 'mail';
  $('#headname').textContent = chatName(id); $('#headav').innerHTML = avatar(chatName(id), 'mid');
  $('#input').placeholder = `Message for ${chatName(id)}`;
  renderSidebar(); loadThread(); renderChip(); renderPanelHead();
}
async function loadThread() {
  try {
    if (S.chat === '*') { const d = await post('/api/s/mail/inbox'); S.mail["*"] = d.messages.slice().sort((a, b) => a.created_at_ms - b.created_at_ms); }
    else { const d = await post('/api/s/mail/thread', { node: S.chat }); S.mail[S.chat] = d.messages; }
  } catch (_) { S.mail[S.chat] = S.mail[S.chat] || []; }
  renderChat(); renderSidebar();
}
function renderChat() {
  const items = [];
  for (const m of S.mail[S.chat] || []) items.push({ ts: m.created_at_ms, kind: 'mail', m });
  for (const l of S.local[S.chat] || []) items.push({ ts: l.ts, kind: 'local', l });
  if (S.chat === '*') for (const [id, ls] of Object.entries(S.local)) if (id !== '*') for (const l of ls) if (l.sys) items.push({ ts: l.ts, kind: 'local', l });
  items.sort((a, b) => a.ts - b.ts);
  const box = $('#chat'); const nearBottom = box.scrollHeight - box.scrollTop - box.clientHeight < 600;
  box.innerHTML = '';
  if (!items.length) box.innerHTML = `<div class="sys">${S.chat === '*' ? 'Mail from your nodes and system events appear here.' : 'No messages yet. Nodes that are neutral have no mailbox until approved.'}</div>`;
  for (const it of items) {
    if (it.kind === 'mail') {
      const m = it.m; const mine = m.from === OPERATOR;
      const d = document.createElement('div'); d.className = 'msg ' + (mine ? 'me' : 'them');
      d.innerHTML = `${mine ? '' : `<div class="who">${esc(m.from)}${S.chat === '*' ? ' → ' + esc(m.to) : ''}</div>`}${m.subject && m.subject !== '-' ? `<b>${esc(m.subject)}</b><br>` : ''}${esc(m.body)}<div class="ts">${fmtTime(m.created_at_ms)}</div>`;
      box.appendChild(d);
    } else {
      const l = it.l;
      if (l.answer) { const d = document.createElement('div'); d.className = 'msg them'; d.innerHTML = `<div class="who">${esc(S.sessionProv || 'agent')} · ${esc(S.chat)}</div>${esc(l.text)}<div class="ts">${fmtTime(l.ts)}</div>`; box.appendChild(d); }
      else if (l.sys) { const d = document.createElement('div'); d.className = 'sys'; d.textContent = l.text; box.appendChild(d); }
      else if (l.card) { const d = document.createElement('div'); d.className = 'card'; d.innerHTML = l.card; box.appendChild(d); }
      else { const d = document.createElement('div'); d.className = 'msg me'; d.innerHTML = `${l.tag ? `<div class="who" style="color:#bcd6ff">${esc(l.tag)}</div>` : ''}${esc(l.text)}<div class="ts">${fmtTime(l.ts)}</div>`; box.appendChild(d); }
    }
  }
  if (S.working[S.chat] && S.working[S.chat] > Date.now()) {
    const w = document.createElement('div'); w.className = 'working'; w.innerHTML = avatar(chatName(S.chat), 'sm') + 'Working on it <span class="dots"><span>.</span><span>.</span><span>.</span></span>'; box.appendChild(w);
  }
  $('#chat').dataset.n = items.length; if (nearBottom) box.scrollTop = box.scrollHeight;
}
function renderChip() {
  const chip = $('#chip');
  if (S.chat === '*') { chip.hidden = false; chip.textContent = '→ room:general'; chip.onclick = null; return; }
  if (S.session && S.sessionNode === S.chat) { chip.hidden = false; chip.textContent = S.target === 'agent' ? `→ agent (${S.sessionProv}) · click to switch to mail` : '→ mail · click to talk to the agent'; chip.onclick = () => { S.target = S.target === 'agent' ? 'mail' : 'agent'; renderChip(); }; }
  else { chip.hidden = false; chip.textContent = '→ mail to ' + S.chat; chip.onclick = null; }
}
async function send() {
  const text = $('#input').value.trim(); if (!text) return; $('#input').value = ''; autosize();
  try {
    if (S.chat === '*') { await post('/api/s/mail/send', { to: 'room:general', subject: '-', body: text }); await loadThread(); return; }
    if (S.target === 'agent' && S.session) {
      addLocal(S.chat, { text, tag: `to agent (${S.sessionProv})` });
      tsend({ t: 'prompt', text }); S.promptAt = Date.now(); S.working[S.chat] = Date.now() + 30000; setTimeout(renderChat, 25100); renderChat();
    } else {
      await post('/api/s/mail/send', { to: S.chat, subject: '-', body: text });
      S.working[S.chat] = Date.now() + 20000; await loadThread(); setTimeout(renderChat, 20100);
    }
  } catch (e) { addLocal(S.chat, { sys: true, text: 'Not sent: ' + e.message }); }
}
const autosize = () => { const i = $('#input'); i.style.height = 'auto'; i.style.height = Math.min(i.scrollHeight, 140) + 'px'; };
$('#input').oninput = autosize;
$('#input').onkeydown = (e) => { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); send(); } };
$('#sendbtn').onclick = send;
$('#micbtn').onclick = () => toast('Voice input is not available in web HQ yet');

// ---- composer + menu
function menu(items, anchorLeft) {
  const m = $('#menu'); m.innerHTML = ''; m.hidden = false;
  for (const [label, fn] of items) { const b = document.createElement('button'); b.textContent = label; b.onclick = () => { m.hidden = true; fn(); }; m.appendChild(b); }
}
document.addEventListener('click', (e) => { if (!e.target.closest('#menu') && !e.target.closest('#addbtn') && !e.target.closest('#plusbtn')) $('#menu').hidden = true; });
$('#addbtn').onclick = () => menu([
  ['Run an agent on this node…', () => { openPanel('terminal'); }],
  ['Create a task…', () => { openPanel('tasks'); }],
  ['Issue enrollment code', issueCode],
  ['Open browser stream…', () => openPanel('browser')],
]);
$('#plusbtn').onclick = () => menu([['Issue enrollment code', issueCode], ['Refresh nodes', loadNodes]]);
async function issueCode() {
  try {
    const d = await post('/api/c2/codes');
    addLocal(S.chat, { card: `<b>Enrollment code</b><br><code class="big" id="codetext">${esc(d.code)}</code><br><span class="sub">Single use. Give it to the node operator; the node generates its own keys.</span>` });
  } catch (e) { toast('error: ' + e.message); }
}

// ---- nodes loading and change notices
async function loadNodes() {
  try {
    const d = await api('/api/nodes'); S.nodes = d.nodes;
    $('#conn').className = 'dot ' + (d.ready ? 'ok' : 'bad'); $('#connt').textContent = d.ready ? 'C2 ready' : 'C2 not ready';
    for (const n of S.nodes) {
      const key = n.status + '/' + (n.online ? 'on' : 'off'); const prev = S.lastStatus[n.node_id];
      if (prev && prev !== key) addLocal(n.node_id, { sys: true, text: `${n.node_id}: ${prev.replace('/', ' · ')} → ${n.status} · ${n.online ? 'online' : 'offline'}` });
      else if (!prev) addLocal(n.node_id, { sys: true, text: `${n.node_id} (${n.kind}) is ${n.status} · ${n.online ? 'online' : 'offline'}` });
      S.lastStatus[n.node_id] = key;
    }
  } catch (_) { $('#conn').className = 'dot bad'; $('#connt').textContent = 'C2 unreachable'; }
  renderSidebar(); renderPanelHead(); if (S.panel && S.tab === 'nodes') renderNodesTab();
}

// ---- right panel
const TABS = [['activity', 'Activity'], ['nodes', 'Nodes'], ['terminal', 'Terminal'], ['browser', 'Browser'], ['tasks', 'Tasks'], ['mail', 'Mail'], ['library', 'Library']];
function renderPanelHead() { $('#panelav').innerHTML = avatar(chatName(S.chat), 'big'); $('#panelname').textContent = chatName(S.chat); const n = S.nodes.find((x) => x.node_id === S.chat); $('#panelsub').textContent = n ? `${n.kind} · ${n.status} · ${n.online ? 'online' : 'offline'}` : 'operator'; }
function openPanel(tab) { S.panel = true; $('#panel').classList.add('open'); setTab(tab || S.tab); renderPanelHead(); }
function setTab(tab) {
  S.tab = tab; $('#ptabs').innerHTML = '';
  for (const [id, label] of TABS) { const b = document.createElement('button'); b.textContent = label; b.className = id === tab ? 'on' : ''; b.onclick = () => setTab(id); $('#ptabs').appendChild(b); }
  document.querySelectorAll('.ptab').forEach((t) => t.classList.toggle('on', t.id === 'p-' + tab));
  if (tab === 'activity') renderActivity(); if (tab === 'terminal') showMini(); if (tab === 'nodes') renderNodesTab(); if (tab === 'tasks') renderTasksTab(); if (tab === 'mail') renderMailTab(); if (tab === 'library') renderLibraryTab();
}
$('#headbtn').onclick = () => { if (S.panel) { S.panel = false; $('#panel').classList.remove('open'); } else openPanel(); };
$('#closepanel').onclick = () => { S.panel = false; $('#panel').classList.remove('open'); };

async function renderNodesTab() {
  const root = $('#p-nodes'); let policy = '';
  try { policy = JSON.stringify(await api('/api/c2/policy'), null, 2); } catch (e) { policy = e.message; }
  root.innerHTML = `<div class="row"><button class="btn blue" id="nc">Issue enrollment code</button><button class="btn" id="pr">Reload policy</button></div>`
    + S.nodes.map((n) => `<div class="pcard"><div class="row"><b>${esc(n.node_id)}</b><span class="st ${esc(n.status)}">${esc(n.status)}</span><span class="sub">${esc(n.kind)} · ${n.online ? 'online' : esc(n.transport || 'offline')} ${n.tunnel_ip ? '· ' + esc(n.tunnel_ip) : ''}</span></div>`
      + (n.enrolled ? `<div class="row">${['neutral', 'member', 'trusted', 'suspended'].filter((s) => s !== n.status).map((s) => `<button class="btn" data-st="${s}" data-id="${esc(n.node_id)}">${s}</button>`).join('')}<button class="btn red" data-rv="${esc(n.node_id)}">revoke</button></div><div class="sub">rooms: ${esc((n.grants?.rooms || []).join(', ') || '—')} · capabilities: ${esc((n.grants?.capabilities || []).join(', ') || '—')}</div>` : '<div class="sub">token node (not enrolled)</div>')
      + '</div>').join('')
    + `<div class="pcard"><h4>Policy</h4><pre>${esc(policy)}</pre></div>`;
  $('#nc').onclick = issueCode; $('#pr').onclick = async () => { try { await post('/api/c2/policy/reload'); toast('policy reloaded'); renderNodesTab(); } catch (e) { toast('error: ' + e.message); } };
  root.querySelectorAll('[data-st]').forEach((b) => (b.onclick = async () => { try { await post(`/api/c2/nodes/${b.dataset.id}/status`, { status: b.dataset.st }); toast(`${b.dataset.id} → ${b.dataset.st}`); await loadNodes(); } catch (e) { toast('error: ' + e.message); } }));
  root.querySelectorAll('[data-rv]').forEach((b) => (b.onclick = async () => { if (!confirm('Revoke ' + b.dataset.rv + '? This is final.')) return; try { await post(`/api/c2/nodes/${b.dataset.rv}/revoke`); await loadNodes(); } catch (e) { toast('error: ' + e.message); } }));
}

// ---- terminal tab
const COLS = 100, ROWS = 30;
const term = new Terminal({ rows: ROWS, cols: COLS, cursorBlink: true, fontSize: 12.5, theme: { background: '#000000' } });
term.open($('#terminal'));
let mini = null;
function showMini() { if (!mini) { mini = new Terminal({ rows: ROWS, cols: COLS, fontSize: 8, theme: { background: '#000000' } }); mini.open($('#miniterm')); mini.onData((data) => tsend({ t: 'data', data })); } if (S.lastAnsi) { mini.reset(); mini.write(S.lastAnsi); } }
function setMode(m) {
  S.mode = m; document.body.classList.toggle('ptymode', m === 'pty'); $('#ptyview').classList.toggle('on', m === 'pty');
  document.querySelectorAll('#seg button').forEach((b) => b.classList.toggle('on', b.dataset.m === m));
  if (m === 'pty') term.focus();
}
document.querySelectorAll('#seg button').forEach((b) => (b.onclick = () => setMode(b.dataset.m)));
let tws = null;
function tsend(o) { if (tws && tws.readyState === 1) tws.send(JSON.stringify(o)); }
$('#tspawn').onclick = () => {
  const node = S.chat === '*' ? (S.nodes[0] && S.nodes[0].node_id) : S.chat; if (!node) { toast('select a node chat first'); return; }
  if (tws) tws.close();
  const prov = $('#tprov').value; const q = new URLSearchParams({ node, provider: prov, rows: ROWS, cols: COLS });
  term.reset(); if (mini) mini.reset(); S.lastAnsi = ''; $('#tinfo').textContent = 'spawning…';
  tws = new WebSocket(`ws://${location.host}/api/term?${q}`);
  tws.onmessage = (m) => {
    const d = JSON.parse(m.data);
    if (d.type === 'spawned') { S.session = d.session; S.sessionNode = node; S.sessionProv = prov; S.target = 'agent'; $('#tinfo').textContent = `${prov} on ${node}`; addLocal(node, { sys: true, text: `Agent ${prov} started on ${node}` }); if (S.chat === node) renderChip(); }
    if (d.type === 'error') { $('#tinfo').textContent = d.text; addLocal(node, { sys: true, text: d.text }); }
    if (d.type === 'ack') { $('#tinfo').textContent = `${d.op}: ${d.ok ? 'ok' : 'failed'}`; if (!d.ok) { S.working[node] = 0; addLocal(node, { sys: true, text: 'Refused: ' + (/does not permit/.test(d.detail) ? 'node status does not permit operator requests (policy)' : d.detail.slice(0, 120)) }); } }
    if (d.type === 'frame') {
      const bytes = Uint8Array.from(atob(d.ansi), (c) => c.charCodeAt(0));
      window.lastFrameText = d.text; term.reset(); term.write(bytes); S.lastAnsi = bytes; if (mini) { mini.reset(); mini.write(bytes); }
      onFrame(node, d.text);
    }
  };
  tws.onclose = () => { $('#tinfo').textContent += ' [closed]'; if (S.sessionNode) { addLocal(S.sessionNode, { sys: true, text: `Agent session on ${S.sessionNode} ended` }); } S.session = null; S.target = 'mail'; renderChip(); };
};
$('#tstop').onclick = () => tsend({ t: 'stop' });
term.onData((data) => tsend({ t: 'data', data }));

// ---- PTY projection: heuristic parsing of the screen into chat answers and widgets.
// No provider streams structured events through the PTY, so every widget is labelled heuristic.
function parseBlocks(text) {
  const blocks = []; let cur = null;
  for (const raw of text.split('\n')) {
    const l = raw.replace(/\s+$/, '');
    if (/^(∴|✻|∵)\s*/.test(l)) { cur = { kind: 'thought', text: l.replace(/^\S+\s*/, '').replace(/^Thinking:\s*/, '') }; blocks.push(cur); }
    else if (/^[●⏺•]\s+[\w.:-]+\(.*\)\s*$/.test(l)) { cur = { kind: 'tool', text: l.replace(/^\S+\s+/, ''), result: '' }; blocks.push(cur); }
    else if (/^[●⏺•]\s+/.test(l)) { cur = { kind: 'answer', text: l.replace(/^\S+\s+/, '') }; blocks.push(cur); }
    else if (/^\s*⎿\s*/.test(l)) { const t = l.replace(/^\s*⎿\s*/, ''); const last = [...blocks].reverse().find((b) => b.kind === 'tool'); if (last) last.result = last.result ? last.result + ' ' + t : t; cur = null; }
    else if (/^(diff --git|@@ )/.test(l) || (cur && cur.kind === 'diff' && /^[ +\-@]/.test(l))) { if (!cur || cur.kind !== 'diff') { cur = { kind: 'diff', text: '' }; blocks.push(cur); } cur.text += (cur.text ? '\n' : '') + l; }
    else if (!l.trim()) cur = null;
    else if (cur && cur.kind === 'answer' && /^\s{2}\S/.test(l)) cur.text += ' ' + l.trim();
  }
  return blocks;
}
let commitTimer = null;
function onFrame(node, text) {
  S.lastFrame = Date.now(); const ws = (S.widgets[node] ||= []);
  const blocks = parseBlocks(text);
  for (const b of blocks) {
    if (b.kind === 'answer') continue;
    const ex = ws.find((w) => w.kind === b.kind && (w.text === b.text || (b.kind === 'diff' && b.text.startsWith(w.text))));
    if (ex && b.kind === 'diff') ex.text = b.text;
    if (ex) { if (b.result) ex.result = b.result; } else ws.push({ ...b, ts: Date.now() });
  }
  if (S.panel && S.tab === 'activity') renderActivity();
  clearTimeout(commitTimer);
  commitTimer = setTimeout(() => {
    for (const b of blocks) if (b.kind === 'answer' && !S.seenAnswers.has(node + '|' + b.text)) { S.seenAnswers.add(node + '|' + b.text); addLocal(node, { answer: true, text: b.text }); }
    if (S.working[node] && S.lastFrame > S.promptAt) S.working[node] = 0;
    renderChat();
  }, 1300);
}
function diffHtml(t) { return t.split('\n').map((l) => `<span class="${l.startsWith('+') ? 'd-add' : l.startsWith('-') ? 'd-del' : l.startsWith('@@') ? 'd-hunk' : ''}">${esc(l)}</span>`).join('\n'); }
function renderActivity() {
  const root = $('#p-activity'); const node = S.chat === '*' ? S.sessionNode : S.chat; const ws = (node && S.widgets[node]) || [];
  let h = `<div class="sub" style="margin-bottom:8px">What the agent does in the PTY${node ? ' on ' + esc(node) : ''} <span class="tagh">heuristic parsing (no structured stream)</span></div>`;
  h += ws.slice(-14).reverse().map((w) => {
    if (w.kind === 'thought') return `<div class="wd thought"><div class="k">💭 Thought</div><div class="b">${esc(w.text)}</div></div>`;
    if (w.kind === 'tool') return `<div class="wd tool"><div class="k">⚙ Action · tool call</div><div class="b">${esc(w.text)}</div>${w.result ? `<div class="r">⎿ ${esc(w.result)}</div>` : ''}</div>`;
    if (w.kind === 'diff') return `<div class="wd diff"><div class="k">± Git diff</div><pre>${diffHtml(w.text)}</pre></div>`;
    return '';
  }).join('') || '<div class="wd"><div class="b sub">No agent activity yet. Start an agent from the Terminal tab, then talk to it in the chat.</div></div>';
  const mail = (S.mail[S.chat] || []).slice(-3);
  h += `<div class="wd"><div class="k">✉ Messenger <span class="tagh">mail with ${esc(chatName(S.chat))}</span></div>` + (mail.map((m) => `<div style="margin:4px 0"><span class="sub">${esc(m.from === OPERATOR ? 'you' : m.from)}:</span> ${esc(m.body)}</div>`).join('') || '<div class="sub">no mail yet</div>') + '</div>';
  root.innerHTML = h;
}

// ---- tasks tab
async function renderTasksTab() {
  const root = $('#p-tasks'); let tasks = [];
  try { tasks = (await api('/api/s/tasks')).tasks; } catch (e) { root.innerHTML = `<div class="sub">${esc(e.message)}</div>`; return; }
  const def = S.chat !== '*' ? S.chat : '';
  root.innerHTML = `<div class="pcard"><h4>New task</h4><div class="row"><input id="tt" placeholder="Title" style="flex:1"></div><div class="row"><input id="tb" placeholder="Details" style="flex:1"></div><div class="row"><input id="ta" placeholder="Assign to node" value="${esc(def)}"><button class="btn blue" id="tc">Create</button></div></div>`
    + (tasks.map((t) => `<div class="pcard"><div class="row"><b>#${t.id} ${esc(t.title)}</b><span class="st">${esc(t.status)}</span></div><div class="sub">${esc(t.body)} · ${t.assignee ? 'assigned to ' + esc(t.assignee) : 'unassigned'}</div>${['done', 'cancelled'].includes(t.status) ? '' : `<div class="row" style="margin-top:8px"><button class="btn" data-as="${t.id}">assign…</button><button class="btn red" data-cx="${t.id}">cancel</button></div>`}</div>`).join('') || '<div class="sub">No tasks yet.</div>');
  $('#tc').onclick = async () => { try { const a = $('#ta').value.trim(); await post('/api/s/tasks', { title: $('#tt').value, body: $('#tb').value, assignee: a || undefined }); addLocal(a || S.chat, { sys: true, text: `Task created: ${$('#tt').value}${a ? ' → ' + a : ''}` }); toast('task created'); renderTasksTab(); } catch (e) { toast('error: ' + e.message); } };
  root.querySelectorAll('[data-cx]').forEach((b) => (b.onclick = async () => { await post(`/api/s/tasks/${b.dataset.cx}/cancel`); renderTasksTab(); }));
  root.querySelectorAll('[data-as]').forEach((b) => (b.onclick = async () => { const who = prompt('Assign to node:'); if (who) { try { await post(`/api/s/tasks/${b.dataset.as}/assign`, { assignee: who }); } catch (e) { toast(e.message); } renderTasksTab(); } }));
}

// ---- mail tab (all inbound mail and rooms; conversations live in the chat)
async function renderMailTab() {
  const root = $('#p-mail'); let msgs = [];
  try { msgs = (await post('/api/s/mail/inbox')).messages.slice().reverse(); } catch (e) { root.innerHTML = `<div class="sub">${esc(e.message)}</div>`; return; }
  root.innerHTML = `<div class="pcard"><h4>Send to a room or node</h4><div class="row"><input id="mt" placeholder="room:general or node" value="room:general"><input id="ms" placeholder="Subject"></div><div class="row"><input id="mbx" placeholder="Message" style="flex:1"><button class="btn blue" id="mgo">Send</button></div></div>`
    + `<div class="pcard"><h4>Inbox of hatchery.operator</h4><table>${msgs.map((m) => `<tr><td>${fmtTime(m.created_at_ms)}</td><td>${esc(m.from)}</td><td>${esc(m.subject)}</td><td>${esc(m.body)}</td></tr>`).join('') || '<tr><td class="sub">empty</td></tr>'}</table></div>`;
  $('#mgo').onclick = async () => { try { await post('/api/s/mail/send', { to: $('#mt').value.trim(), subject: $('#ms').value || '-', body: $('#mbx').value }); toast('sent'); } catch (e) { toast('error: ' + e.message); } };
}

// ---- library tab (evidence screens)
async function renderLibraryTab() {
  const root = $('#p-library');
  try { S.library = (await api('/api/library')).files; } catch (_) { S.library = []; }
  root.innerHTML = S.library.length ? `<div class="lib">${S.library.map((f) => `<div><img loading="lazy" src="/api/library/${encodeURIComponent(f)}" data-f="${esc(f)}"><div class="sub">${esc(f)}</div></div>`).join('')}</div>` : '<div class="sub">No library folder configured (HATCHERY_HQ_LIBRARY).</div>';
  root.querySelectorAll('img').forEach((i) => (i.onclick = () => { const lb = $('#lightbox'); lb.innerHTML = `<img src="${i.src}">`; lb.hidden = false; lb.onclick = () => (lb.hidden = true); }));
}

// ---- browser tab (dig2 station frames)
let bws = null; const canvas = $('#bcanvas'); const ctx = canvas.getContext('2d');
function bsend(o) { if (bws && bws.readyState === 1) bws.send(JSON.stringify(o)); }
$('#bopen').onclick = () => {
  if (bws) bws.close();
  bws = new WebSocket(`ws://${location.host}/api/browser?session=${encodeURIComponent($('#bsess').value.trim())}`); bws.binaryType = 'blob';
  bws.onmessage = async (m) => {
    if (typeof m.data === 'string') { const d = JSON.parse(m.data); if (d.type === 'sent') $('#binfo').textContent = `sent ${d.op} (${d.bytes} bytes) at ${d.x ?? ''},${d.y ?? ''}`; return; }
    const bmp = await createImageBitmap(m.data); canvas.width = bmp.width; canvas.height = bmp.height; ctx.drawImage(bmp, 0, 0); window.browserFrames = (window.browserFrames || 0) + 1;
  };
};
const pos = (e) => { const r = canvas.getBoundingClientRect(); return { x: Math.round((e.clientX - r.left) * canvas.width / r.width), y: Math.round((e.clientY - r.top) * canvas.height / r.height) }; };
canvas.onclick = (e) => bsend({ t: 'click', ...pos(e), button: 'left' });
canvas.oncontextmenu = (e) => { e.preventDefault(); bsend({ t: 'click', ...pos(e), button: 'right' }); };
canvas.onwheel = (e) => { e.preventDefault(); bsend({ t: 'scroll', ...pos(e), dx: e.deltaX, dy: e.deltaY }); };
$('#bnav').onclick = () => bsend({ t: 'nav', url: $('#burl').value });
$('#btype').onclick = () => bsend({ t: 'text', text: $('#btext').value });
$('#benter').onclick = () => bsend({ t: 'key', key: 'Enter' });
$('#bup').onclick = () => bsend({ t: 'scroll', x: 400, y: 250, dx: 0, dy: -300 });
$('#bdown').onclick = () => bsend({ t: 'scroll', x: 400, y: 250, dx: 0, dy: 300 });

// ---- live events: mail wakes HQ
const es = new EventSource('/api/events');
es.onmessage = (m) => {
  const d = JSON.parse(m.data);
  if (d.type === 'mail') {
    const msg = d.message; window.lastWake = d;
    toast(`HQ woken: mail from ${msg.from}`);
    S.working[msg.from] = 0;
    addLocal(msg.from, { sys: true, text: `HQ woken by mail from ${msg.from} (+${Math.max(0, d.woke_at_ms - msg.created_at_ms)} ms)` });
    if (S.chat === msg.from || S.chat === '*') loadThread(); else { S.mail[msg.from] = (S.mail[msg.from] || []).concat([msg]); renderSidebar(); }
    if (S.panel && S.tab === 'mail') renderMailTab();
  }
};
$('#headav').innerHTML = avatar('Hatchery HQ', 'mid'); setInterval(() => { if (S.working[S.chat]) renderChat(); }, 800); setTab('activity');
loadNodes().then(() => { openChat('*'); }); setInterval(loadNodes, 4000);
