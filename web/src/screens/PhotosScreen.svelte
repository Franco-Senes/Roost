<script>
  import { onMount } from 'svelte';
  import { api } from '../lib/api.js';

  let items = $state([]);
  let nextCursor = $state(null);
  let albums = $state([]);
  let activeAlbum = $state(null);
  let showTrash = $state(false);
  let uploading = $state('');
  let error = $state('');
  let newAlbumName = $state('');
  let viewerIndex = $state(-1);
  let viewerDialog = $state(null);

  let groupedByMonth = $derived.by(() => {
    const groups = new Map();
    for (const p of items) {
      const d = new Date(p.taken_at * 1000);
      const label = d.toLocaleDateString('es-AR', { year: 'numeric', month: 'long' });
      if (!groups.has(label)) groups.set(label, []);
      groups.get(label).push(p);
    }
    return [...groups.entries()];
  });

  async function load(reset = true) {
    error = '';
    try {
      const page = await api.photos({
        cursor: reset ? null : nextCursor,
        album: activeAlbum,
        trash: showTrash,
      });
      items = reset ? page.items : [...items, ...page.items];
      nextCursor = page.next_cursor;
    } catch (err) {
      error = err.message;
    }
  }

  async function loadAlbums() {
    try {
      albums = await api.albums();
    } catch {
      albums = [];
    }
  }

  onMount(async () => {
    await Promise.all([load(true), loadAlbums()]);
  });

  async function handleFiles(event) {
    const files = Array.from(event.target.files || []);
    if (!files.length) return;
    error = '';
    const chunkSize = 512 * 1024; // 512 KB por bloque retomable
    for (let i = 0; i < files.length; i++) {
      const file = files[i];
      const uploadId = `u_${Date.now()}_${i}`;
      uploading = `Subiendo ${file.name} (${i + 1}/${files.length})...`;
      try {
        let offset = 0;
        while (offset < file.size || file.size === 0) {
          const slice = file.slice(offset, offset + chunkSize);
          await api.uploadPhotoChunk(file, uploadId, offset, slice);
          offset += slice.size;
          if (file.size === 0) break;
        }
      } catch (err) {
        error = err.message;
      }
    }
    uploading = '';
    event.target.value = '';
    await load(true);
  }

  async function createAlbum(event) {
    event.preventDefault();
    if (!newAlbumName.trim()) return;
    try {
      await api.createAlbum(newAlbumName.trim());
      newAlbumName = '';
      await loadAlbums();
    } catch (err) {
      error = err.message;
    }
  }

  function openViewer(photoId) {
    viewerIndex = items.findIndex((p) => p.id === photoId);
    if (viewerIndex >= 0) viewerDialog?.showModal();
  }

  function stepViewer(delta) {
    if (viewerIndex < 0 || !items.length) return;
    viewerIndex = (viewerIndex + delta + items.length) % items.length;
  }

  async function moveToTrash(id) {
    await api.deletePhoto(id);
    viewerDialog?.close();
    await load(true);
  }

  async function restore(id) {
    await api.restorePhoto(id);
    await load(true);
  }
</script>

