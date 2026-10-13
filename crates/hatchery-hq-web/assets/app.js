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
const PALETTE = ['#1677ff', '#ff7a1a', '#ff3d8b', '#8b5cf6', '#14b8a6', '#a855f7', '#22c55e', '#f5b014', '#ef4444', '#06b6d4'];
function colorOf(name) { let h = 0; for (const c of String(name)) h = (h * 31 + c.charCodeAt(0)) >>> 0; return PALETTE[h % PALETTE.length]; }
function avatarState(name) {
  const n = S.nodes.find((x) => x.node_id === name);
  if (n && !n.online) return 'off';
  const st = S.agentState[name];
  if (st === 'needs_input') return 'attention';
  if (st === 'working') { const t = (S.events[name] && [...S.events[name].values()].filter((e) => e.kind === 'thought').pop()); return t && Date.now() - t.at < 5000 ? 'thinking' : 'working'; }
  return 'idle';
}
// Avatar: round blob with two eyes; the state class (idle / working / thinking / attention / off) animates it.
const SHAPES = [
  'M32 5c15 0 27 11 27 27S47 59 32 59 5 47 5 32 17 5 32 5z',
  'M32 4c16 0 28 10 28 26 0 18-10 30-28 30S4 48 4 30C4 14 16 4 32 4z',
  'M32 6c5 0 26 38 26 46 0 6-6 8-26 8S6 58 6 52C6 44 27 6 32 6z',
  'M16 6h32c6 0 10 4 10 10v32c0 6-4 10-10 10H16C10 58 6 54 6 48V16C6 10 10 6 16 6z',
  'M23 5h18l17 17v20L41 59H23L6 42V22z',
  'M30 5c14-2 29 9 28 27-1 17-13 27-28 26C14 57 4 46 6 30 7 16 17 6 30 5z',
  'M32 4c6 0 9 6 15 8s12 4 12 12c0 6-4 9-4 15 0 12-9 21-23 21S9 51 9 39c0-6-4-9-4-15 0-8 6-10 12-12s9-8 15-8z',
];
function hashOf(name) { let h = 2166136261; for (const c of String(name)) h = Math.imul(h ^ c.charCodeAt(0), 16777619) >>> 0; return h; }
// Avatar "critter": shape + colour derived from the id; state class animates it, .look follows the cursor.
const FAMILY = { dm: [0, 5], group: [3, 4], agent: [1, 2, 6] };
const kindOf = (id) => (id === '*' || String(id).startsWith('room:') ? 'group' : S.session && S.sessionNode === id ? 'agent' : 'dm');
function avatar(name, cls = '', online, force, fam) {
  const h = hashOf(name); const c = colorOf(name); const f = FAMILY[fam || kindOf(name)] || FAMILY.dm; const shape = SHAPES[f[(h >>> 8) % f.length]]; const st = force || avatarState(name);
  const extra = st === 'thinking' ? '<g class="tdots"><circle cx="50" cy="8" r="2.4"/><circle cx="56" cy="3" r="1.8"/></g>' : st === 'attention' ? '<circle class="ring" cx="32" cy="32" r="30" fill="none" stroke="#e0a63a" stroke-width="2.5"/>' : '';
  const ey = 28 + ((h >>> 4) % 5) - 2;
  return `<div class="av ${cls} st-${st}" data-av="${esc(name)}"><svg viewBox="0 0 64 64" width="100%" height="100%"><g class="bodyg"><path class="body" d="${shape}" fill="${c}"/><g class="look"><g class="eyes"><ellipse class="eye" cx="23" cy="${ey}" rx="4" ry="6" fill="#0b0b0d" transform="rotate(-12 23 ${ey})"/><ellipse class="eye" cx="41" cy="${ey}" rx="4" ry="6" fill="#0b0b0d" transform="rotate(-12 41 ${ey})"/></g></g></g>${extra}</svg>${online ? '<span class="on"></span>' : ''}</div>`;
}
// eyes of the open chat's avatars follow the cursor
document.addEventListener('mousemove', (e) => {
  if (matchMedia('(prefers-reduced-motion: reduce)').matches) return;
  document.querySelectorAll('#headav .av .look, #chat .hello .av .look').forEach((g) => {
    const r = g.closest('.av').getBoundingClientRect(); const dx = e.clientX - (r.left + r.width / 2), dy = e.clientY - (r.top + r.height / 2); const d = Math.hypot(dx, dy) || 1; const k = Math.min(1, d / 300) * 4;
    g.setAttribute('transform', `translate(${(dx / d * k).toFixed(2)} ${(dy / d * k).toFixed(2)})`);
  });
});

