<script>
  import { api } from '../lib/api.js';

  let {
    me,
    system,
    installedApps = [],
    installs = {},
    onRefresh,
    onOpenStore,
  } = $props();

  let busyId = $state('');
  let feedback = $state('');
  let confirmApp = $state(null);
  let dialogEl = $state(null);

  let activeCount = $derived(
    installedApps.filter((a) => a.status === 'running').length
  );
  let activeInstalls = $derived(Object.values(installs));

  function openAppUrl(app) {
    const host = window.location.hostname;
    window.open(`http://${host}:${app.web_port}`, '_blank', 'noopener');
  }

  async function runAction(id, fn) {
    busyId = id;
    feedback = '';
    try {
      await fn();
      await onRefresh();
    } catch (err) {
      feedback = err.message;
    } finally {
      busyId = '';
    }
  }

  function askUninstall(app) {
    confirmApp = app;
    dialogEl?.showModal();
  }

  async function confirmUninstall() {
    if (!confirmApp) return;
    const target = confirmApp;
    dialogEl?.close();
    confirmApp = null;
    await runAction(target.id, () => api.uninstallApp(target.id));
  }
</script>

<section class="page" aria-labelledby="home-title">
  <header class="page-head">
    <div>
      <h1 id="home-title">Hola, {me.name}</h1>
      <p class="status-line">
        Todo funciona · {activeCount} {activeCount === 1 ? 'app activa' : 'apps activas'}
        {#if system?.demo}
          <span class="demo-chip">Modo demo</span>
        {/if}
      </p>
    </div>
    <button class="btn btn-primary" onclick={onOpenStore}>Instalar app</button>
  </header>

  {#if feedback}
    <div class="alert" role="alert">{feedback}</div>
  {/if}

  {#if activeInstalls.length > 0}
    <section class="card-high" aria-live="polite">
      <h2>Instalando ahora</h2>
      {#each activeInstalls as item (item.app_id)}
        <div class="progress-row">
          <div class="progress-meta">
            <strong>{item.app_id}</strong>
            <span>{item.downloaded_mb} / {item.total_mb} MB ({item.percent}%)</span>
          </div>
          <div class="progress-track" role="progressbar" aria-valuenow={item.percent} aria-valuemin="0" aria-valuemax="100">
            <div class="progress-fill" style="width: {item.percent}%"></div>
          </div>
        </div>
      {/each}
    </section>
  {/if}

  <div class="metrics-grid">
    <article class="card">
      <h2>Sistema</h2>
      {#if system}
        <dl class="metric-list">
          <div>
            <dt>Procesador</dt>
            <dd>{system.cpu_percent}%</dd>
          </div>
          <div>
            <dt>Memoria</dt>
            <dd>{system.ram_used_mb} / {system.ram_total_mb} MB</dd>
          </div>
          <div>
            <dt>Temperatura</dt>
            <dd>{system.temperature_c != null ? `${system.temperature_c} °C` : 'No disponible'}</dd>
          </div>
        </dl>
      {:else}
        <p class="muted">Cargando estado del equipo...</p>
      {/if}
    </article>

    <article class="card">
      <h2>Almacenamiento</h2>
      {#if system}
        <p class="storage-big">{system.disk_used_gb} GB usados</p>
        <p class="muted">de {system.disk_total_gb} GB totales</p>
        {@const diskPct = system.disk_total_gb > 0 ? Math.min(100, Math.round((system.disk_used_gb / system.disk_total_gb) * 100)) : 0}
        <div class="progress-track" aria-hidden="true">
          <div class="progress-fill" style="width: {diskPct}%"></div>
        </div>
      {:else}
        <p class="muted">Calculando espacio disponible...</p>
      {/if}
    </article>
  </div>

  <section class="apps-section" aria-labelledby="my-apps-title">
    <h2 id="my-apps-title">Tus apps</h2>
    {#if installedApps.length === 0}
      <div class="card empty">
        <p>Todavía no instalaste ninguna app.</p>
        <button class="btn btn-tonal" onclick={onOpenStore}>Explorar la Tienda</button>
      </div>
    {:else}
      <div class="apps-grid">
        {#each installedApps as app (app.id)}
          <article class="card app-card">
            <div class="app-top">
              <img src={`/api/store/${app.id}/icon`} alt="" width="44" height="44" loading="lazy" />
              <div>
                <h3>{app.name}</h3>
                <p class="muted">{app.tagline}</p>
              </div>
              <span class="pill" class:pill-on={app.status === 'running'}>
                {app.status === 'running' ? 'Activa' : 'Detenida'}
              </span>
            </div>

            <div class="app-actions">
              {#if app.status === 'running'}
                <button class="btn btn-primary" onclick={() => openAppUrl(app)}>Abrir</button>
              {/if}
              {#if me.role === 'admin'}
                {#if app.status === 'running'}
                  <button
                    class="btn btn-outlined"
                    disabled={busyId === app.id}
                    onclick={() => runAction(app.id, () => api.stopApp(app.id))}
                  >
                    Detener
                  </button>
                {:else}
                  <button
                    class="btn btn-tonal"
                    disabled={busyId === app.id}
                    onclick={() => runAction(app.id, () => api.startApp(app.id))}
                  >
                    Iniciar
                  </button>
                {/if}
                <button
                  class="btn btn-danger"
                  disabled={busyId === app.id}
                  onclick={() => askUninstall(app)}
                >
                  Desinstalar
                </button>
              {/if}
            </div>
          </article>
        {/each}
      </div>
    {/if}
  </section>
</section>

<dialog class="md-dialog" bind:this={dialogEl}>
  {#if confirmApp}
    <h3>¿Desinstalar {confirmApp.name}?</h3>
    <p class="muted">La app se va a quitar de este equipo. Podés volver a instalarla cuando quieras.</p>
    <div class="dialog-actions">
      <button class="btn btn-outlined" onclick={() => dialogEl?.close()}>Cancelar</button>
      <button class="btn btn-danger" onclick={confirmUninstall}>Sí, desinstalar</button>
    </div>
  {/if}
</dialog>

<style>
  .page { display: flex; flex-direction: column; gap: 20px; }
  .page-head { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: 12px; }
  h1 { margin: 0; font-size: 1.75rem; font-weight: 500; }
  h2 { margin: 0 0 12px; font-size: 1.125rem; font-weight: 500; }
  h3 { margin: 0; font-size: 1rem; font-weight: 500; }
  .status-line { margin: 4px 0 0; color: var(--md-on-surface-variant); display: flex; align-items: center; gap: 8px; }
  .demo-chip { padding: 2px 10px; border-radius: var(--radius-pill); background: var(--md-primary-container); color: var(--md-on-primary-container); font-size: 0.75rem; font-weight: 500; }
  .metrics-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(260px, 1fr)); gap: 16px; }
  .metric-list { margin: 0; display: grid; grid-template-columns: repeat(3, 1fr); gap: 12px; }
  .metric-list dt { font-size: 0.8125rem; color: var(--md-on-surface-variant); }
  .metric-list dd { margin: 4px 0 0; font-size: 1.125rem; font-weight: 500; }
  .storage-big { margin: 0; font-size: 1.375rem; font-weight: 500; }
  .muted { color: var(--md-on-surface-variant); margin: 4px 0 0; }
  .progress-row { display: flex; flex-direction: column; gap: 6px; margin-bottom: 10px; }
  .progress-meta { display: flex; justify-content: space-between; font-size: 0.875rem; }
  .progress-track { height: 8px; border-radius: var(--radius-pill); background: var(--md-surface-container-highest); overflow: hidden; margin-top: 8px; }
  .progress-fill { height: 100%; background: var(--md-primary); transition: width var(--transition-fast); }
  .apps-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(280px, 1fr)); gap: 16px; }
  .app-card { display: flex; flex-direction: column; gap: 16px; }
  .app-top { display: flex; align-items: center; gap: 12px; }
  .app-top img { border-radius: 12px; flex-shrink: 0; }
  .pill { margin-left: auto; padding: 4px 10px; border-radius: var(--radius-pill); font-size: 0.75rem; background: var(--md-surface-container-highest); }
  .pill-on { background: var(--md-primary-container); color: var(--md-on-primary-container); }
  .app-actions { display: flex; flex-wrap: wrap; gap: 8px; }
  .empty { display: flex; flex-direction: column; align-items: flex-start; gap: 12px; }
  .dialog-actions { display: flex; justify-content: flex-end; gap: 10px; margin-top: 20px; }
</style>