<section class="page" aria-labelledby="photos-title">
  <header class="head">
    <div>
      <h1 id="photos-title">Fotos y videos</h1>
      <p class="muted">Tus recuerdos guardados en casa, sin compresión y solo visibles para vos.</p>
    </div>
    <label class="btn btn-primary upload-label">
      Subir fotos o videos
      <input type="file" accept="image/*,video/*" multiple hidden onchange={handleFiles} />
    </label>
  </header>

  <div class="alert alert-info" role="note">
    Aviso honesto: en celulares (especialmente iPhone), los navegadores limitan la copia automática en segundo plano. Abrí Roost y tocá <strong>Subir</strong> cuando quieras respaldar tus fotos nuevas.
  </div>

  {#if uploading}
    <div class="alert alert-info" role="status">{uploading}</div>
  {/if}
  {#if error}
    <div class="alert" role="alert">{error}</div>
  {/if}

  <div class="bar">
    <button
      type="button"
      class="btn"
      class:btn-tonal={!showTrash && !activeAlbum}
      class:btn-outlined={showTrash || activeAlbum}
      onclick={() => { showTrash = false; activeAlbum = null; load(true); }}
    >
      Todas
    </button>
    {#each albums as a (a.id)}
      <button
        type="button"
        class="btn"
        class:btn-tonal={activeAlbum === a.id}
        class:btn-outlined={activeAlbum !== a.id}
        onclick={() => { showTrash = false; activeAlbum = a.id; load(true); }}
      >
        {a.name} ({a.photo_count}){a.shared ? ' · Compartido' : ''}
      </button>
    {/each}
    <button
      type="button"
      class="btn"
      class:btn-tonal={showTrash}
      class:btn-outlined={!showTrash}
      onclick={() => { showTrash = true; activeAlbum = null; load(true); }}
    >
      Papelera
    </button>
  </div>

  <form class="album-form" onsubmit={createAlbum}>
    <label for="album-name" class="sr-only">Nombre del nuevo álbum</label>
    <input id="album-name" bind:value={newAlbumName} placeholder="Crear álbum nuevo..." />
    <button class="btn btn-outlined" type="submit">Crear álbum</button>
  </form>

  {#if items.length === 0}
    <div class="card">
      <p class="muted">{showTrash ? 'La papelera está vacía.' : 'Todavía no subiste fotos ni videos.'}</p>
    </div>
  {:else}
    {#each groupedByMonth as [monthLabel, group]}
      <section class="month-group">
        <h2>{monthLabel}</h2>
        <div class="photo-grid">
          {#each group as photo (photo.id)}
            <div class="photo-tile" style="background:{photo.avg_color}">
              <button
                type="button"
                class="tile-btn"
                aria-label={`Abrir ${photo.filename}`}
                onclick={() => openViewer(photo.id)}
              >
                <img
                  src={`/api/photos/${photo.id}/thumb/256`}
                  alt={photo.filename}
                  loading="lazy"
                />
                {#if photo.kind === 'video'}
                  <span class="video-tag">Video</span>
                {/if}
              </button>
              {#if showTrash}
                <button class="btn btn-tonal restore-btn" onclick={() => restore(photo.id)}>
                  Restaurar
                </button>
              {/if}
            </div>
          {/each}
        </div>
      </section>
    {/each}

    {#if nextCursor}
      <button class="btn btn-outlined" onclick={() => load(false)}>Cargar más</button>
    {/if}
  {/if}
</section>

<dialog class="md-dialog viewer" bind:this={viewerDialog}>
  {#if viewerIndex >= 0 && items[viewerIndex]}
    {@const current = items[viewerIndex]}
    <div class="viewer-body">
      {#if current.kind === 'video'}
        <video controls src={`/api/photos/${current.id}/stream`} class="viewer-media">
          <track kind="captions" />
        </video>
      {:else}
        <img src={`/api/photos/${current.id}/thumb/1024`} alt={current.filename} class="viewer-media" />
      {/if}
      <div class="viewer-actions">
        <button class="btn btn-outlined" onclick={() => stepViewer(-1)}>Anterior</button>
        <button class="btn btn-outlined" onclick={() => stepViewer(1)}>Siguiente</button>
        <a class="btn btn-tonal" href={`/api/photos/${current.id}/original`} download={current.filename}>
          Original
        </a>
        {#if !showTrash}
          <button class="btn btn-danger" onclick={() => moveToTrash(current.id)}>Papelera</button>
        {/if}
        <button class="btn btn-outlined" onclick={() => viewerDialog?.close()}>Cerrar</button>
      </div>
    </div>
  {/if}
</dialog>

<style>
  .page { display: flex; flex-direction: column; gap: 16px; }
  .head { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: 12px; }
  h1 { margin: 0; font-size: 1.75rem; font-weight: 500; }
  h2 { margin: 0 0 10px; font-size: 1.0625rem; font-weight: 500; text-transform: capitalize; }
  .muted { color: var(--md-on-surface-variant); margin: 4px 0 0; }
  .bar { display: flex; flex-wrap: wrap; gap: 8px; }
  .album-form { display: flex; gap: 8px; max-width: 420px; }
  .album-form input { flex: 1; min-height: 44px; padding: 0 14px; border-radius: var(--radius-sm); border: 1px solid var(--md-outline-variant); background: var(--md-surface); }
  .photo-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(150px, 1fr)); gap: 10px; }
  .photo-tile { position: relative; aspect-ratio: 1; border-radius: var(--radius-md); overflow: hidden; }
  .tile-btn { width: 100%; height: 100%; padding: 0; border: none; background: transparent; cursor: pointer; display: block; }
  .tile-btn img { width: 100%; height: 100%; object-fit: cover; display: block; }
  .video-tag { position: absolute; bottom: 8px; right: 8px; padding: 2px 8px; border-radius: var(--radius-pill); background: rgba(0, 0, 0, 0.7); color: #fff; font-size: 0.75rem; }
  .restore-btn { position: absolute; bottom: 8px; left: 8px; }
  .viewer { max-width: 900px; width: calc(100% - 24px); }
  .viewer-body { display: flex; flex-direction: column; gap: 14px; }
  .viewer-media { max-height: 70vh; width: 100%; object-fit: contain; border-radius: var(--radius-md); background: #000; }
  .viewer-actions { display: flex; flex-wrap: wrap; gap: 8px; justify-content: flex-end; }
</style>