// ---- state
const S = { mode: 'chat', view: 'chat', space: '*', log: [], events: {}, agentState: {}, lastFrame: 0, promptAt: 0, lastAnsi: '', nodes: [], ready: null, chat: '*', mail: {}, local: {}, session: null, sessionNode: null, sessionProv: null, target: 'mail', working: {}, panel: false, tab: 'activity', lastStatus: {}, wake: null, library: [] };
const chatName = (id) => (id === '*' ? 'general' : id);
const addLocal = (chat, item) => { (S.local[chat] ||= []).push({ ts: Date.now(), ...item }); renderChat(); };
const logEv = (text) => { S.log.push({ ts: Date.now(), text }); if (S.log.length > 300) S.log.shift(); if (S.panel && S.tab === 'status') renderStatus(); };
const visibleNodes = () => S.nodes.filter((n) => n.status !== 'revoked');

// ---- sidebar: only chats (groups, DMs, agent sessions); kind is shown by creature shape only
function renderSidebar() {
  const q = $('#search').value.toLowerCase(); const match = (n) => !q || n.toLowerCase().includes(q);
  const root = $('#chats'); root.innerHTML = '';
  const last = (id) => { const m = S.mail[id] || []; const l = m[m.length - 1]; return l ? l.body : ''; };
  const item = (id, name, preview) => { if (!match(name)) return; const b = document.createElement('button'); b.className = 'chatitem' + (S.chat === id ? ' on' : ''); b.title = name; b.innerHTML = avatar(id) + `<div class="t"><div class="n">${esc(name)}</div><div class="p">${esc(preview)}</div></div>`; b.onclick = () => { openChat(id); document.body.classList.remove('sideopen'); }; root.appendChild(b); };
  if (S.session) item(S.sessionNode, S.sessionNode, last(S.sessionNode) || S.sessionProv);
  item('*', 'general', last('*') || '');
}
$('#searchbtn').onclick = () => { const s = $('#search'); s.hidden = !s.hidden; if (!s.hidden) s.focus(); };
$('#search').oninput = renderSidebar;
$('#sidetoggle').onclick = () => document.body.classList.toggle('sideopen');

