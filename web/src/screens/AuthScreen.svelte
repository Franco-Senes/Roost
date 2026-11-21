<script>
  import { api, setCsrfToken } from '../lib/api.js';

  let { needsSetup = false, onAuthenticated } = $props();
  let name = $state('');
  let password = $state('');
  let error = $state('');
  let loading = $state(false);

  async function handleSubmit(event) {
    event.preventDefault();
    error = '';
    loading = true;
    try {
      const me = needsSetup
        ? await api.setupAdmin(name, password)
        : await api.login(name, password);
      setCsrfToken(me.csrf_token);
      onAuthenticated(me);
    } catch (err) {
      error = err.message;
    } finally {
      loading = false;
    }
  }
</script>

<main class="auth-wrap">
  <section class="card auth-card" aria-labelledby="auth-title">
    <div class="brand-badge" aria-hidden="true">R</div>
    <h1 id="auth-title">
      {needsSetup ? 'Te damos la bienvenida a Roost' : 'Iniciá sesión en Roost'}
    </h1>
    <p class="subtitle">
      {needsSetup
        ? 'Creá la cuenta principal de la familia. Elegí una contraseña de al menos 10 caracteres.'
        : 'Ingresá con tu nombre y contraseña para abrir tus apps y archivos.'}
    </p>

    {#if error}
      <div class="alert" role="alert">{error}</div>
    {/if}

    <form onsubmit={handleSubmit} class="auth-form">
      <div class="field">
        <label for="auth-name">Tu nombre</label>
        <input
          id="auth-name"
          type="text"
          autocomplete="username"
          required
          bind:value={name}
          placeholder="Por ejemplo: Franco"
        />
      </div>

      <div class="field">
        <label for="auth-pass">Contraseña</label>
        <input
          id="auth-pass"
          type="password"
          autocomplete={needsSetup ? 'new-password' : 'current-password'}
          minlength={needsSetup ? 10 : 1}
          required
          bind:value={password}
          placeholder={needsSetup ? 'Mínimo 10 caracteres' : 'Tu contraseña'}
        />
      </div>

      <button class="btn btn-primary" type="submit" disabled={loading}>
        {loading
          ? 'Un momento...'
          : needsSetup
            ? 'Crear cuenta principal'
            : 'Entrar'}
      </button>
    </form>
  </section>
</main>

<style>
  .auth-wrap {
    min-height: 100vh;
    display: grid;
    place-items: center;
    padding: 20px;
  }
  .auth-card {
    width: 100%;
    max-width: 420px;
    display: flex;
    flex-direction: column;
    gap: 16px;
  }
  .brand-badge {
    width: 48px;
    height: 48px;
    border-radius: 16px;
    background: var(--md-primary-container);
    color: var(--md-on-primary-container);
    display: grid;
    place-items: center;
    font-weight: 700;
    font-size: 1.5rem;
  }
  h1 {
    margin: 0;
    font-size: 1.5rem;
    font-weight: 500;
  }
  .subtitle {
    margin: 0;
    color: var(--md-on-surface-variant);
    line-height: 1.5;
  }
  .auth-form {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
</style>
