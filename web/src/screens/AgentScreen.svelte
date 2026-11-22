<script>
  import { onMount } from 'svelte';
  import { api } from '../lib/api.js';

  let { me } = $props();

  let view = $state('chat'); // 'chat' | 'projects' | 'market' | 'keys'
  let models = $state([]);
  let skills = $state([]);
  let marketScope = $state('');
  let selectedModel = $state('roost/local-assistant');
  let providers = $state([]);
  let messages = $state([]);
  let input = $state('');
  let sending = $state(false);
  let error = $state('');

  // Claves de proveedores
  let keyProvider = $state('openrouter');
  let keySecret = $state('');
  let keyBaseUrl = $state('');
  let keySaved = $state('');

  // Proyectos de agentes locales 24/7 (Antigravity / Claude)
  let projects = $state([]);
  let pName = $state('');
  let pRunner = $state('antigravity');
  let pWorkdir = $state('');
  let pCommand = $state('');
  let pAutoRestart = $state(true);
  let activeLogs = $state(null);

  let currentModelObj = $derived(models.find((m) => m.id === selectedModel));

  async function loadAll() {
    try {
      const [m, p, sk] = await Promise.all([
        api.marketModels(marketScope),
        api.aiProviders(),
        api.marketSkills(),
      ]);
      models = m;
      providers = p;
      skills = sk;
      if (m.length && !m.some((x) => x.id === selectedModel)) {
        selectedModel = m[0].id;
      }
      if (me.role === 'admin') {
        projects = await api.aiProjects();
      }
    } catch (err) {
      error = err.message;
    }
  }

  onMount(loadAll);

  async function sendMessage(event) {
    event.preventDefault();
    const text = input.trim();
    if (!text || sending) return;
    error = '';
    input = '';
    sending = true;

    const assistantMsg = {
      role: 'assistant',
      content: '',
      reasoning: '',
      action: null,
    };
    messages = [...messages, { role: 'user', content: text }, assistantMsg];
    const idx = messages.length - 1;

    try {
      await api.aiChatStream(selectedModel, text, (ev) => {
        const next = [...messages];
        const target = { ...next[idx] };
        if (ev.type === 'reasoning') target.reasoning += ev.text;
        if (ev.type === 'content') target.content += ev.text;
        if (ev.type === 'action') target.action = ev.action;
        next[idx] = target;
        messages = next;
      });
    } catch (err) {
      error = err.message;
    } finally {
      sending = false;
    }
  }

  async function decideAction(msgIdx, actionId, approve) {
    error = '';
    try {
      const updated = approve
        ? await api.approveAction(actionId)
        : await api.rejectAction(actionId);
      const next = [...messages];
      next[msgIdx] = { ...next[msgIdx], action: updated };
      messages = next;
    } catch (err) {
      error = err.message;
    }
  }

  async function saveKey(event) {
    event.preventDefault();
    error = '';
    keySaved = '';
    try {
      await api.saveAiProvider(keyProvider, keySecret, keyBaseUrl);
      keySecret = '';
      keySaved = 'Clave guardada y cifrada en el servidor.';
      await loadAll();
    } catch (err) {
      error = err.message;
    }
  }

  async function createProject(event) {
    event.preventDefault();
    error = '';
    try {
      await api.createAiProject({
        name: pName,
        runner: pRunner,
        workdir: pWorkdir,
        command: pCommand || null,
        auto_restart: pAutoRestart,
      });
      pName = '';
      pWorkdir = '';
      pCommand = '';
      projects = await api.aiProjects();
    } catch (err) {
      error = err.message;
    }
  }

  async function toggleProject(project) {
    error = '';
    try {
      if (project.running) {
        await api.stopAiProject(project.id);
      } else {
        await api.startAiProject(project.id);
      }
      projects = await api.aiProjects();
    } catch (err) {
      error = err.message;
    }
  }

  async function openLogs(project) {
    const res = await api.aiProjectLogs(project.id);
    activeLogs = { name: project.name, lines: res.lines || [] };
  }

  async function removeProject(id) {
    await api.deleteAiProject(id);
    projects = await api.aiProjects();
  }

  async function installSkill(skill) {
    error = '';
    try {
      await api.installSkill(skill.id, skill.permissions);
      skills = await api.marketSkills();
    } catch (err) {
      error = err.message;
    }
  }

  async function uninstallSkill(id) {
    error = '';
    try {
      await api.uninstallSkill(id);
      skills = await api.marketSkills();
    } catch (err) {
      error = err.message;
    }
  }