// ---- chat
function openChat(id) {
  setMode('chat');
  S.chat = id; S.target = id === '*' ? 'room' : S.session && S.sessionNode === id ? 'agent' : 'mail';
  $('#headname').textContent = chatName(id); $('#headbtn').title = chatName(id); $('#headav').innerHTML = avatar(id, 'sm');
  $('#input').placeholder = id === '*' ? 'Message general' : `Message ${id}`;
  renderSidebar(); loadThread(); renderPanelHead(); if (S.panel) setTab(S.tab);
}
async function loadThread() {
  try {
    if (S.chat === '*') { const d = await post('/api/s/mail/inbox'); S.mail["*"] = d.messages.slice().sort((a, b) => a.created_at_ms - b.created_at_ms); }
    else { const d = await post('/api/s/mail/thread', { node: S.chat }); S.mail[S.chat] = d.messages; }
  } catch (_) { S.mail[S.chat] = S.mail[S.chat] || []; }
  renderChat(); renderSidebar();
}
const isGroup = (id) => id === '*' || String(id).startsWith('room:');
function renderChat() {
  const items = []; const group = isGroup(S.chat);
  const mail = S.mail[S.chat] || [];
  // reconcile: drop optimistic own messages once the server copy is present
  if (S.local[S.chat]) S.local[S.chat] = S.local[S.chat].filter((l) => !l.pending || !mail.some((m) => m.from === OPERATOR && m.body === l.text && m.created_at_ms >= l.ts - 60000));
  for (const m of mail) items.push({ ts: m.created_at_ms, kind: 'mail', m });
  for (const l of S.local[S.chat] || []) items.push({ ts: l.ts, kind: 'local', l });
  if (S.chat !== '*' && S.events[S.chat]) for (const e of S.events[S.chat].values()) if (e.kind === 'answer' || e.kind === 'prompt_needed') items.push({ ts: e.at, kind: 'ev', e });
  items.sort((a, b) => a.ts - b.ts);
  const box = $('#chat'); const nearBottom = box.scrollHeight - box.scrollTop - box.clientHeight < 600;
  box.innerHTML = '';
  if (!items.length) box.innerHTML = `<div class="hello">${avatar(S.chat, 'big')}<b>${esc(chatName(S.chat))}</b></div>`;
  let prevFrom = null;
  const bubble = (mine, from, html, ts, extra = '') => {
    const d = document.createElement('div'); d.className = 'msg ' + (mine ? 'me' : 'them') + extra;
    const label = group && !mine && from && from !== prevFrom ? `<div class="who" style="color:${colorOf(from)}">${esc(from)}</div>` : '';
    if (label) d.classList.add('first');
    d.innerHTML = `${label}${html}<span class="ts" title="${esc(new Date(ts).toLocaleString())}">${fmtTime(ts).slice(0, 5)}</span>`;
    prevFrom = mine ? OPERATOR : from; box.appendChild(d);
  };
  for (const it of items) {
    if (it.kind === 'mail') {
      const m = it.m; const mine = m.from === OPERATOR;
      bubble(mine, m.from, `${m.subject && m.subject !== '-' ? `<b>${esc(m.subject)}</b><br>` : ''}${esc(m.body)}`, m.created_at_ms);
    } else if (it.kind === 'ev') {
      const e = it.e;
      if (e.kind === 'answer') bubble(false, S.chat, esc(e.text), e.at);
      else { const d = document.createElement('div'); d.className = 'card attn'; d.innerHTML = `<b>${esc(e.provider)} needs input</b> · ${esc(e.prompt)}<br>${esc(e.text)}${e.options.length ? '<br><span class="sub">' + e.options.map(esc).join('<br>') + '</span>' : ''}`; box.appendChild(d); prevFrom = null; }
    } else {
      const l = it.l;
      if (l.answer) bubble(false, S.chat, esc(l.text), l.ts);
      else if (l.sys) { const d = document.createElement('div'); d.className = 'sys'; d.textContent = l.text; box.appendChild(d); prevFrom = null; }
      else if (l.card) { const d = document.createElement('div'); d.className = 'card'; d.innerHTML = l.card; box.appendChild(d); prevFrom = null; }
      else bubble(true, OPERATOR, esc(l.text), l.ts, l.pending ? ' pending' : l.failed ? ' failed' : '');
    }
  }
  if ((S.working[S.chat] && S.working[S.chat] > Date.now()) || ['working'].includes(S.agentState[S.chat]) && S.session && S.sessionNode === S.chat) {
    const w = document.createElement('div'); w.className = 'working'; w.innerHTML = avatar(chatName(S.chat), 'sm') + (S.agentState[S.chat] === 'working' && avatarState(S.chat) === 'thinking' ? 'Thinking' : 'Working on it') + ' <span class="dots"><span>.</span><span>.</span><span>.</span></span>'; box.appendChild(w);
  }
  $('#chat').dataset.n = items.length; if (nearBottom) box.scrollTop = box.scrollHeight;
}
function renderChip() {}
async function send() {
  const text = $('#input').value.trim(); if (!text) return; $('#input').value = ''; autosize();
  try {
    if (S.chat === '*' || isGroup(S.chat)) {
      const to = S.chat === '*' ? 'room:general' : S.chat; const l = { ts: Date.now(), text, pending: true }; (S.local[S.chat] ||= []).push(l); renderChat(); $('#chat').scrollTop = 1e9;
      try { await post('/api/s/mail/send', { to, subject: '-', body: text }); l.pending = false; l.sent = true; } catch (e) { l.pending = false; l.failed = true; renderChat(); toast('Not sent: ' + e.message); return; }
      renderChat(); await loadThread(); return;
    }
    if (S.target === 'agent' && S.session) {
      addLocal(S.chat, { text, tag: `to agent (${S.sessionProv})` });
      tsend({ t: 'prompt', text }); S.promptAt = Date.now(); S.working[S.chat] = Date.now() + 30000; setTimeout(renderChat, 25100); renderChat();
    } else {
      await post('/api/s/mail/send', { to: S.chat, subject: '-', body: text });
      S.working[S.chat] = Date.now() + 20000; await loadThread(); setTimeout(renderChat, 20100);
    }
  } catch (e) { addLocal(S.chat, { sys: true, text: 'Not sent: ' + e.message }); }
}
$('#micbtn').onclick = () => {
  const SR = window.SpeechRecognition || window.webkitSpeechRecognition; if (!SR) { toast('Voice input is not supported in this browser'); return; }
  if (S.rec) { S.rec.stop(); return; }
  const r = new SR(); r.interimResults = true; r.lang = navigator.language || 'ru-RU'; const base = $('#input').value; S.rec = r; $('#micbtn').classList.add('rec');
  r.onresult = (e) => { $('#input').value = base + [...e.results].map((x) => x[0].transcript).join(''); autosize(); };
  r.onend = () => { S.rec = null; $('#micbtn').classList.remove('rec'); }; r.start();
};
const autosize = () => { const i = $('#input'); const max = 118; i.style.height = 'auto'; const h = Math.min(i.scrollHeight, max); i.style.height = h + 'px'; i.classList.toggle('scroll', i.scrollHeight > max); };
$('#input').oninput = autosize; ['keyup', 'cut', 'paste', 'change', 'focus'].forEach((ev) => $('#input').addEventListener(ev, () => setTimeout(autosize, 0)));
$('#input').onkeydown = (e) => { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); send(); } };
$('#sendbtn').onclick = send;

