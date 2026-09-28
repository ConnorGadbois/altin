(() => {
    const STORAGE_KEYS = {
        theme: 'altin.theme',
        token: 'altin.token',
        serverUrl: 'altin.serverUrl',
    };

    const state = {
        serverUrl: '',
        token: '',
        keys: [],
        filteredKeys: [],
        search: '',
        editingKey: null,
        revealedKeys: new Set(),
    };

    const els = {};

    const normalizeServer = (value) => value.trim().replace(/\/+$/, '');

    const escapeHTML = (value) => String(value ?? '')
        .replace(/&/g, '&amp;')
        .replace(/</g, '&lt;')
        .replace(/>/g, '&gt;')
        .replace(/"/g, '&quot;')
        .replace(/'/g, '&#39;');

    const setTheme = (theme) => {
        const nextTheme = theme === 'light' ? 'light' : 'dark';
        document.documentElement.setAttribute('data-theme', nextTheme);
        localStorage.setItem(STORAGE_KEYS.theme, nextTheme);
        if (els.themeToggle) {
            els.themeToggle.textContent = nextTheme === 'dark' ? 'Light' : 'Dark';
        }
    };

    const getTheme = () => localStorage.getItem(STORAGE_KEYS.theme) || 'dark';

    const redirectToLogin = () => {
        window.location.href = '/login.html';
    };

    const requireAuth = () => {
        const token = localStorage.getItem(STORAGE_KEYS.token);
        const serverUrl = localStorage.getItem(STORAGE_KEYS.serverUrl);

        if (!token || !serverUrl) {
            redirectToLogin();
            return false;
        }

        state.token = token;
        state.serverUrl = normalizeServer(serverUrl);
        return true;
    };

    const apiFetch = async (path, options = {}) => {
        const headers = new Headers(options.headers || {});
        headers.set('Authorization', state.token);

        if (options.body && !headers.has('Content-Type')) {
            headers.set('Content-Type', 'application/json');
        }

        const response = await fetch(`${state.serverUrl}${path}`, {
            ...options,
            headers,
        });

        if (response.status === 401) {
            redirectToLogin();
            throw new Error('Unauthorized');
        }

        return response;
    };

    const loadJSON = async (path) => {
        const response = await apiFetch(path);
        const payload = await response.json().catch(() => ({}));
        if (!response.ok) {
            throw new Error(payload.message || `Request failed for ${path}`);
        }
        return payload;
    };

    const setMessage = (message, tone = '') => {
        if (!els.keyModalMessage) return;
        els.keyModalMessage.textContent = message;
        els.keyModalMessage.dataset.tone = tone;
    };

    const renderServer = () => {
        if (els.serverChip) {
            els.serverChip.textContent = state.serverUrl.replace(/^https?:\/\//, '');
        }
    };

    const formatKey = (key) => state.revealedKeys.has(key.id)
        ? `<span class="key-mask revealed">${escapeHTML(key.key)}</span>`
        : '<span class="key-mask">••••••••••••</span>';

    const renderTable = () => {
        const query = state.search.trim().toLowerCase();
        const keys = state.keys.filter((key) => {
            if (!query) return true;
            return [key.id, key.name, key.key].some((value) => String(value || '').toLowerCase().includes(query));
        });

        state.filteredKeys = keys;
        els.keysCount.textContent = `${keys.length} total`;

        if (!keys.length) {
            els.keysTableBody.innerHTML = '<tr><td colspan="4"><div class="empty-state">No keys match the current search.</div></td></tr>';
            return;
        }

        els.keysTableBody.innerHTML = keys.map((key) => `
            <tr>
                <td>${escapeHTML(key.id)}</td>
                <td>${escapeHTML(key.name)}</td>
                <td>${formatKey(key)}</td>
                <td>
                    <div class="table-actions">
                        <button class="button-secondary" type="button" data-key-action="toggle" data-key-id="${escapeHTML(key.id)}">${state.revealedKeys.has(key.id) ? 'Hide' : 'Show'}</button>
                        <button class="button-secondary" type="button" data-key-action="copy" data-key-id="${escapeHTML(key.id)}">Copy</button>
                        <button class="button-secondary" type="button" data-key-action="edit" data-key-id="${escapeHTML(key.id)}">Edit</button>
                        <button class="button" type="button" data-key-action="delete" data-key-id="${escapeHTML(key.id)}">Delete</button>
                    </div>
                </td>
            </tr>
        `).join('');
    };

    const loadKeys = async () => {
        const payload = await loadJSON('/api/keys');
        state.keys = payload.keys || [];
        state.keys.sort((a, b) => (a.name || '').localeCompare(b.name || '', undefined, { sensitivity: 'base' }));
        renderTable();
    };

    const openModal = (key = null) => {
        state.editingKey = key;
        els.keyModal.classList.add('is-open');
        els.keyModal.classList.remove('hidden');
        els.keyModal.setAttribute('aria-hidden', 'false');
        els.keyModalTitle.textContent = key ? 'Replace key' : 'Create key';
        els.saveKeyButton.textContent = key ? 'Replace key' : 'Save key';
        els.keyName.value = key?.name || '';
        els.keyValue.value = key?.key || '';
        setMessage('');
        els.keyName.focus();
    };

    const closeModal = () => {
        els.keyModal.classList.remove('is-open');
        els.keyModal.classList.add('hidden');
        els.keyModal.setAttribute('aria-hidden', 'true');
        state.editingKey = null;
        setMessage('');
    };

    const saveKey = async () => {
        const name = els.keyName.value.trim();
        const key = els.keyValue.value.trim();
        if (!name || !key) {
            setMessage('Name and key are required.', 'error');
            return;
        }

        setMessage(state.editingKey ? 'Updating key…' : 'Creating key…');

        try {
            if (state.editingKey) {
                await apiFetch('/api/keys', {
                    method: 'DELETE',
                    body: JSON.stringify({ id: state.editingKey.id }),
                });
            }

            const response = await apiFetch('/api/keys', {
                method: 'POST',
                body: JSON.stringify({ name, key }),
            });

            if (!response.ok) {
                const payload = await response.json().catch(() => ({}));
                throw new Error(payload.message || 'Unable to save key.');
            }

            await loadKeys();
            closeModal();
        } catch (error) {
            setMessage(error.message || 'Unable to save key.', 'error');
        }
    };

    const deleteKey = async (id) => {
        if (!confirm('Delete this key?')) return;
        try {
            const response = await apiFetch('/api/keys', {
                method: 'DELETE',
                body: JSON.stringify({ id }),
            });
            if (!response.ok && response.status !== 204) {
                const payload = await response.json().catch(() => ({}));
                throw new Error(payload.message || 'Unable to delete key.');
            }
            state.revealedKeys.delete(id);
            await loadKeys();
        } catch (error) {
            setMessage(error.message || 'Unable to delete key.', 'error');
        }
    };

    const copyKey = async (id) => {
        const key = state.keys.find((entry) => entry.id === id);
        if (!key) return;
        try {
            await navigator.clipboard.writeText(key.key);
            // Show a temporary confirmation
            const button = document.querySelector(`[data-key-action="copy"][data-key-id="${escapeHTML(id)}"]`);
            if (button) {
                const originalText = button.textContent;
                button.textContent = 'Copied!';
                setTimeout(() => {
                    button.textContent = originalText;
                }, 2000);
            }
        } catch (error) {
            console.error('Failed to copy key:', error);
            // Fallback for older browsers or secure context issues
            const textArea = document.createElement('textarea');
            textArea.value = key.key;
            document.body.appendChild(textArea);
            textArea.select();
            try {
                document.execCommand('copy');
                const button = document.querySelector(`[data-key-action="copy"][data-key-id="${escapeHTML(id)}"]`);
                if (button) {
                    const originalText = button.textContent;
                    button.textContent = 'Copied!';
                    setTimeout(() => {
                        button.textContent = originalText;
                    }, 2000);
                }
            } catch (fallbackError) {
                console.error('Fallback copy also failed:', fallbackError);
            }
            document.body.removeChild(textArea);
        }
    };

    const initializeTheme = () => {
        setTheme(getTheme());
        els.themeToggle.addEventListener('click', () => {
            setTheme(getTheme() === 'dark' ? 'light' : 'dark');
        });
    };

    const initializeLogout = () => {
        els.logoutButton.addEventListener('click', () => {
            localStorage.removeItem(STORAGE_KEYS.token);
            redirectToLogin();
        });
    };

    const initializeHandlers = () => {
        els.keySearch.addEventListener('input', (event) => {
            state.search = event.target.value;
            renderTable();
        });

        els.refreshKeysButton.addEventListener('click', loadKeys);
        els.createKeyButton.addEventListener('click', () => openModal());
        els.closeKeyModalButton.addEventListener('click', closeModal);
        els.cancelKeyModalButton.addEventListener('click', closeModal);
        els.keyModal.addEventListener('click', (event) => {
            if (event.target === els.keyModal) closeModal();
        });
        els.saveKeyButton.addEventListener('click', saveKey);

        els.keysTableBody.addEventListener('click', (event) => {
            const button = event.target.closest('[data-key-action]');
            if (!button) return;
            const id = button.dataset.keyId;
            const key = state.keys.find((entry) => entry.id === id);
            if (!key) return;

            switch (button.dataset.keyAction) {
                case 'toggle':
                    if (state.revealedKeys.has(id)) state.revealedKeys.delete(id); else state.revealedKeys.add(id);
                    renderTable();
                    break;
                case 'copy':
                    copyKey(id);
                    break;
                case 'edit':
                    openModal(key);
                    break;
                case 'delete':
                    deleteKey(id);
                    break;
            }
        });
    };

    const cacheElements = () => {
        els.themeToggle = document.getElementById('themeToggle');
        els.logoutButton = document.getElementById('logoutButton');
        els.serverChip = document.getElementById('serverChip');
        els.refreshKeysButton = document.getElementById('refreshKeysButton');
        els.createKeyButton = document.getElementById('createKeyButton');
        els.keySearch = document.getElementById('keySearch');
        els.keysCount = document.getElementById('keysCount');
        els.keysTableBody = document.getElementById('keysTableBody');
        els.keyModal = document.getElementById('keyModal');
        els.closeKeyModalButton = document.getElementById('closeKeyModalButton');
        els.cancelKeyModalButton = document.getElementById('cancelKeyModalButton');
        els.keyModalTitle = document.getElementById('keyModalTitle');
        els.keyModalMessage = document.getElementById('keyModalMessage');
        els.keyName = document.getElementById('keyName');
        els.keyValue = document.getElementById('keyValue');
        els.saveKeyButton = document.getElementById('saveKeyButton');
    };

    const main = async () => {
        if (!requireAuth()) return;
        cacheElements();
        initializeTheme();
        initializeLogout();
        initializeHandlers();
        renderServer();
        try {
            await loadKeys();
        } catch (error) {
            setMessage(error.message || 'Unable to load keys.', 'error');
            els.keysTableBody.innerHTML = '<tr><td colspan="4"><div class="empty-state">Unable to load keys.</div></td></tr>';
        }
    };

    document.addEventListener('DOMContentLoaded', main);
})();
