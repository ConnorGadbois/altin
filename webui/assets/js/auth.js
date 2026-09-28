(() => {
    const STORAGE_KEYS = {
        theme: 'altin.theme',
        token: 'altin.token',
        serverUrl: 'altin.serverUrl',
        savedServers: 'altin.savedServers',
    };

    const state = {
        pendingLogin: null,
    };

    const els = {};

    const readJSON = (key, fallback) => {
        try {
            const raw = localStorage.getItem(key);
            return raw ? JSON.parse(raw) : fallback;
        } catch {
            return fallback;
        }
    };

    const writeJSON = (key, value) => {
        localStorage.setItem(key, JSON.stringify(value));
    };

    const normalizeServer = (value) => value.trim().replace(/\/+$/, '');

    const getTheme = () => localStorage.getItem(STORAGE_KEYS.theme) || 'dark';

    const setTheme = (theme) => {
        const nextTheme = theme === 'light' ? 'light' : 'dark';
        document.documentElement.setAttribute('data-theme', nextTheme);
        localStorage.setItem(STORAGE_KEYS.theme, nextTheme);
        if (els.themeToggle) {
            els.themeToggle.textContent = nextTheme === 'dark' ? 'Light' : 'Dark';
            els.themeToggle.setAttribute('aria-label', `Switch to ${nextTheme === 'dark' ? 'light' : 'dark'} theme`);
        }
    };

    const showMessage = (message, tone = '') => {
        if (!els.loginMessage) return;
        els.loginMessage.textContent = message;
        els.loginMessage.dataset.tone = tone;
    };

    const getSavedServers = () => {
        const servers = readJSON(STORAGE_KEYS.savedServers, []);
        return Array.isArray(servers) ? servers : [];
    };

    const saveServerEntry = ({ url, username }) => {
        const servers = getSavedServers();
        const normalizedUrl = normalizeServer(url);
        const existingIndex = servers.findIndex((entry) => entry.url === normalizedUrl);
        const entry = {
            url: normalizedUrl,
            label: normalizedUrl,
            lastUsed: new Date().toISOString(),
        };

        if (existingIndex >= 0) {
            servers[existingIndex] = entry;
        } else {
            servers.unshift(entry);
        }

        writeJSON(STORAGE_KEYS.savedServers, servers.slice(0, 10));
    };

    const renderSavedServers = () => {
        const servers = getSavedServers();
        if (!els.savedServerWrap || !els.savedServer) return;

        if (!servers.length) {
            els.savedServerWrap.classList.add('hidden');
            els.savedServer.innerHTML = '';
            return;
        }

        els.savedServerWrap.classList.remove('hidden');
        els.savedServer.innerHTML = [
            '<option value="">Select a saved server</option>',
            ...servers.map((entry, index) => `<option value="${index}">${entry.label || entry.url}</option>`),
        ].join('');
    };

    const applyServerSelection = (index) => {
        const servers = getSavedServers();
        if (index === '' || index == null) return;
        const entry = servers[Number(index)];
        if (!entry) return;

        els.serverUrl.value = entry.url;
        showMessage(`Loaded ${entry.label || entry.url}`, '');
    };

    const goHome = () => {
        window.location.href = '/index.html';
    };

    const persistAuth = ({ token, url, username }) => {
        localStorage.setItem(STORAGE_KEYS.token, token);
        localStorage.setItem(STORAGE_KEYS.serverUrl, normalizeServer(url));
    };

    const login = async (event) => {
        event.preventDefault();
        showMessage('Signing in…');

        const url = normalizeServer(els.serverUrl.value);
        const username = els.username.value.trim();
        const password = els.password.value;

        if (!url || !username || !password) {
            showMessage('All fields are required.', 'error');
            return;
        }

        try {
            const response = await fetch(`${url}/api/login`, {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json',
                },
                body: JSON.stringify({ username, password }),
            });

            const payload = await response.json().catch(() => ({}));

            if (!response.ok) {
                throw new Error(payload.message || 'Login failed.');
            }

            persistAuth({ token: payload.token, url, username });
            if (els.rememberServer?.checked) {
                saveServerEntry({ url, username });
                renderSavedServers();
            }

            showMessage('Login successful.', 'success');
            goHome();
        } catch (error) {
            showMessage(error.message || 'Unable to reach the server.', 'error');
        }
    };

    const initializeThemeToggle = () => {
        setTheme(getTheme());
        els.themeToggle?.addEventListener('click', () => {
            setTheme(getTheme() === 'dark' ? 'light' : 'dark');
        });
    };

    const initializeSavedServerPicker = () => {
        renderSavedServers();
        els.savedServer?.addEventListener('change', (event) => applyServerSelection(event.target.value));
    };

    const initializeLoginHandlers = () => {
        els.loginForm?.addEventListener('submit', login);
    };

    const restoreDefaults = () => {
        const serverUrl = localStorage.getItem(STORAGE_KEYS.serverUrl);

        if (serverUrl) {
            els.serverUrl.value = serverUrl;
        }
    };

    const cacheElements = () => {
        els.themeToggle = document.getElementById('themeToggle');
        els.loginForm = document.getElementById('loginForm');
        els.serverUrl = document.getElementById('serverUrl');
        els.username = document.getElementById('username');
        els.password = document.getElementById('password');
        els.loginMessage = document.getElementById('loginMessage');
        els.savedServerWrap = document.getElementById('savedServerWrap');
        els.savedServer = document.getElementById('savedServer');
        els.rememberServer = document.getElementById('rememberServer');
    };

    document.addEventListener('DOMContentLoaded', () => {
        cacheElements();
        initializeThemeToggle();
        restoreDefaults();
        initializeSavedServerPicker();
        initializeLoginHandlers();
        showMessage('Choose a server and sign in to continue.');
    });
})();