// ---- composer + menu
function menu(items, anchorLeft) {
  const m = $('#menu'); m.removeAttribute('style'); m.innerHTML = ''; m.hidden = false;
  for (const [label, fn] of items) { const b = document.createElement('button'); b.textContent = label; b.onclick = () => { m.hidden = true; fn(); }; m.appendChild(b); }
}
document.addEventListener('click', (e) => { if (!e.target.closest('#menu') && !e.target.closest('#addbtn') && !e.target.closest('#plusbtn')) $('#menu').hidden = true; if (!e.target.closest('#acctmenu')) $('#acctmenu').hidden = true; });
$('#addbtn').onclick = () => menu([
  ['Open terminal for this agent', () => { if (S.chat !== '*') setMode('pty'); else toast('Open an agent chat first'); }],
  ['Create a task…', () => { openPanel('computer'); }],
  ['Issue enrollment code', issueCode],
  ['Open browser stream…', () => openPanel('computer')],
]);
$('#plusbtn').onclick = (e) => { e.stopPropagation(); menu([['New agent session…', () => { const n = (visibleNodes().find((x) => x.online) || {}).node_id; if (!n) { toast('No online node'); return; } openChat(n); setMode('pty'); }], ['Add a node (enrollment code)', () => { openPanel('status'); issueCode(); }]]); const m = $('#menu'); m.style.position = 'fixed'; m.style.left = '120px'; m.style.top = '54px'; m.style.bottom = 'auto'; };
// ---- account pill
async function loadMe() {
  try { const r = await fetch('/api/me', { credentials: 'same-origin' }); const d = await r.json(); const u = d.user || {}; S.me = u; const nm = u.display_name || u.username || 'Account';
    $('#acctav').innerHTML = avatar(u.username || nm, '', false, 'idle'); } catch (_) {}
}
const signOut = async () => { try { await fetch('/auth/logout', { method: 'POST', credentials: 'same-origin', headers: { 'content-type': 'application/json' }, body: '{}' }); } catch (_) {} location.href = '/'; };
$('#acctbtn').onclick = (e) => { e.stopPropagation(); const m = $('#acctmenu'); m.hidden = !m.hidden; if (m.hidden) return; const u = S.me || {}; m.className = 'pop amenu';
  m.innerHTML = `<div class="hd"><b>${esc(u.display_name || u.username || 'Account')}</b><span>@${esc(u.username || '')}</span></div>`;
  const it = (ic, label, fn, r) => { const b = document.createElement('button'); b.innerHTML = `<span class="ic">${ic}</span>${esc(label)}${r ? `<span class="r">${r}</span>` : ''}`; b.onclick = () => { m.hidden = true; fn(); }; m.appendChild(b); };
  it('◔', 'Usage & plan', () => openSettings('usage'), '›'); it('?', 'Support', () => toast('Support: coming soon'), '›'); it('⚙', 'Settings', () => openSettings('general'));
  m.insertAdjacentHTML('beforeend', '<hr>'); it('+', 'Add account', () => toast('Multiple accounts: coming soon')); it('⇥', 'Sign out', signOut); };
// ---- settings modal (stubs)
const SNAV = [['general', '⚙', 'General'], ['computer', '▭', 'Computer & nodes'], ['usage', '◔', 'Usage'], ['updates', '↻', 'Updates']];
function openSettings(sec) {
  let d = $('#settings'); if (!d) { d = document.createElement('div'); d.id = 'settings'; document.body.appendChild(d); d.onclick = (e) => { if (e.target === d) d.remove(); }; }
  const u = S.me || {}; const sel = (opts) => `<select>${opts.map((o) => `<option>${esc(o)}</option>`).join('')}</select>`; const sw = (on) => `<button class="sw${on ? ' on' : ''}"></button>`;
  const body = {
    general: `<h2>General</h2><h5>Account</h5><div class="scard"><div class="acc">${avatar(u.username || 'me', '', false, 'idle', 'dm')}<div class="who"><b>${esc(u.display_name || u.username || '')}</b><span>@${esc(u.username || '')}</span></div><button class="pbtn" id="sadd">Add account</button><button class="pbtn" id="sout">Sign out</button></div></div>
      <h5>Appearance</h5><div class="scard"><div class="srow">Theme${sel(['System', 'Dark'])}</div><div class="srow">Accent${sel(['Blue', 'Violet', 'Green'])}</div><div class="srow">Language${sel(['System', 'English', 'Русский'])}</div><div class="srow">Spellcheck while typing${sw(true)}</div></div>
      <h5>System</h5><div class="scard"><div class="srow">Microphone${sel(['System default'])}</div><div class="srow">Sounds${sw(false)}</div></div>
      <h5>HQ</h5><div class="scard"><div class="srow">Timezone${sel(['Auto (' + Intl.DateTimeFormat().resolvedOptions().timeZone + ')'])}</div></div>`,
    computer: `<h2>Computer & nodes</h2><div class="scard"><div class="srow">Nodes online<span>${S.nodes.filter((n) => n.online).length} / ${S.nodes.length}</span></div><div class="srow">Add a node<button class="pbtn" id="senr">Enrollment code</button></div></div>`,
    usage: '<h2>Usage</h2><div class="scard"><div class="srow">Plan<span>Operator</span></div><div class="srow">Traffic this month<span>—</span></div></div>',
    updates: '<h2>Updates</h2><div class="scard"><div class="srow">HQ web<span>feat/hq-web-ui-polish</span></div></div>',
  }[sec];
  d.innerHTML = `<div class="sdlg"><nav class="snav">${SNAV.map(([id, ic, l]) => `<button data-s="${id}" class="${id === sec ? 'on' : ''}"><span>${ic}</span>${l}</button>`).join('')}</nav><div class="sbody">${body}</div><button class="round sx" id="sclose">✕</button></div>`;
  d.querySelectorAll('[data-s]').forEach((b) => (b.onclick = () => openSettings(b.dataset.s)));
  d.querySelectorAll('.sw').forEach((b) => (b.onclick = () => b.classList.toggle('on')));
  $('#sclose').onclick = () => d.remove(); const so = $('#sout'); if (so) so.onclick = signOut; const sa = $('#sadd'); if (sa) sa.onclick = () => toast('Multiple accounts: coming soon');
  const se = $('#senr'); if (se) se.onclick = () => { d.remove(); openPanel('status'); issueCode(); };
}
document.addEventListener('keydown', (e) => { if (e.key === 'Escape' && $('#settings')) $('#settings').remove(); });
loadMe();
async function issueCode() {
  try {
    const d = await post('/api/c2/codes');
    S.code = d.code; logEv('enrollment code issued'); if (S.panel && S.tab === 'status') renderStatus(); else addLocal(S.chat, { card: `<b>Enrollment code</b><br><code class="big" id="codetext">${esc(d.code)}</code><br><span class="sub">Single use. Give it to the node operator; the node generates its own keys.</span>` });
  } catch (e) { toast('error: ' + e.message); }
}

