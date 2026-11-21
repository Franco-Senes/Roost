let csrfToken = '';

export function setCsrfToken(token) {
  csrfToken = token || '';
}

async function request(path, options = {}) {
  const headers = { ...(options.headers || {}) };
  if (options.body && !headers['Content-Type']) {
    headers['Content-Type'] = 'application/json';
  }
  const method = (options.method || 'GET').toUpperCase();
  if (method !== 'GET' && method !== 'HEAD' && csrfToken) {
    headers['X-CSRF-Token'] = csrfToken;
  }

  const res = await fetch(path, {
    credentials: 'same-origin',
    ...options,
    headers,
  });

  if (res.status === 204) {
    return null;
  }

  const text = await res.text();
  let data = null;
  try {
    data = text ? JSON.parse(text) : null;
  } catch {
    data = { error: text };
  }

  if (!res.ok) {
    throw new Error(data?.error || 'Ocurrió un problema inesperado. Probá de nuevo.');
  }
  return data;
}

export const api = {
  setupStatus: () => request('/api/setup'),
  setupAdmin: (name, password) =>
    request('/api/setup', { method: 'POST', body: JSON.stringify({ name, password }) }),
  login: (name, password) =>
    request('/api/login', { method: 'POST', body: JSON.stringify({ name, password }) }),
  logout: () => request('/api/logout', { method: 'POST' }),
  me: () => request('/api/me'),
  system: () => request('/api/system'),
  store: () => request('/api/store'),
  apps: () => request('/api/apps'),
  installApp: (id) => request(`/api/apps/${encodeURIComponent(id)}/install`, { method: 'POST' }),
  startApp: (id) => request(`/api/apps/${encodeURIComponent(id)}/start`, { method: 'POST' }),
  stopApp: (id) => request(`/api/apps/${encodeURIComponent(id)}/stop`, { method: 'POST' }),
  restartApp: (id) => request(`/api/apps/${encodeURIComponent(id)}/restart`, { method: 'POST' }),
  uninstallApp: (id) =>
    request(`/api/apps/${encodeURIComponent(id)}`, {
      method: 'DELETE',
      body: JSON.stringify({ confirm: true }),
    }),
  users: () => request('/api/users'),
  createUser: (name, password, role) =>
    request('/api/users', { method: 'POST', body: JSON.stringify({ name, password, role }) }),
  updateUser: (id, patch) =>
    request(`/api/users/${id}`, { method: 'PATCH', body: JSON.stringify(patch) }),
  deleteUser: (id) =>
    request(`/api/users/${id}`, { method: 'DELETE', body: JSON.stringify({ confirm: true }) }),
  settings: () => request('/api/settings'),
  updateSettings: (patch) =>
    request('/api/settings', { method: 'PATCH', body: JSON.stringify(patch) }),
};