</script>

<section class="page" aria-labelledby="agent-title">
  <header class="head">
    <div>
      <h1 id="agent-title">Agente de IA</h1>
      <p class="muted">Asistente de tu servidor y proyectos de agentes locales 24/7.</p>
    </div>
    <div class="tabs" role="tablist">
      <button type="button" class="btn" class:btn-tonal={view === 'chat'} class:btn-outlined={view !== 'chat'} onclick={() => (view = 'chat')}>
        Chat
      </button>
      {#if me.role === 'admin'}
        <button type="button" class="btn" class:btn-tonal={view === 'projects'} class:btn-outlined={view !== 'projects'} onclick={() => (view = 'projects')}>
          Proyectos 24/7
        </button>
      {/if}
      <button type="button" class="btn" class:btn-tonal={view === 'market'} class:btn-outlined={view !== 'market'} onclick={() => (view = 'market')}>
        Marketplace
      </button>
      <button type="button" class="btn" class:btn-tonal={view === 'keys'} class:btn-outlined={view !== 'keys'} onclick={() => (view = 'keys')}>
        Proveedores y claves
      </button>
    </div>
  </header>

  {#if error}
    <div class="alert" role="alert">{error}</div>
  {/if}

  {#if view === 'chat'}
    <div class="model-bar card">
      <div class="field" style="flex:1">
        <label for="ai-model">Modelo</label>
        <select id="ai-model" bind:value={selectedModel}>
          {#each models as m (m.id)}
            <option value={m.id}>
              {m.name} ({m.is_cloud ? 'Nube' : 'Local'})
            </option>
          {/each}
        </select>
      </div>
      {#if currentModelObj?.is_cloud}
        <div class="alert" role="note">Esta conversación sale de tu casa.</div>
      {:else}
        <div class="alert alert-info" role="note">Procesamiento local en tu casa.</div>
      {/if}
    </div>

    <div class="chat-box card">
      {#if messages.length === 0}
        <p class="muted">
          Probá escribir: <strong>"instalá algo para ver películas"</strong> o <strong>"¿por qué está lento?"</strong>
        </p>
      {/if}
      {#each messages as msg, i}
        <div class="msg" class:msg-user={msg.role === 'user'}>
          <strong>{msg.role === 'user' ? me.name : 'Agente Roost'}</strong>
          {#if msg.reasoning}
            <details class="reasoning">
              <summary>Ver razonamiento</summary>
              <p>{msg.reasoning}</p>
            </details>
          {/if}
          <p class="msg-text">{msg.content}</p>
          {#if msg.action}
            <div class="action-card">
              <div>
                <strong>Acción propuesta:</strong> {msg.action.summary}
                <div class="muted">Estado: {msg.action.status}{msg.action.result ? ` · ${msg.action.result}` : ''}</div>
              </div>
              {#if msg.action.status === 'pending'}
                <div class="action-btns">
                  <button class="btn btn-primary" onclick={() => decideAction(i, msg.action.id, true)}>Aprobar</button>
                  <button class="btn btn-outlined" onclick={() => decideAction(i, msg.action.id, false)}>Rechazar</button>
                </div>
              {/if}
            </div>
          {/if}
        </div>
      {/each}
    </div>

    <form class="chat-input" onsubmit={sendMessage}>
      <label for="chat-msg" class="sr-only">Mensaje para el agente</label>
      <input
        id="chat-msg"
        bind:value={input}
        placeholder="Escribí qué necesitás (ej. instalá algo para ver películas)..."
        disabled={sending}
      />
      <button class="btn btn-primary" type="submit" disabled={sending}>
        {sending ? 'Pensando...' : 'Enviar'}
      </button>
    </form>
  {:else if view === 'projects'}
    <article class="card">
      <h2>Proyectos de Antigravity y Claude (24/7)</h2>
      <p class="muted">
        Corré tus proyectos locales de Antigravity (`agy`), Claude Code (`claude`) o scripts propios 24/7 con reinicio automático y logs en vivo.
      </p>

      <form class="proj-form" onsubmit={createProject}>
        <div class="field">
          <label for="p-name">Nombre</label>
          <input id="p-name" required bind:value={pName} placeholder="Mi bot de Antigravity" />
        </div>
        <div class="field">
          <label for="p-runner">Agente</label>
          <select id="p-runner" bind:value={pRunner}>
            <option value="antigravity">Antigravity (agy)</option>
            <option value="claude">Claude Code (claude)</option>
            <option value="custom">Comando propio</option>
          </select>
        </div>
        <div class="field">
          <label for="p-dir">Carpeta del proyecto</label>
          <input id="p-dir" required bind:value={pWorkdir} placeholder="/Users/franco/proyecto" />
        </div>
        <div class="field">
          <label for="p-cmd">Comando (opcional)</label>
          <input id="p-cmd" bind:value={pCommand} placeholder="Dejalo vacío para usar el comando por defecto" />
        </div>
        <label class="check-row">
          <input type="checkbox" bind:checked={pAutoRestart} />
          Mantener encendido 24/7 (reiniciar automáticamente si se detiene)
        </label>
        <button class="btn btn-primary" type="submit">Agregar proyecto</button>
      </form>
    </article>

    <div class="proj-list">
      {#each projects as proj (proj.id)}
        <article class="card proj-card">
          <div>
            <strong>{proj.name}</strong> · <span class="muted">{proj.runner}</span>
            <div class="muted">{proj.workdir} — <code>{proj.command}</code></div>
            <div class="muted">{proj.running ? 'Activo 24/7' : 'Detenido'}</div>
          </div>
          <div class="action-btns">
            <button class="btn btn-tonal" onclick={() => toggleProject(proj)}>
              {proj.running ? 'Detener' : 'Iniciar 24/7'}
            </button>
            <button class="btn btn-outlined" onclick={() => openLogs(proj)}>Ver logs</button>
            <button class="btn btn-danger" onclick={() => removeProject(proj.id)}>Quitar</button>
          </div>
        </article>
      {/each}
    </div>

    {#if activeLogs}
      <article class="card">
        <h3>Logs recientes: {activeLogs.name}</h3>
        <pre class="log-box">{activeLogs.lines.join('\n') || '(sin salida todavía)'}</pre>
      </article>
    {/if}
  {:else if view === 'market'}
    <article class="card">
      <h2>Marketplace de modelos</h2>
      <div class="tabs" style="margin-bottom:12px">
        <button class="btn" class:btn-tonal={marketScope === ''} class:btn-outlined={marketScope !== ''} onclick={() => { marketScope = ''; loadAll(); }}>Todos</button>
        <button class="btn" class:btn-tonal={marketScope === 'local'} class:btn-outlined={marketScope !== 'local'} onclick={() => { marketScope = 'local'; loadAll(); }}>Local</button>
        <button class="btn" class:btn-tonal={marketScope === 'cloud'} class:btn-outlined={marketScope !== 'cloud'} onclick={() => { marketScope = 'cloud'; loadAll(); }}>Nube</button>
      </div>
      <ul class="prov-list">
        {#each models as m (m.id)}
          <li>
            <strong>{m.name}</strong> · {m.is_cloud ? 'Nube' : 'Local'} · {m.is_free ? 'Gratis' : 'Pago'}
            {#if m.fits_ram === true} · <span>Entra en tu RAM</span>{/if}
            {#if m.fits_ram === false} · <span>Requiere más RAM libre</span>{/if}
          </li>
        {/each}
      </ul>
    </article>

    <article class="card">
      <h2>Habilidades del agente</h2>
      <div class="proj-list">
        {#each skills as sk (sk.id)}
          <div class="action-card">
            <div>
              <strong>{sk.name}</strong> <span class="muted">v{sk.version}</span>
              <p class="muted">{sk.description}</p>
              <div class="muted">Permisos: {sk.permissions_es.join(' · ')}</div>
            </div>
            {#if me.role === 'admin'}
              {#if sk.installed}
                <button class="btn btn-danger" onclick={() => uninstallSkill(sk.id)}>Desinstalar</button>
              {:else}
                <button class="btn btn-primary" onclick={() => installSkill(sk)}>Aprobar e instalar</button>
              {/if}
            {/if}
          </div>
        {/each}
      </div>
    </article>
  {:else}
    <article class="card">
      <h2>Proveedores y claves cifradas</h2>
      <p class="muted">Cada persona carga su propia clave. Roost las cifra con ChaCha20-Poly1305 y nunca muestra la clave completa.</p>
      {#if keySaved}
        <div class="alert alert-info">{keySaved}</div>
      {/if}
      <ul class="prov-list">
        {#each providers as p (p.provider)}
          <li>
            <strong>{p.provider}</strong> ({p.is_cloud ? 'Nube' : 'Local'}) ·
            {p.key_last4 ? `Clave terminada en ****${p.key_last4}` : p.configured ? 'Listo' : 'Sin clave'}
          </li>
        {/each}
      </ul>
      <form class="proj-form" onsubmit={saveKey}>
        <div class="field">
          <label for="k-prov">Proveedor</label>
          <select id="k-prov" bind:value={keyProvider}>
            <option value="ollama">Ollama (local)</option>
            <option value="openrouter">OpenRouter (nube)</option>
            <option value="hackclub">Hack Club AI (nube)</option>
          </select>
        </div>
        <div class="field">
          <label for="k-sec">Clave API</label>
          <input id="k-sec" type="password" bind:value={keySecret} placeholder="Pegá tu clave privada" />
        </div>
        <div class="field">
          <label for="k-url">URL base (opcional)</label>
          <input id="k-url" bind:value={keyBaseUrl} placeholder="http://localhost:11434" />
        </div>
        <button class="btn btn-primary" type="submit">Guardar cifrada</button>
      </form>
    </article>
  {/if}
</section>

<style>
  .page { display: flex; flex-direction: column; gap: 16px; }
  .head { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: 12px; }
  h1 { margin: 0; font-size: 1.75rem; font-weight: 500; }
  h2 { margin: 0 0 8px; font-size: 1.125rem; font-weight: 500; }
  .muted { color: var(--md-on-surface-variant); margin: 4px 0 0; }
  .tabs { display: flex; flex-wrap: wrap; gap: 8px; }
  .model-bar { display: flex; flex-wrap: wrap; align-items: flex-end; gap: 12px; }
  .chat-box { display: flex; flex-direction: column; gap: 14px; min-height: 240px; }
  .msg { padding: 12px 16px; border-radius: var(--radius-md); background: var(--md-surface-container-high); }
  .msg-user { background: var(--md-primary-container); color: var(--md-on-primary-container); align-self: flex-end; max-width: 85%; }
  .msg-text { margin: 6px 0 0; line-height: 1.5; white-space: pre-wrap; }
  .reasoning { margin-top: 6px; font-size: 0.875rem; color: var(--md-on-surface-variant); }
  .action-card { margin-top: 12px; padding: 12px; border-radius: var(--radius-sm); background: var(--md-surface); border: 1px solid var(--md-outline-variant); display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: 10px; }
  .action-btns { display: flex; flex-wrap: wrap; gap: 8px; }
  .chat-input { display: flex; gap: 10px; }
  .chat-input input { flex: 1; min-height: 48px; padding: 0 16px; border-radius: var(--radius-pill); border: 1px solid var(--md-outline-variant); background: var(--md-surface-container); }
  .proj-form { display: grid; grid-template-columns: repeat(auto-fit, minmax(220px, 1fr)); gap: 12px; margin-top: 12px; align-items: end; }
  .check-row { grid-column: 1 / -1; display: flex; align-items: center; gap: 8px; min-height: 44px; }
  .proj-list { display: flex; flex-direction: column; gap: 12px; }
  .proj-card { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: 12px; }
  .log-box { background: #111; color: #e2e8f0; padding: 12px; border-radius: var(--radius-sm); overflow-x: auto; max-height: 260px; font-size: 0.8125rem; }
  .prov-list { list-style: none; padding: 0; margin: 12px 0; display: flex; flex-direction: column; gap: 6px; }
</style>