// ---- nodes loading and change notices
async function loadNodes() {
  try {
    const d = await api('/api/nodes'); S.nodes = d.nodes; S.ready = !!d.ready;
    for (const n of S.nodes) {
      const key = n.status + '/' + (n.online ? 'on' : 'off'); const prev = S.lastStatus[n.node_id];
      if (prev && prev !== key) logEv(`${n.node_id}: ${prev.replace('/', ' · ')} → ${n.status} · ${n.online ? 'online' : 'offline'}`);
      else if (!prev) logEv(`${n.node_id} (${n.kind}) ${n.status} · ${n.online ? 'online' : 'offline'}`);
      S.lastStatus[n.node_id] = key;
    }
  } catch (_) { if (S.ready !== false) logEv('C2 unreachable'); S.ready = false; }
  renderSidebar(); if (S.panel && S.tab === 'status') renderStatus();
}

// ---- status: the one place for C2 state, node trust, policy and the event log
async function renderStatus() {
  const root = $('#p-status'); const on = S.nodes.filter((n) => n.online).length; let policy = '';
  try { policy = JSON.stringify(await api('/api/c2/policy'), null, 2); } catch (e) { policy = e.message; }
  root.innerHTML = `<div class="stgrid" style="grid-template-columns:1fr 1fr"><div class="stat"><div class="l">C2</div><div class="v"><span class="dot ${S.ready ? 'ok' : 'bad'}"></span>${S.ready ? 'Ready' : 'Not ready'}</div></div><div class="stat"><div class="l">Nodes online</div><div class="v">${on} / ${S.nodes.length}</div></div><div class="stat"><div class="l">Agent session</div><div class="v">${S.session ? esc(S.sessionProv + ' · ' + S.sessionNode) : 'None'}</div></div></div>`
    + `<div class="row"><button class="btn blue" id="nc">Issue enrollment code</button><button class="btn" id="pr">Reload policy</button></div>${S.code ? `<div class="pcard"><h4>Enrollment code</h4><code class="big">${esc(S.code)}</code><div class="sub" style="margin-top:6px">Single use. The node generates its own keys.</div></div>` : ''}`
    + `<h3>Nodes</h3>${S.nodes.map((n) => `<div class="ncard"><div class="nh"><b>${esc(n.node_id)}</b><span class="badge ${esc(n.status)}">${esc(n.status)}</span></div><div class="sub"><span class="dot ${n.online ? 'ok' : ''}"></span> ${n.online ? 'online' : esc(n.transport || 'offline')} · ${esc(n.kind)}${n.tunnel_ip ? ' · ' + esc(n.tunnel_ip) : ''}</div>${n.enrolled && n.status !== 'revoked' ? `<select data-id="${esc(n.node_id)}"><option value="">Set trust…</option>${['neutral', 'member', 'trusted', 'suspended'].filter((s) => s !== n.status).map((s) => `<option>${s}</option>`).join('')}<option value="revoke">revoke</option></select>` : ''}</div>`).join('')}`
    + `<h3>Events</h3><div class="pcard log">${S.log.slice().reverse().map((l) => `<div>${fmtTime(l.ts)}  ${esc(l.text)}</div>`).join('') || '<div>No events yet</div>'}</div>`
    + `<h3>Policy</h3><pre>${esc(policy)}</pre>`;
  $('#nc').onclick = issueCode; $('#pr').onclick = async () => { try { await post('/api/c2/policy/reload'); toast('Policy reloaded'); renderStatus(); } catch (e) { toast('Error: ' + e.message); } };
  root.querySelectorAll('select[data-id]').forEach((sel) => (sel.onchange = async () => {
    const id = sel.dataset.id, v = sel.value; if (!v) return;
    try { if (v === 'revoke') { if (!confirm('Revoke ' + id + '? This is final.')) { sel.value = ''; return; } await post(`/api/c2/nodes/${id}/revoke`); } else await post(`/api/c2/nodes/${id}/status`, { status: v }); toast(`${id} → ${v}`); await loadNodes(); } catch (e) { toast('Error: ' + e.message); }
  }));
}

