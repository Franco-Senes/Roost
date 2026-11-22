<script>
  import { onMount, tick } from 'svelte';
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
  let chatScrollEl = $state(null);

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

  const suggestions = [
    { title: 'Ver películas en la tele', prompt: 'Instalá algo para ver películas' },
    { title: 'Revisar el servidor', prompt: '¿Por qué está lento el servidor?' },
    { title: 'Bloquear anuncios', prompt: 'Quiero bloquear anuncios en toda la casa' },
    { title: 'Mis fotos recientes', prompt: 'Buscá mis fotos recientes' },
  ];

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

  async function scrollToBottom() {
    await tick();
    if (chatScrollEl) {
      chatScrollEl.scrollTop = chatScrollEl.scrollHeight;
    }
  }

  async function sendPrompt(text) {
    const clean = text.trim();
    if (!clean || sending) return;
    error = '';
    input = '';
    sending = true;

    const assistantMsg = {
      role: 'assistant',
      content: '',
      reasoning: '',
      action: null,
      streaming: true,
    };
    messages = [...messages, { role: 'user', content: clean }, assistantMsg];
    const idx = messages.length - 1;
    await scrollToBottom();

    try {
      await api.aiChatStream(selectedModel, clean, async (ev) => {
        const next = [...messages];
        const target = { ...next[idx] };
        if (ev.type === 'reasoning') target.reasoning += ev.text;
        if (ev.type === 'content') target.content += ev.text;
        if (ev.type === 'action') target.action = ev.action;
        if (ev.type === 'done') target.streaming = false;
        next[idx] = target;
        messages = next;
        await scrollToBottom();
      });
    } catch (err) {
      error = err.message;
    } finally {
      const next = [...messages];
      if (next[idx]) next[idx] = { ...next[idx], streaming: false };
      messages = next;
      sending = false;
    }
  }

  function handleSubmit(event) {
    event.preventDefault();
    sendPrompt(input);
  }

  function handleKeydown(event) {
    if (event.key === 'Enter' && !event.shiftKey) {
      event.preventDefault();
      sendPrompt(input);
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

<section class="agent-shell" aria-labelledby="agent-title">
  <header class="top-bar">
    <div class="top-left">
      <div class="ai-logo" aria-hidden="true">✦</div>
      <div>
        <h1 id="agent-title">Asistente Roost</h1>
        <span class="badge-mode" class:badge-cloud={currentModelObj?.is_cloud}>
          {currentModelObj?.is_cloud ? 'Nube · Esta conversación sale de tu casa' : 'Local · En tu casa'}
        </span>
      </div>
    </div>

    <div class="segmented" role="tablist" aria-label="Secciones del Agente">
      <button type="button" class="seg-btn" class:seg-active={view === 'chat'} onclick={() => (view = 'chat')}>
        Chat
      </button>
      {#if me.role === 'admin'}
        <button type="button" class="seg-btn" class:seg-active={view === 'projects'} onclick={() => (view = 'projects')}>
          Proyectos 24/7
        </button>
      {/if}
      <button type="button" class="seg-btn" class:seg-active={view === 'market'} onclick={() => (view = 'market')}>
        Marketplace
      </button>
      <button type="button" class="seg-btn" class:seg-active={view === 'keys'} onclick={() => (view = 'keys')}>
        Claves
      </button>
    </div>
  </header>

  {#if error}
    <div class="alert" role="alert">{error}</div>
  {/if}

  {#if view === 'chat'}
    <div class="chat-workspace">
      <div class="chat-toolbar">
        <label for="ai-model" class="sr-only">Modelo de IA</label>
        <select id="ai-model" class="model-pill" bind:value={selectedModel}>
          {#each models as m (m.id)}
            <option value={m.id}>
              {m.name} ({m.is_cloud ? 'Nube' : 'Local'})
            </option>
          {/each}
        </select>
        {#if messages.length > 0}
          <button type="button" class="btn btn-outlined clear-btn" onclick={() => (messages = [])}>
            Nuevo chat
          </button>
        {/if}
      </div>

      <div class="chat-feed" bind:this={chatScrollEl}>
        {#if messages.length === 0}
          <div class="welcome">
            <div class="welcome-orb" aria-hidden="true">✦</div>
            <h2>¿En qué te ayudo hoy, {me.name}?</h2>
            <p class="muted">
              Podés pedirme que instale apps, revisar el estado del servidor o buscar tus fotos.
            </p>
            <div class="suggestions">
              {#each suggestions as s}
                <button type="button" class="suggestion-card" onclick={() => sendPrompt(s.prompt)}>
                  <strong>{s.title}</strong>
                  <span>{s.prompt}</span>
                </button>
              {/each}
            </div>
          </div>
        {:else}
          <div class="thread">
            {#each messages as msg, i}
              <div class="bubble-row" class:row-user={msg.role === 'user'}>
                {#if msg.role !== 'user'}
                  <div class="avatar avatar-ai" aria-hidden="true">✦</div>
                {/if}
                <div class="bubble" class:bubble-user={msg.role === 'user'} class:bubble-ai={msg.role !== 'user'}>
                  {#if msg.reasoning}
                    <details class="reasoning">
                      <summary>Ver razonamiento</summary>
                      <p>{msg.reasoning}</p>
                    </details>
                  {/if}
                  {#if msg.streaming && !msg.content}
                    <div class="typing" aria-label="Pensando">
                      <span></span><span></span><span></span>
                    </div>
                  {:else}
                    <p class="msg-text">{msg.content}</p>
                  {/if}
                  {#if msg.action}
                    <div class="action-card">
                      <div>
                        <strong>{msg.action.summary}</strong>
                        <div class="muted">
                          Estado: {msg.action.status}{msg.action.result ? ` · ${msg.action.result}` : ''}
                        </div>
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
                {#if msg.role === 'user'}
                  <div class="avatar avatar-user" aria-hidden="true">
                    {me.name.slice(0, 1).toUpperCase()}
                  </div>
                {/if}
              </div>
            {/each}
          </div>
        {/if}
      </div>

      <form class="composer" onsubmit={handleSubmit}>
        <label for="chat-msg" class="sr-only">Mensaje para el asistente</label>
        <textarea
          id="chat-msg"
          rows="1"
          bind:value={input}
          onkeydown={handleKeydown}
          placeholder="Preguntale a Roost… (Enter para enviar)"
          disabled={sending}
        ></textarea>
        <button
          class="send-btn"
          type="submit"
          aria-label="Enviar mensaje"
          disabled={sending || !input.trim()}
        >
          <svg width="20" height="20" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
            <path d="M2.01 21L23 12 2.01 3 2 10l15 2-15 2z" />
          </svg>
        </button>
      </form>
    </div>
  {:else if view === 'projects'}
    <div class="panel-scroll">
      <article class="card">
        <h2>Proyectos de Antigravity y Claude (24/7)</h2>
        <p class="muted">
          Corré tus proyectos locales de Antigravity (<code>agy</code>), Claude Code (<code>claude</code>) o scripts propios 24/7 con reinicio automático y logs en vivo.
        </p>

        <form class="proj-form" onsubmit={createProject}>
          <div class="field">
            <label for="p-name">Nombre</label>
            <input id="p-name" required bind:value={pName} placeholder="Mi proyecto de Antigravity" />
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
            <input id="p-cmd" bind:value={pCommand} placeholder="Vacío usa el comando por defecto" />
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
    </div>
  {:else if view === 'market'}
    <div class="panel-scroll">
      <article class="card">
        <h2>Marketplace de modelos</h2>
        <div class="action-btns" style="margin-bottom:12px">
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
    </div>
  {:else}
    <div class="panel-scroll">
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
    </div>
  {/if}
</section>

<style>
  .agent-shell {
    display: flex;
    flex-direction: column;
    height: calc(100vh - 48px);
    gap: 14px;
  }
  .top-bar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }
  .top-left {
    display: flex;
    align-items: center;
    gap: 12px;
  }
  .ai-logo {
    width: 42px;
    height: 42px;
    border-radius: 14px;
    background: var(--md-primary-container);
    color: var(--md-on-primary-container);
    display: grid;
    place-items: center;
    font-size: 1.25rem;
  }
  h1 { margin: 0; font-size: 1.375rem; font-weight: 500; }
  h2 { margin: 0 0 8px; font-size: 1.25rem; font-weight: 500; }
  .badge-mode {
    display: inline-block;
    font-size: 0.75rem;
    padding: 2px 10px;
    border-radius: var(--radius-pill);
    background: var(--md-secondary-container);
    color: var(--md-on-secondary-container);
    margin-top: 2px;
  }
  .badge-cloud {
    background: var(--md-error-container);
    color: var(--md-on-error-container);
  }
  .segmented {
    display: inline-flex;
    background: var(--md-surface-container-high);
    padding: 4px;
    border-radius: var(--radius-pill);
    gap: 2px;
  }
  .seg-btn {
    min-height: 38px;
    padding: 0 14px;
    border: none;
    border-radius: var(--radius-pill);
    background: transparent;
    color: var(--md-on-surface-variant);
    cursor: pointer;
    font-weight: 500;
    font-size: 0.875rem;
  }
  .seg-active {
    background: var(--md-primary);
    color: var(--md-on-primary);
  }

  .chat-workspace {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    background: var(--md-surface-container);
    border-radius: var(--radius-lg);
    overflow: hidden;
    border: 1px solid var(--md-outline-variant);
  }
  .chat-toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    padding: 10px 16px;
    border-bottom: 1px solid var(--md-outline-variant);
    background: var(--md-surface-container-high);
  }
  .model-pill {
    min-height: 38px;
    max-width: 340px;
    padding: 0 14px;
    border-radius: var(--radius-pill);
    border: 1px solid var(--md-outline-variant);
    background: var(--md-surface);
    font-size: 0.875rem;
  }
  .clear-btn { min-height: 36px; padding: 0 14px; font-size: 0.875rem; }

  .chat-feed {
    flex: 1;
    overflow-y: auto;
    padding: 24px 20px;
    display: flex;
    flex-direction: column;
  }
  .welcome {
    margin: auto;
    max-width: 640px;
    text-align: center;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
  }
  .welcome-orb {
    width: 56px;
    height: 56px;
    border-radius: 18px;
    background: var(--md-primary-container);
    color: var(--md-on-primary-container);
    display: grid;
    place-items: center;
    font-size: 1.75rem;
  }
  .suggestions {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
    gap: 10px;
    width: 100%;
    margin-top: 12px;
  }
  .suggestion-card {
    text-align: left;
    padding: 14px 16px;
    border-radius: var(--radius-md);
    border: 1px solid var(--md-outline-variant);
    background: var(--md-surface);
    cursor: pointer;
    display: flex;
    flex-direction: column;
    gap: 4px;
    transition: background var(--transition-fast);
  }
  .suggestion-card:hover { background: var(--md-surface-container-high); }
  .suggestion-card span { font-size: 0.8125rem; color: var(--md-on-surface-variant); }

  .thread {
    display: flex;
    flex-direction: column;
    gap: 16px;
    max-width: 780px;
    width: 100%;
    margin: 0 auto;
  }
  .bubble-row {
    display: flex;
    align-items: flex-end;
    gap: 10px;
  }
  .row-user { justify-content: flex-end; }
  .avatar {
    width: 34px;
    height: 34px;
    border-radius: 50%;
    display: grid;
    place-items: center;
    flex-shrink: 0;
    font-weight: 600;
    font-size: 0.875rem;
  }
  .avatar-ai {
    background: var(--md-primary);
    color: var(--md-on-primary);
  }
  .avatar-user {
    background: var(--md-secondary-container);
    color: var(--md-on-secondary-container);
  }
  .bubble {
    max-width: min(82%, 620px);
    padding: 12px 16px;
    border-radius: 20px;
    line-height: 1.5;
  }
  .bubble-ai {
    background: var(--md-surface);
    border: 1px solid var(--md-outline-variant);
    border-bottom-left-radius: 6px;
  }
  .bubble-user {
    background: var(--md-primary-container);
    color: var(--md-on-primary-container);
    border-bottom-right-radius: 6px;
  }
  .msg-text { margin: 0; white-space: pre-wrap; }
  .reasoning {
    margin-bottom: 8px;
    padding: 6px 10px;
    border-radius: var(--radius-sm);
    background: var(--md-surface-container-high);
    font-size: 0.8125rem;
    color: var(--md-on-surface-variant);
  }
  .reasoning summary { cursor: pointer; font-weight: 500; }
  .reasoning p { margin: 6px 0 0; }

  .typing { display: inline-flex; gap: 5px; padding: 4px 2px; }
  .typing span {
    width: 7px; height: 7px; border-radius: 50%;
    background: var(--md-on-surface-variant);
    animation: pulse 1s infinite ease-in-out;
  }
  .typing span:nth-child(2) { animation-delay: 0.15s; }
  .typing span:nth-child(3) { animation-delay: 0.3s; }
  @keyframes pulse {
    0%, 80%, 100% { opacity: 0.3; transform: scale(0.85); }
    40% { opacity: 1; transform: scale(1); }
  }

  .composer {
    display: flex;
    align-items: center;
    gap: 10px;
    margin: 12px 16px 16px;
    padding: 6px 8px 6px 18px;
    border-radius: 28px;
    background: var(--md-surface);
    border: 1px solid var(--md-outline-variant);
  }
  .composer textarea {
    flex: 1;
    border: none;
    background: transparent;
    resize: none;
    outline: none;
    font: inherit;
    padding: 8px 0;
    max-height: 120px;
  }
  .send-btn {
    width: 44px;
    height: 44px;
    border-radius: 50%;
    border: none;
    background: var(--md-primary);
    color: var(--md-on-primary);
    display: grid;
    place-items: center;
    cursor: pointer;
    flex-shrink: 0;
  }
  .send-btn:disabled { opacity: 0.45; cursor: not-allowed; }

  .panel-scroll { flex: 1; overflow-y: auto; display: flex; flex-direction: column; gap: 16px; }
  .muted { color: var(--md-on-surface-variant); margin: 4px 0 0; }
  .action-card { margin-top: 12px; padding: 12px; border-radius: var(--radius-sm); background: var(--md-surface-container); border: 1px solid var(--md-outline-variant); display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: 10px; }
  .action-btns { display: flex; flex-wrap: wrap; gap: 8px; }
  .proj-form { display: grid; grid-template-columns: repeat(auto-fit, minmax(220px, 1fr)); gap: 12px; margin-top: 12px; align-items: end; }
  .check-row { grid-column: 1 / -1; display: flex; align-items: center; gap: 8px; min-height: 44px; }
  .proj-list { display: flex; flex-direction: column; gap: 12px; }
  .proj-card { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: 12px; }
  .log-box { background: #111; color: #e2e8f0; padding: 12px; border-radius: var(--radius-sm); overflow-x: auto; max-height: 260px; font-size: 0.8125rem; }
  .prov-list { list-style: none; padding: 0; margin: 12px 0; display: flex; flex-direction: column; gap: 6px; }

  @media (max-width: 600px) {
    .agent-shell { height: calc(100vh - 110px); }
  }
</style>
