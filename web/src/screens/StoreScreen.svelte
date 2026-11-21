<script>
  import { api } from '../lib/api.js';
  import Icon from '../components/Icon.svelte';

  let { me, store = [], installedApps = [], installs = {} } = $props();

  let query = $state('');
  let selectedCategory = $state('Todas');
  let notice = $state('');
  let error = $state('');

  const categories = ['Todas', 'Películas', 'Archivos', 'Red', 'Casa', 'Seguridad', 'Fotos'];

  let installedMap = $derived(
    Object.fromEntries(installedApps.map((app) => [app.id, app]))
  );

  let filtered = $derived(
    store.filter((app) => {
      const catOk = selectedCategory === 'Todas' || app.category === selectedCategory;
      const q = query.trim().toLowerCase();
      const textOk =
        !q ||
        app.name.toLowerCase().includes(q) ||
        app.tagline.toLowerCase().includes(q) ||
        app.description.toLowerCase().includes(q);
      return catOk && textOk;
    })
  );

  let recommended = $derived(store.find((a) => a.id === 'jellyfin') || store[0] || null);

  async function install(app) {
    error = '';
    notice = '';
    try {
      const res = await api.installApp(app.id);
      if (res.ram_warning) {
        notice = res.ram_warning;
      }
    } catch (err) {
      error = err.message;
    }
  }

  function openApp(app) {
    const host = window.location.hostname;
    window.open(`http://${host}:${app.web_port}`, '_blank', 'noopener');
  }
</script>

<section class="page" aria-labelledby="store-title">
  <h1 id="store-title">Tienda de apps</h1>

  <div class="search-box">
    <Icon name="search" size={20} />
    <label for="store-search" class="sr-only">Buscar apps</label>
    <input
      id="store-search"
      type="search"
      bind:value={query}
      placeholder="¿Qué querés hacer? (películas, archivos, anuncios...)"
    />
  </div>

  <div class="chips" role="group" aria-label="Filtrar por categoría">
    {#each categories as cat}
      <button
        type="button"
        class="chip"
        class:chip-active={selectedCategory === cat}
        aria-pressed={selectedCategory === cat}
        onclick={() => (selectedCategory = cat)}
      >
        {#if selectedCategory === cat}
          <Icon name="check" size={16} />
        {/if}
        <span>{cat}</span>
      </button>
    {/each}
  </div>

  {#if notice}
    <div class="alert alert-info" role="status">{notice}</div>
  {/if}
  {#if error}
    <div class="alert" role="alert">{error}</div>
  {/if}

  {#if recommended && !query && selectedCategory === 'Todas'}
    <article class="card-high featured">
      <div class="featured-text">
        <span class="badge">Recomendada</span>
        <h2>{recommended.name}</h2>
        <p>{recommended.description}</p>
      </div>
      {#if installedMap[recommended.id]}
        <button class="btn btn-primary" onclick={() => openApp(recommended)}>Abrir</button>
      {:else if me.role === 'admin'}
        <button
          class="btn btn-primary"
          disabled={Boolean(installs[recommended.id])}
          onclick={() => install(recommended)}
        >
          {installs[recommended.id] ? `Instalando ${installs[recommended.id].percent}%` : 'Instalar'}
        </button>
      {/if}
    </article>
  {/if}

  <div class="grid">
    {#each filtered as app (app.id)}
      {@const inst = installedMap[app.id]}
      {@const prog = installs[app.id]}
      <article class="card app-tile">
        <div class="tile-head">
          <img src={`/api/store/${app.id}/icon`} alt="" width="48" height="48" loading="lazy" />
          <div>
            <h3>{app.name}</h3>
            <p class="tagline">{app.tagline}</p>
          </div>
        </div>
        <p class="desc">{app.description}</p>
        <div class="tile-foot">
          <span class="meta">{app.category} · {app.ram_recommended_mb} MB RAM</span>
          {#if prog}
            <span class="progress-tag">{prog.percent}% ({prog.downloaded_mb} MB)</span>
          {:else if inst}
            <button class="btn btn-tonal" onclick={() => openApp(app)}>Abrir</button>
          {:else if me.role === 'admin'}
            <button class="btn btn-primary" onclick={() => install(app)}>Instalar</button>
          {:else}
            <span class="meta">Pedile al admin que la instale</span>
          {/if}
        </div>
      </article>
    {/each}
  </div>
</section>

<style>
  .page { display: flex; flex-direction: column; gap: 18px; }
  h1 { margin: 0; font-size: 1.75rem; font-weight: 500; }
  h2 { margin: 6px 0; font-size: 1.375rem; font-weight: 500; }
  h3 { margin: 0; font-size: 1.0625rem; font-weight: 500; }
  .search-box {
    display: flex; align-items: center; gap: 10px;
    padding: 0 16px; min-height: 48px;
    border-radius: var(--radius-pill);
    background: var(--md-surface-container-high);
  }
  .search-box input {
    border: none; background: transparent; width: 100%; min-height: 44px; outline: none;
  }
  .chips { display: flex; flex-wrap: wrap; gap: 8px; }
  .chip {
    display: inline-flex; align-items: center; gap: 6px;
    min-height: 44px; padding: 0 14px;
    border-radius: var(--radius-sm);
    border: 1px solid var(--md-outline-variant);
    background: transparent; cursor: pointer;
  }
  .chip-active {
    background: var(--md-secondary-container);
    color: var(--md-on-secondary-container);
    border-color: transparent;
  }
  .featured {
    display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: 16px;
    background: var(--md-primary-container); color: var(--md-on-primary-container);
  }
  .badge { font-size: 0.75rem; font-weight: 600; text-transform: uppercase; letter-spacing: 0.04em; }
  .featured-text p { margin: 0; max-width: 560px; }
  .grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(280px, 1fr)); gap: 16px; }
  .app-tile { display: flex; flex-direction: column; gap: 12px; }
  .tile-head { display: flex; align-items: center; gap: 12px; }
  .tile-head img { border-radius: 12px; }
  .tagline { margin: 2px 0 0; font-size: 0.875rem; color: var(--md-on-surface-variant); }
  .desc { margin: 0; font-size: 0.9375rem; line-height: 1.45; color: var(--md-on-surface-variant); flex: 1; }
  .tile-foot { display: flex; align-items: center; justify-content: space-between; gap: 8px; }
  .meta { font-size: 0.8125rem; color: var(--md-on-surface-variant); }
  .progress-tag { font-size: 0.875rem; font-weight: 500; color: var(--md-primary); }
</style>
