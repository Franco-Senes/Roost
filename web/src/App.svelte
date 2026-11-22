<script>
  import { onMount } from 'svelte';
  import { api, setCsrfToken } from './lib/api.js';
  import Icon from './components/Icon.svelte';
  import AuthScreen from './screens/AuthScreen.svelte';

  let loading = $state(true);
  let needsSetup = $state(false);
  let me = $state(null);
  let tab = $state('home');

  let theme = $state(localStorage.getItem('roost_theme') || 'green');
  let mode = $state(localStorage.getItem('roost_mode') || 'light');

  let system = $state(null);
  let store = $state([]);
  let installedApps = $state([]);
  let installs = $state({});

  // Code splitting por pantalla (AGENTS.md §2)
  let ActiveScreen = $state(null);

  const navItems = [
    { id: 'home', label: 'Inicio', icon: 'home' },
    { id: 'store', label: 'Tienda', icon: 'store' },
    { id: 'photos', label: 'Fotos', icon: 'photos' },
    { id: 'agent', label: 'Agente', icon: 'agent' },
    { id: 'settings', label: 'Ajustes', icon: 'settings' },
  ];

  function applyTheme(nextTheme, nextMode) {
    theme = nextTheme;
    mode = nextMode;
    document.documentElement.dataset.theme = nextTheme;
    document.documentElement.dataset.mode = nextMode;
    localStorage.setItem('roost_theme', nextTheme);
    localStorage.setItem('roost_mode', nextMode);
  }

  async function loadScreen(name) {
    if (name === 'home') {
      ActiveScreen = (await import('./screens/HomeScreen.svelte')).default;
    } else if (name === 'store') {
      ActiveScreen = (await import('./screens/StoreScreen.svelte')).default;
    } else if (name === 'photos') {
      ActiveScreen = (await import('./screens/PhotosScreen.svelte')).default;
    } else if (name === 'agent') {
      ActiveScreen = (await import('./screens/AgentScreen.svelte')).default;
    } else if (name === 'settings') {
      ActiveScreen = (await import('./screens/SettingsScreen.svelte')).default;
    } else {
      ActiveScreen = null;
    }
  }

  $effect(() => {
    loadScreen(tab);
  });

  async function refreshData() {
    const [sys, st, apps] = await Promise.all([api.system(), api.store(), api.apps()]);
    system = sys;
    store = st;
    installedApps = apps;
  }

  let eventSource = null;

  function connectEvents() {
    if (eventSource) eventSource.close();
    eventSource = new EventSource('/api/events');
    eventSource.onmessage = (msg) => {
      try {
        const ev = JSON.parse(msg.data);
        if (ev.type === 'system_metrics') {
          system = ev;
        } else if (ev.type === 'app_install_progress') {
          installs = { ...installs, [ev.app_id]: ev };
        } else if (ev.type === 'app_install_finished' || ev.type === 'app_install_failed') {
          const next = { ...installs };
          delete next[ev.app_id];
          installs = next;
          refreshData();
        } else if (ev.type === 'app_state_changed') {
          refreshData();
        }
      } catch {
        // Ignora mensajes malformados.
      }
    };
  }

  async function onAuthenticated(user) {
    me = user;
    try {
      const saved = await api.settings();
      if (saved?.theme) applyTheme(saved.theme, saved.mode || mode);
    } catch {
      // Usa preferencias locales.
    }
    await refreshData();
    connectEvents();
  }

  async function logout() {
    if (eventSource) {
      eventSource.close();
      eventSource = null;
    }
    try {
      await api.logout();
    } finally {
      setCsrfToken('');
      me = null;
    }
  }

  onMount(async () => {
    applyTheme(theme, mode);
    try {
      const setup = await api.setupStatus();
      needsSetup = setup.needs_setup;
      if (!needsSetup) {
        const user = await api.me();
        setCsrfToken(user.csrf_token);
        await onAuthenticated(user);
      }
    } catch {
      me = null;
    } finally {
      loading = false;
    }
    return () => {
      if (eventSource) eventSource.close();
    };
  });
</script>

<a class="skip-link" href="#main-content">Saltar al contenido principal</a>