// ---- right panel: chat info + all status / control
const TABS = [['info', 'Info'], ['status', 'Status'], ['library', 'Library'], ['computer', 'Computer']];
function renderPanelHead() { if (!S.chat) return; $('#panelav').innerHTML = avatar(S.chat, 'big'); $('#panelname').textContent = chatName(S.chat); $('#panelsub').textContent = ''; }
function openPanel(tab) { S.panel = true; $('#panel').classList.add('open'); syncHead(); renderPanelHead(); setTab(tab || S.tab); }
function closePanel() { S.panel = false; $('#panel').classList.remove('open'); syncHead(); }
// The pill lives inside the main pane (overflow hidden). On panel open/close it holds its old screen
// position briefly, then eases to the new centre; every frame is clamped to the main pane bounds.
let pillAnim = 0;
function syncHead() {
  document.body.classList.toggle('panelopen', !!S.panel);
  const b = $('#headbtn'); const from = b.getBoundingClientRect(); const fromC = (from.left + from.right) / 2 - (parseFloat(b.dataset.tx) || 0);
  const oldX = fromC + (parseFloat(b.dataset.tx) || 0); const t0 = performance.now(); const id = ++pillAnim;
  const step = (now) => {
    if (id !== pillAnim) return;
    b.style.transform = ''; const r = b.getBoundingClientRect(); const m = $('#main').getBoundingClientRect(); const nat = (r.left + r.right) / 2;
    const t = Math.max(0, Math.min(1, (now - t0 - 140) / 380)); const e = 1 - Math.pow(1 - t, 3);
    let want = oldX + (nat - oldX) * e; const half = r.width / 2; want = Math.max(m.left + 12 + half, Math.min(m.right - 12 - half, want));
    const tx = want - nat; b.dataset.tx = tx; b.style.transform = tx ? `translateX(${tx}px)` : '';
    if (t < 1) requestAnimationFrame(step); else { b.style.transform = ''; b.dataset.tx = 0; }
  };
  if (matchMedia('(prefers-reduced-motion: reduce)').matches) { b.style.transform = ''; return; }
  requestAnimationFrame(step);
}
// draggable separators; widths persist in localStorage
function setW(v, px) { document.documentElement.style.setProperty(v, px + 'px'); }
setW('--sw', +localStorage.getItem('hq.sw') || 258); setW('--pw', +localStorage.getItem('hq.pw') || 258);
function dragger(el, v, key, min, max, fromRight) {
  el.onpointerdown = (e) => {
    e.preventDefault(); el.setPointerCapture(e.pointerId); el.classList.add('drag'); document.body.classList.add('dragging');
    el.onpointermove = (ev) => { const w = Math.round(Math.max(min, Math.min(max(), fromRight ? innerWidth - ev.clientX : ev.clientX))); setW(v, w); localStorage.setItem(key, w); };
    el.onpointerup = () => { el.onpointermove = null; el.classList.remove('drag'); document.body.classList.remove('dragging'); };
  };
}
// left separator: shrink to 200, past the threshold snap to a 72px avatar rail; drag back out to expand
function setRail(on) { if (document.body.classList.contains('rail') === on) return; document.body.classList.add('snap'); document.body.classList.toggle('rail', on); localStorage.setItem('hq.rail', on ? '1' : ''); setTimeout(() => document.body.classList.remove('snap'), 220); renderSidebar(); }
if (localStorage.getItem('hq.rail')) document.body.classList.add('rail');
$('#rzl').onpointerdown = (e) => {
  const el = $('#rzl'); e.preventDefault(); el.setPointerCapture(e.pointerId); el.classList.add('drag'); document.body.classList.add('dragging');
  el.onpointermove = (ev) => { const x = ev.clientX; if (x < 140) { setRail(true); return; } setRail(false); const w = Math.round(Math.max(200, Math.min(Math.min(440, innerWidth * 0.35), x))); setW('--sw', w); localStorage.setItem('hq.sw', w); };
  el.onpointerup = () => { el.onpointermove = null; el.classList.remove('drag'); document.body.classList.remove('dragging'); };
};
dragger($('#rzr'), '--pw', 'hq.pw', 240, () => Math.min(640, innerWidth * 0.4), true);
function setTab(tab) {
  if (!TABS.some(([id]) => id === tab)) tab = 'info';
  S.tab = tab; $('#ptabs').innerHTML = '';
  for (const [id, label] of TABS) { const b = document.createElement('button'); b.textContent = label; b.className = id === tab ? 'on' : ''; b.onclick = () => setTab(id); $('#ptabs').appendChild(b); }
  document.querySelectorAll('.ptab').forEach((t) => t.classList.toggle('on', t.id === 'p-' + tab));
  if (tab === 'info') renderInfo(); if (tab === 'status') renderStatus(); if (tab === 'computer') renderTasksTab(); if (tab === 'library') renderLibraryTab();
}
function renderInfo() {
  const root = $('#p-info'); const agent = S.chat && S.chat !== '*'; const n = S.nodes.find((x) => x.node_id === S.chat);
  let h = `<div class="pcard">${agent ? `<div class="kv"><span>Session</span><span>${S.session && S.sessionNode === S.chat ? esc(S.sessionProv) + ' · live' : 'not running'}</span></div>` : '<div class="kv"><span>Room</span><span>room:general</span></div>'}${n ? `<div class="kv"><span>Node</span><span>${esc(n.node_id)} · ${n.online ? 'online' : 'offline'}</span></div>` : ''}</div>`;
  if (agent) h += `<div class="pcard"><h4>View</h4><div class="tog" id="vtog"><button data-m="chat" class="${S.mode === 'chat' ? 'on' : ''}">Chat</button><button data-m="pty" class="${S.mode === 'pty' ? 'on' : ''}">Terminal</button></div></div>`;
  root.innerHTML = h + '<div id="p-activity"></div>';
  root.querySelectorAll('#vtog button').forEach((b) => (b.onclick = () => { setMode(b.dataset.m); renderInfo(); }));
  if (agent) renderActivity();
}
$('#headbtn').onclick = () => { if (S.panel) closePanel(); else openPanel('info'); };
$('#closepanel').onclick = closePanel;

