(() => {
    const STORAGE_KEYS = {
        theme: 'altin.theme',
        token: 'altin.token',
        serverUrl: 'altin.serverUrl',
    };

    const state = {
        serverUrl: '',
        token: '',
        users: [],
        search: '',
        editingUser: null,
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

        try {
            const payloadPart = token.split('.')[1] || '';
            const base64 = payloadPart.replace(/-/g, '+').replace(/_/g, '/');
            const padded = base64 + '='.repeat((4 - (base64.length % 4 || 4)) % 4);
            const payload = JSON.parse(atob(padded));
            if (!payload.admin) {
                window.location.href = '/index.html';
                return false;
            }
        } catch {
            window.location.href = '/index.html';
            return false;
        }

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
        if (!els.userModalMessage) return;
        els.userModalMessage.textContent = message;
        els.userModalMessage.dataset.tone = tone;
    };

    const renderServer = () => {
        if (els.serverChip) {
            els.serverChip.textContent = state.serverUrl.replace(/^https?:\/\//, '');
        }
    };

    const renderTable = () => {
        const query = state.search.trim().toLowerCase();
        const users = state.users.filter((user) => {
            if (!query) return true;
            return [user.id, user.username].some((value) => String(value || '').toLowerCase().includes(query));
        });

        els.usersCount.textContent = `${users.length} total`;

        if (!users.length) {
            els.usersTableBody.innerHTML = '<tr><td colspan="4"><div class="empty-state">No users match the current search.</div></td></tr>';
            return;
        }

        els.usersTableBody.innerHTML = users.map((user) => `
            <tr>
                <td>${escapeHTML(user.id)}</td>
                <td>${escapeHTML(user.username)}</td>
                <td>${user.admin ? '<span class="admin-chip">Admin</span>' : '<span class="user-chip">User</span>'}</td>
                <td>
                    <div class="table-actions">
                        <button class="button-secondary" type="button" data-user-action="edit" data-user-id="${escapeHTML(user.id)}">Edit</button>
                        <button class="button" type="button" data-user-action="delete" data-user-id="${escapeHTML(user.id)}">Delete</button>
                    </div>
                </td>
            </tr>
        `).join('');
    };

    const loadUsers = async () => {
        const payload = await loadJSON('/api/users');
        state.users = payload.users || [];
        state.users.sort((a, b) => (a.username || '').localeCompare(b.username || '', undefined, { sensitivity: 'base' }));
        renderTable();
    };

    const openModal = (user = null) => {
        state.editingUser = user;
        els.userModal.classList.add('is-open');
        els.userModal.classList.remove('hidden');
        els.userModal.setAttribute('aria-hidden', 'false');
        els.userModalTitle.textContent = user ? 'Edit user' : 'Create user';
        els.saveUserButton.textContent = user ? 'Update user' : 'Save user';
        els.userName.value = user?.username || '';
        els.userPassword.value = '';
        els.userAdmin.checked = Boolean(user?.admin);
        setMessage('');
        els.userName.focus();
    };

    const closeModal = () => {
        els.userModal.classList.remove('is-open');
        els.userModal.classList.add('hidden');
        els.userModal.setAttribute('aria-hidden', 'true');
        state.editingUser = null;
        setMessage('');
    };

    const saveUser = async () => {
        const username = els.userName.value.trim();
        const password = els.userPassword.value;
        const admin = els.userAdmin.checked;

        if (!username) {
            setMessage('Username is required.', 'error');
            return;
        }

        setMessage(state.editingUser ? 'Updating user…' : 'Creating user…');

        try {
            if (state.editingUser) {
                if (username !== state.editingUser.username) {
                    const response = await apiFetch('/api/users', {
                        method: 'PATCH',
                        body: JSON.stringify({ id: state.editingUser.id, username }),
                    });
                    if (!response.ok) throw new Error((await response.json().catch(() => ({}))).message || 'Unable to update user.');
                }
                if (password) {
                    const response = await apiFetch('/api/users', {
                        method: 'PATCH',
                        body: JSON.stringify({ id: state.editingUser.id, password }),
                    });
                    if (!response.ok) throw new Error((await response.json().catch(() => ({}))).message || 'Unable to update user.');
                }
                if (admin !== Boolean(state.editingUser.admin)) {
                    const response = await apiFetch('/api/users', {
                        method: 'PATCH',
                        body: JSON.stringify({ id: state.editingUser.id, admin }),
                    });
                    if (!response.ok) throw new Error((await response.json().catch(() => ({}))).message || 'Unable to update user.');
                }
            } else {
                if (!password) {
                    setMessage('Password is required.', 'error');
                    return;
                }
                const response = await apiFetch('/api/users', {
                    method: 'POST',
                    body: JSON.stringify({ username, password, admin }),
                });
                if (!response.ok) throw new Error((await response.json().catch(() => ({}))).message || 'Unable to create user.');
            }

            await loadUsers();
            closeModal();
        } catch (error) {
            setMessage(error.message || 'Unable to save user.', 'error');
        }
    };

    const deleteUser = async (id) => {
        if (!confirm('Delete this user?')) return;
        try {
            const response = await apiFetch('/api/users', {
                method: 'DELETE',
                body: JSON.stringify({ id }),
            });
            if (!response.ok && response.status !== 204) {
                const payload = await response.json().catch(() => ({}));
                throw new Error(payload.message || 'Unable to delete user.');
            }
            await loadUsers();
        } catch (error) {
            setMessage(error.message || 'Unable to delete user.', 'error');
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
        els.userSearch.addEventListener('input', (event) => {
            state.search = event.target.value;
            renderTable();
        });

        els.refreshUsersButton.addEventListener('click', loadUsers);
        els.createUserButton.addEventListener('click', () => openModal());
        els.closeUserModalButton.addEventListener('click', closeModal);
        els.cancelUserModalButton.addEventListener('click', closeModal);
        els.userModal.addEventListener('click', (event) => {
            if (event.target === els.userModal) closeModal();
        });
        els.saveUserButton.addEventListener('click', saveUser);

        els.usersTableBody.addEventListener('click', (event) => {
            const button = event.target.closest('[data-user-action]');
            if (!button) return;
            const id = button.dataset.userId;
            const user = state.users.find((entry) => entry.id === id);
            if (!user) return;

            switch (button.dataset.userAction) {
                case 'edit':
                    openModal(user);
                    break;
                case 'delete':
                    deleteUser(id);
                    break;
            }
        });
    };

    const cacheElements = () => {
        els.themeToggle = document.getElementById('themeToggle');
        els.logoutButton = document.getElementById('logoutButton');
        els.serverChip = document.getElementById('serverChip');
        els.refreshUsersButton = document.getElementById('refreshUsersButton');
        els.createUserButton = document.getElementById('createUserButton');
        els.userSearch = document.getElementById('userSearch');
        els.usersCount = document.getElementById('usersCount');
        els.usersTableBody = document.getElementById('usersTableBody');
        els.userModal = document.getElementById('userModal');
        els.closeUserModalButton = document.getElementById('closeUserModalButton');
        els.cancelUserModalButton = document.getElementById('cancelUserModalButton');
        els.userModalTitle = document.getElementById('userModalTitle');
        els.userModalMessage = document.getElementById('userModalMessage');
        els.userName = document.getElementById('userName');
        els.userPassword = document.getElementById('userPassword');
        els.userAdmin = document.getElementById('userAdmin');
        els.saveUserButton = document.getElementById('saveUserButton');
    };

    const main = async () => {
        if (!requireAuth()) return;
        cacheElements();
        initializeTheme();
        initializeLogout();
        initializeHandlers();
        renderServer();
        try {
            await loadUsers();
        } catch (error) {
            setMessage(error.message || 'Unable to load users.', 'error');
            els.usersTableBody.innerHTML = '<tr><td colspan="4"><div class="empty-state">Unable to load users.</div></td></tr>';
        }
    };

    document.addEventListener('DOMContentLoaded', main);
})();
