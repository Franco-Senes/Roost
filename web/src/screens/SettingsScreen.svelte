<script>
  import { onMount } from 'svelte';
  import { api } from '../lib/api.js';

  let { me, theme, mode, onThemeChange, onLogout } = $props();

  let users = $state([]);
  let newName = $state('');
  let newPass = $state('');
  let newRole = $state('member');
  let feedback = $state('');
  let error = $state('');

  const themes = [
    { id: 'green', label: 'Verde', swatch: '#3B6939' },
    { id: 'blue', label: 'Azul', swatch: '#415F91' },
    { id: 'purple', label: 'Violeta', swatch: '#6750A4' },
  ];

  async function loadUsers() {
    if (me.role !== 'admin') return;
    try {
      users = await api.users();
    } catch (err) {
      error = err.message;
    }
  }

  onMount(loadUsers);

  async function pickTheme(nextTheme, nextMode) {
    onThemeChange(nextTheme, nextMode);
    try {
      await api.updateSettings({ theme: nextTheme, mode: nextMode });
    } catch {
      // Mantiene la preferencia local si el servidor no respondió.
    }
  }

  async function addUser(event) {
    event.preventDefault();
    error = '';
    feedback = '';
    try {
      await api.createUser(newName, newPass, newRole);
      newName = '';
      newPass = '';
      feedback = 'Usuario creado correctamente.';
      await loadUsers();
    } catch (err) {
      error = err.message;
    }
  }

  async function toggleSuspend(user) {
    error = '';
    try {
      await api.updateUser(user.id, { suspended: !user.suspended });
      await loadUsers();
    } catch (err) {
      error = err.message;
    }
  }
</script>

<section class="page" aria-labelledby="settings-title">
  <header class="head">
    <h1 id="settings-title">Ajustes</h1>
    <button class="btn btn-outlined" onclick={onLogout}>Cerrar sesión</button>
  </header>

  {#if feedback}
    <div class="alert alert-info" role="status">{feedback}</div>
  {/if}
  {#if error}
    <div class="alert" role="alert">{error}</div>
  {/if}

  <article class="card">
    <h2>Apariencia</h2>
    <div class="row">
      {#each themes as item}
        <button
          type="button"
          class="btn"
          class:btn-primary={theme === item.id}
          class:btn-outlined={theme !== item.id}
          aria-pressed={theme === item.id}
          onclick={() => pickTheme(item.id, mode)}
        >
          <span class="swatch" style="background:{item.swatch}" aria-hidden="true"></span>
          {item.label}
        </button>
      {/each}
    </div>
    <div class="row" style="margin-top: 12px;">
      <button
        type="button"
        class="btn"
        class:btn-tonal={mode === 'light'}
        class:btn-outlined={mode !== 'light'}
        aria-pressed={mode === 'light'}
        onclick={() => pickTheme(theme, 'light')}
      >
        Modo claro
      </button>
      <button
        type="button"
        class="btn"
        class:btn-tonal={mode === 'dark'}
        class:btn-outlined={mode !== 'dark'}
        aria-pressed={mode === 'dark'}
        onclick={() => pickTheme(theme, 'dark')}
      >
        Modo oscuro
      </button>
    </div>
  </article>

  {#if me.role === 'admin'}
    <article class="card">
      <h2>Familia y usuarios</h2>
      <ul class="user-list">
        {#each users as u (u.id)}
          <li>
            <div>
              <strong>{u.name}</strong>
              <span class="muted">
                · {u.role === 'admin' ? 'Administrador' : 'Miembro'}
                {u.suspended ? ' · Suspendido' : ''}
              </span>
            </div>
            {#if u.id !== me.id}
              <button class="btn btn-outlined" onclick={() => toggleSuspend(u)}>
                {u.suspended ? 'Reactivar' : 'Suspender'}
              </button>
            {/if}
          </li>
        {/each}
      </ul>

      <form class="add-user" onsubmit={addUser}>
        <h3>Agregar persona</h3>
        <div class="form-row">
          <div class="field">
            <label for="u-name">Nombre</label>
            <input id="u-name" required bind:value={newName} placeholder="Sofi" />
          </div>
          <div class="field">
            <label for="u-pass">Contraseña (mín. 10)</label>
            <input id="u-pass" type="password" minlength="10" required bind:value={newPass} />
          </div>
          <div class="field">
            <label for="u-role">Rol</label>
            <select id="u-role" bind:value={newRole}>
              <option value="member">Miembro</option>
              <option value="admin">Administrador</option>
            </select>
          </div>
        </div>
        <button class="btn btn-primary" type="submit">Crear usuario</button>
      </form>
    </article>
  {/if}

  <article class="card">
    <h2>Acerca de Roost</h2>
    <p class="muted">
      Roost es software libre bajo licencia <strong>MIT</strong>.
    </p>
    <p class="muted">
      Este proyecto se construye con ayuda de inteligencia artificial y <strong>Franco Senes</strong> revisa cada cambio antes de integrarlo.
    </p>
  </article>
</section>

<style>
  .page { display: flex; flex-direction: column; gap: 18px; }
  .head { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
  h1 { margin: 0; font-size: 1.75rem; font-weight: 500; }
  h2 { margin: 0 0 12px; font-size: 1.125rem; font-weight: 500; }
  h3 { margin: 16px 0 8px; font-size: 1rem; font-weight: 500; }
  .row { display: flex; flex-wrap: wrap; gap: 10px; }
  .swatch { width: 14px; height: 14px; border-radius: 50%; border: 1px solid rgba(0, 0, 0, 0.2); }
  .user-list { list-style: none; padding: 0; margin: 0; display: flex; flex-direction: column; gap: 10px; }
  .user-list li { display: flex; align-items: center; justify-content: space-between; gap: 12px; padding: 8px 0; border-bottom: 1px solid var(--md-outline-variant); }
  .form-row { display: grid; grid-template-columns: repeat(auto-fit, minmax(180px, 1fr)); gap: 12px; margin-bottom: 12px; }
  .muted { color: var(--md-on-surface-variant); margin: 4px 0 0; }
</style>