// ---- terminal tab
const COLS = 100, ROWS = 30;
const term = new Terminal({ rows: ROWS, cols: COLS, cursorBlink: true, fontSize: 13, fontFamily: 'ui-monospace,Menlo,Consolas,monospace', theme: { background: '#000000' } });
term.open($('#terminal'));
let mini = null;
function setMode(m) {
  S.mode = m; document.body.classList.toggle('ptymode', m === 'pty'); $('#ptyview').classList.toggle('on', m === 'pty');
  
  if (m === 'pty') term.focus();
}
let tws = null;
function tsend(o) { if (tws && tws.readyState === 1) tws.send(JSON.stringify(o)); }
$('#tspawn').onclick = () => {
  const node = S.chat && S.chat !== '*' ? S.chat : null; if (!node) { toast('Open an agent first'); return; }
  if (tws) tws.close();
  const opt = $('#tprov').selectedOptions[0]; const prov = opt.dataset.parser ? 'fixture' : opt.value; const q = new URLSearchParams({ node, provider: opt.value, rows: ROWS, cols: COLS }); if (opt.dataset.parser) q.set('parser', opt.dataset.parser);
  term.reset(); if (mini) mini.reset(); S.lastAnsi = ''; $('#tinfo').textContent = 'spawning…';
  tws = new WebSocket(`${location.protocol === 'https:' ? 'wss' : 'ws'}://${location.host}/api/term?${q}`);
  tws.onmessage = (m) => {
    const d = JSON.parse(m.data);
    if (d.type === 'spawned') { S.events[node] = new Map(); S.agentState[node] = 'idle'; S.session = d.session; S.sessionNode = node; S.sessionProv = prov; S.target = 'agent'; $('#tinfo').textContent = `${prov} on ${node}`; logEv(`agent ${prov} started on ${node}`); if (S.chat === node) openChat(node); }
    if (d.type === 'error') { $('#tinfo').textContent = d.text; addLocal(node, { sys: true, text: d.text }); }
    if (d.type === 'ack') { $('#tinfo').textContent = `${d.op}: ${d.ok ? 'ok' : 'failed'}`; if (!d.ok) { S.working[node] = 0; addLocal(node, { sys: true, text: 'Refused: ' + (/does not permit/.test(d.detail) ? 'node status does not permit operator requests (policy)' : d.detail.slice(0, 120)) }); } }
    if (d.type === 'events') onEvents(node, d.events);
    if (d.type === 'frame') {
      const bytes = Uint8Array.from(atob(d.ansi), (c) => c.charCodeAt(0));
      window.lastFrameText = d.text; term.reset(); term.write(bytes); S.lastAnsi = bytes; if (mini) { mini.reset(); mini.write(bytes); }
      S.lastFrame = Date.now();
    }
  };
  tws.onclose = () => { $('#tinfo').textContent += ' [closed]'; if (S.sessionNode) { logEv(`agent session on ${S.sessionNode} ended`); } S.session = null; if (S.sessionNode) S.agentState[S.sessionNode] = 'idle'; S.target = 'mail'; renderChip(); };
};
$('#tstop').onclick = () => tsend({ t: 'stop' });
term.onData((data) => tsend({ t: 'data', data }));