{#if loading}
  <main id="main-content" class="loading-wrap">
    <p>Cargando Roost...</p>
  </main>
{:else if !me}
  <AuthScreen {needsSetup} {onAuthenticated} />
{:else}
  <div class="shell">
    <nav class="rail" aria-label="Navegación principal">
      <button
        type="button"
        class="fab-rail"
        aria-label="Instalar una app"
        onclick={() => (tab = 'store')}
      >
        <Icon name="add" size={24} />
      </button>

      {#each navItems as item}
        <button
          type="button"
          class="nav-btn"
          class:nav-active={tab === item.id}
          aria-current={tab === item.id ? 'page' : undefined}
          onclick={() => (tab = item.id)}
        >
          <span class="pill-indicator">
            <Icon name={item.icon} size={22} />
          </span>
          <span class="nav-label">{item.label}</span>
        </button>
      {/each}
    </nav>

    <main id="main-content" class="content">
      {#if tab === 'home' && ActiveScreen}
        <ActiveScreen
          {me}
          {system}
          {installedApps}
          {installs}
          onRefresh={refreshData}
          onOpenStore={() => (tab = 'store')}
        />
      {:else if tab === 'store' && ActiveScreen}
        <ActiveScreen {me} {store} {installedApps} {installs} onRefresh={refreshData} />
      {:else if tab === 'settings' && ActiveScreen}
        <ActiveScreen
          {me}
          {theme}
          {mode}
          onThemeChange={applyTheme}
          onLogout={logout}
        />
      {:else if tab === 'photos' && ActiveScreen}
        <ActiveScreen />
      {:else if tab === 'agent' && ActiveScreen}
        <ActiveScreen {me} />
      {/if}
    </main>

    <button
      type="button"
      class="fab-mobile"
      aria-label="Instalar una app"
      onclick={() => (tab = 'store')}
    >
      <Icon name="add" size={24} />
    </button>
  </div>
{/if}

<style>
  .loading-wrap { min-height: 100vh; display: grid; place-items: center; }
  .shell { display: flex; min-height: 100vh; }

  .rail {
    width: 88px;
    padding: 16px 8px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
    background: var(--md-surface-container);
    border-right: 1px solid var(--md-outline-variant);
    position: sticky;
    top: 0;
    height: 100vh;
  }

  .fab-rail {
    width: 56px;
    height: 56px;
    border-radius: 16px;
    border: none;
    background: var(--md-primary-container);
    color: var(--md-on-primary-container);
    display: grid;
    place-items: center;
    cursor: pointer;
    margin-bottom: 8px;
  }

  .nav-btn {
    width: 72px;
    min-height: 56px;
    border: none;
    background: transparent;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 4px;
    cursor: pointer;
    border-radius: 12px;
    color: var(--md-on-surface-variant);
  }

  .pill-indicator {
    width: 56px;
    height: 32px;
    border-radius: var(--radius-pill);
    display: grid;
    place-items: center;
    transition: background var(--transition-fast);
  }

  .nav-active { color: var(--md-on-surface); font-weight: 600; }
  .nav-active .pill-indicator {
    background: var(--md-secondary-container);
    color: var(--md-on-secondary-container);
  }

  .nav-label { font-size: 0.75rem; }

  .content {
    flex: 1;
    max-width: 1120px;
    width: 100%;
    margin: 0 auto;
    padding: 24px;
  }

  .fab-mobile { display: none; }

  @media (max-width: 600px) {
    .shell { flex-direction: column; padding-bottom: 80px; }
    .rail {
      position: fixed;
      bottom: 0;
      left: 0;
      right: 0;
      top: auto;
      width: 100%;
      height: 76px;
      flex-direction: row;
      justify-content: space-around;
      padding: 6px 4px;
      border-right: none;
      border-top: 1px solid var(--md-outline-variant);
      z-index: 20;
    }
    .fab-rail { display: none; }
    .fab-mobile {
      display: grid;
      place-items: center;
      position: fixed;
      right: 16px;
      bottom: 92px;
      width: 56px;
      height: 56px;
      border-radius: 16px;
      border: none;
      background: var(--md-primary-container);
      color: var(--md-on-primary-container);
      box-shadow: 0 4px 14px rgba(0, 0, 0, 0.2);
      cursor: pointer;
      z-index: 21;
    }
    .content { padding: 16px; }
  }
</style>