// ---- event layer: the browser reads ONLY typed session events from the server (no screen parsing here)
function refreshAvatars() { renderSidebar(); if (S.chat && S.chat !== '*') $('#headav').innerHTML = avatar(S.chat, 'mid'); renderChat(); }
function onEvents(node, evs) {
  const m = (S.events[node] ||= new Map()); let chatChanged = false; let stateChanged = false;
  for (const e of evs) {
    const prev = m.get(e.id);
    m.set(e.id, { ...e, at: prev ? prev.at : Date.now() });
    if (e.kind === 'state') { if (S.agentState[node] !== e.state) { S.agentState[node] = e.state; stateChanged = true; } if (e.state !== 'working') S.working[node] = 0; }
    if (e.kind === 'thought' && !prev) { stateChanged = true; setTimeout(refreshAvatars, 5200); }
    if (e.kind === 'answer' || e.kind === 'prompt_needed') chatChanged = true;
    if (e.kind === 'answer') S.working[node] = 0;
  }
  window.lastEvents = Object.fromEntries(Object.entries(S.events).map(([k, v]) => [k, [...v.values()]]));
  if (chatChanged || stateChanged) renderChat();
  if (stateChanged) refreshAvatars();
  if (S.panel && S.tab === 'info') renderInfo();
}
const confTag = (e) => `<span class="tagh" title="source ${esc(e.source)}">${e.confidence === 'structured' ? 'structured' : 'heuristic'} · ${esc(e.source)}</span>`;
function diffHtml(t) { return t.split('\n').map((l) => `<span class="${l.startsWith('+') ? 'd-add' : l.startsWith('-') ? 'd-del' : l.startsWith('@@') ? 'd-hunk' : ''}">${esc(l)}</span>`).join('\n'); }
function renderActivity() {
  const root = $('#p-activity'); const node = !S.chat || S.chat === '*' ? S.sessionNode : S.chat; const evs = node && S.events[node] ? [...S.events[node].values()] : [];
  const st = node ? S.agentState[node] || 'idle' : 'idle';
  let h = `<div class="sub" style="margin-bottom:8px">Agent activity${node ? ' on ' + esc(node) : ''} · state <b>${esc(st)}</b> <span class="tagh">from the session event layer</span></div>`;
  h += evs.filter((e) => ['thought', 'action', 'diff', 'prompt_needed'].includes(e.kind)).slice(-14).reverse().map((e) => {
    if (e.kind === 'thought') return `<div class="wd thought"><div class="k">💭 Thought ${confTag(e)}</div><div class="b">${esc(e.text)}</div></div>`;
    if (e.kind === 'action') return `<div class="wd tool"><div class="k">⚙ Action · ${esc(e.status)} ${confTag(e)}</div><div class="b">${esc(e.name)}(${esc(e.args)})</div>${e.result ? `<div class="r">⎿ ${esc(e.result)}</div>` : ''}</div>`;
    if (e.kind === 'diff') return `<div class="wd diff"><div class="k">± Git diff · ${esc(e.files.join(', '))} ${confTag(e)}</div><pre>${diffHtml(e.patch)}</pre></div>`;
    return `<div class="wd attn"><div class="k">⚠ Needs input · ${esc(e.prompt)} ${confTag(e)}</div><div class="b">${esc(e.text)}</div>${e.options.length ? `<div class="r">${e.options.map(esc).join('<br>')}</div>` : ''}</div>`;
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
  bws = new WebSocket(`${location.protocol === 'https:' ? 'wss' : 'ws'}://${location.host}/api/browser?session=${encodeURIComponent($('#bsess').value.trim())}`); bws.binaryType = 'blob';
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
    logEv(`mail from ${msg.from}`);
    S.working[msg.from] = 0;
    if (S.chat === msg.from || S.chat === '*') loadThread(); else { S.mail[msg.from] = (S.mail[msg.from] || []).concat([msg]); renderSidebar(); }
  }
};
setTab('info');
loadNodes().then(() => { openChat('*'); }); setInterval(loadNodes, 4000);
