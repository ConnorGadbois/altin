(() => {
    const STORAGE_KEYS = {
        theme: 'altin.theme',
        token: 'altin.token',
        serverUrl: 'altin.serverUrl',
    };

    const state = {
        serverUrl: '',
        token: '',
        agents: [],
        tasks: [],
        tasksUnavailable: false,
    };

    const els = {};

    const palette = [
        '#d4af37', '#7aa2f7', '#8bd17c', '#e88f6a', '#c97ff5', '#55c0b7', '#f0c674', '#d46a6a'
    ];

    // An agent that checked in more recently than this counts as active. Tombili's default
    // check-in interval is 10s with 5s of jitter, so 60s tolerates a few missed callbacks
    // while still flagging agents whose sleep interval has been raised via setsleep.
    const ACTIVE_WINDOW_MS = 300 * 1000;

    // Shown instead of a number when a metric's source endpoint failed, so a 0 is never
    // mistaken for "none exist".
    const UNAVAILABLE = '—';

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
            els.themeToggle.setAttribute('aria-label', `Switch to ${nextTheme === 'dark' ? 'light' : 'dark'} theme`);
        }
    };

    const getTheme = () => localStorage.getItem(STORAGE_KEYS.theme) || 'dark';

    // The header carries no progress or success text any more - only failures, which
    // is what used to get silently dropped when these elements were missing. The
    // element starts empty and .page-error:empty hides it, so a healthy load leaves
    // no residue under the page title.
    const setError = (message) => {
        if (!els.pageError) return;
        els.pageError.textContent = message;
    };

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
            throw new Error(payload.message || `Request failed (${response.status}) for ${path}`);
        }
        return payload;
    };

    const formatNumber = (value) => new Intl.NumberFormat().format(value || 0);

    // The server writes naive *local* timestamps (datetime.now()) but serialises them
    // with a GMT/UTC label, so parsing that label as UTC makes every timestamp look
    // hours in the past. Here that silently pinned every live agent to "Inactive".
    //
    // Rule: a real numeric offset (or Z) is authoritative and is trusted as-is. A bare
    // GMT/UTC label on an otherwise naive value is that bug, so the wall-clock fields
    // are re-read as local time instead.
    //
    // This is only exact when the browser and the server share a timezone, which holds
    // for single-host deployments. The real fix is datetime.now(timezone.utc) in
    // server/altin/agent.py.
    const parseServerTime = (value) => {
        if (!value) return NaN;
        if (typeof value === 'number') return value;
        if (/^\d{10,}$/.test(String(value).trim())) return Number(value);

        const text = String(value).trim();

        if (/[+-]\d{2}:?\d{2}$/.test(text) || /Z$/i.test(text)) {
            return new Date(text).getTime();
        }

        const iso = text.match(/^(?:[A-Za-z]{3},?\s*)?(\d{4})-(\d{2})-(\d{2})[T ](\d{2}):(\d{2}):(\d{2})/);
        if (iso) {
            return new Date(+iso[1], +iso[2] - 1, +iso[3], +iso[4], +iso[5], +iso[6]).getTime();
        }

        const rfc = text.match(/^[A-Za-z]{3},?\s*(\d{1,2})\s+([A-Za-z]{3})\s+(\d{4})\s+(\d{2}):(\d{2}):(\d{2})/);
        if (rfc) {
            const months = ['jan', 'feb', 'mar', 'apr', 'may', 'jun', 'jul', 'aug', 'sep', 'oct', 'nov', 'dec'];
            const month = months.indexOf(rfc[2].toLowerCase());
            if (month >= 0) {
                return new Date(+rfc[3], month, +rfc[1], +rfc[4], +rfc[5], +rfc[6]).getTime();
            }
        }

        return new Date(text).getTime();
    };

    const formatTimeAgo = (value) => {
        const time = parseServerTime(value);
        if (!Number.isFinite(time)) return 'unknown';

        const delta = Math.max(0, Date.now() - time);
        const minutes = Math.floor(delta / 60000);
        const hours = Math.floor(minutes / 60);
        const days = Math.floor(hours / 24);

        if (days > 0) return `${days}d ago`;
        if (hours > 0) return `${hours}h ago`;
        if (minutes > 0) return `${minutes}m ago`;
        return 'just now';
    };

    const isAgentActive = (agent) => {
        const lastSeen = parseServerTime(agent.last_checkin);
        if (!Number.isFinite(lastSeen)) return false;
        return (Date.now() - lastSeen) < ACTIVE_WINDOW_MS;
    };

    const buildCountMap = (items, keyFn) => {
        const map = new Map();
        items.forEach((item) => {
            const key = keyFn(item) || 'unknown';
            map.set(key, (map.get(key) || 0) + 1);
        });
        return [...map.entries()].sort((a, b) => b[1] - a[1]);
    };

    const renderMetrics = () => {
        const sent = state.tasks.filter((task) => task.sent).length;
        const completed = state.tasks.filter((task) => task.completed).length;
        const active = state.agents.filter(isAgentActive).length;
        const inactive = state.agents.length - active;
        // A 0 here would read as "no tasks exist" when the truth is "we could not ask".
        const taskValue = state.tasksUnavailable ? UNAVAILABLE : formatNumber(state.tasks.length);
        const taskNote = state.tasksUnavailable ? 'task endpoint returned an error' : null;

        els.metricAgents.textContent = formatNumber(state.agents.length);
        els.metricActive.textContent = formatNumber(active);
        els.metricInactive.textContent = formatNumber(inactive);
        els.metricTasks.textContent = taskValue;
        els.metricSent.textContent = state.tasksUnavailable ? UNAVAILABLE : formatNumber(sent);
        els.metricCompleted.textContent = state.tasksUnavailable ? UNAVAILABLE : formatNumber(completed);
    };

    const drawPieChart = (canvas, data) => {
        const ctx = canvas.getContext('2d');
        const dpr = Math.max(1, window.devicePixelRatio || 1);
        const size = Math.floor(canvas.clientWidth);

        canvas.width = size * dpr;
        canvas.height = size * dpr;
        ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
        ctx.clearRect(0, 0, size, size);

        const total = data.reduce((sum, item) => sum + item.value, 0);
        const center = size / 2;
        const radius = size * 0.36;
        let startAngle = -Math.PI / 2;

        ctx.save();
        ctx.translate(center, center);

        if (!total) {
            ctx.beginPath();
            ctx.arc(0, 0, radius, 0, Math.PI * 2);
            ctx.fillStyle = 'rgba(255,255,255,0.03)';
            ctx.fill();
            ctx.lineWidth = 1;
            ctx.strokeStyle = 'rgba(255,255,255,0.08)';
            ctx.stroke();
            ctx.restore();
            return;
        }

        data.forEach((item, index) => {
            const angle = (item.value / total) * Math.PI * 2;
            ctx.beginPath();
            ctx.moveTo(0, 0);
            ctx.arc(0, 0, radius, startAngle, startAngle + angle);
            ctx.closePath();
            ctx.fillStyle = palette[index % palette.length];
            ctx.fill();
            startAngle += angle;
        });

        ctx.beginPath();
        ctx.arc(0, 0, radius * 0.58, 0, Math.PI * 2);
        ctx.fillStyle = getComputedStyle(document.documentElement).getPropertyValue('--bg-panel');
        ctx.fill();

        ctx.restore();
    };

    const renderDonut = ({ canvas, legend, totalChip, centerTotal, subtitle, keyFn, caption, emptyLabel }) => {
        const counts = buildCountMap(state.agents, keyFn);
        const total = state.agents.length;
        const slices = counts.map(([name, value]) => ({ name, value }));

        totalChip.textContent = formatNumber(total);
        centerTotal.textContent = formatNumber(total);
        subtitle.textContent = total ? caption : 'no agents registered';

        if (!slices.length) {
            legend.innerHTML = `<div class="empty-state">${emptyLabel}</div>`;
            drawPieChart(canvas, []);
            return;
        }

        legend.innerHTML = slices.map((item, index) => `
            <div class="legend-item">
                <div class="legend-key">
                    <span class="legend-swatch" style="background:${palette[index % palette.length]}"></span>
                    <span class="legend-name">${escapeHTML(item.name)}</span>
                </div>
                <span class="legend-count">${item.value}</span>
            </div>
        `).join('');

        drawPieChart(canvas, slices);
    };

    const renderCharts = () => {
        renderDonut({
            canvas: els.chartCanvas,
            legend: els.legendList,
            totalChip: els.chartTotal,
            centerTotal: els.chartTotalCenter,
            subtitle: els.chartSubtitle,
            keyFn: (agent) => agent.implant_id,
            caption: 'agents by implant type',
            emptyLabel: 'No implant types available yet.',
        });

        renderDonut({
            canvas: els.osChartCanvas,
            legend: els.osLegendList,
            totalChip: els.osChartTotal,
            centerTotal: els.osChartTotalCenter,
            subtitle: els.osChartSubtitle,
            keyFn: (agent) => agent.os,
            caption: 'agents by operating system',
            emptyLabel: 'No operating systems available yet.',
        });
    };

    const renderRecentAgents = () => {
        const recent = [...state.agents].sort((a, b) => new Date(b.last_checkin) - new Date(a.last_checkin)).slice(0, 6);

        if (!recent.length) {
            els.recentList.innerHTML = '<div class="empty-state">No agents have checked in yet.</div>';
            return;
        }

        els.recentList.innerHTML = recent.map((agent) => `
            <article class="recent-item">
                <div class="recent-topline">
                    <div class="recent-strong">${agent.ip}</div>
                    <span class="status-chip">${agent.os}</span>
                </div>
                <div class="recent-meta">
                    <span>${agent.implant_id}</span>
                    <span>${formatTimeAgo(agent.last_checkin)}</span>
                    <span>${(agent.tags || []).length} tags</span>
                </div>
            </article>
        `).join('');
    };

    const renderHeaderMeta = () => {
        const server = state.serverUrl.replace(/^https?:\/\//, '');
        if (els.serverChip) {
            els.serverChip.textContent = server;
        }
    };

    const sortAgents = () => {
        state.agents.sort((a, b) => {
            const ipCompare = (a.ip || '').localeCompare(b.ip || '', undefined, { numeric: true, sensitivity: 'base' });
            if (ipCompare !== 0) return ipCompare;
            return (a.implant_id || '').localeCompare(b.implant_id || '', undefined, { sensitivity: 'base' });
        });
    };

    const loadOverview = async () => {
        setError('');
        state.tasksUnavailable = false;

        // Settled rather than all: the agents and tasks endpoints are independent, and one
        // failing must not discard the other's data. /api/tasks in particular 500s whenever a
        // task references a deleted agent, which would otherwise blank the whole dashboard.
        const [agentsOutcome, tasksOutcome] = await Promise.allSettled([
            loadJSON('/api/agents'),
            loadJSON('/api/tasks'),
        ]);

        const agentsFailed = agentsOutcome.status === 'rejected';
        const tasksFailed = tasksOutcome.status === 'rejected';

        state.agents = agentsFailed ? [] : (agentsOutcome.value.agents || []);
        if (!agentsFailed) sortAgents();

        state.tasks = tasksFailed ? [] : (tasksOutcome.value.tasks || []);
        state.tasksUnavailable = tasksFailed;

        renderMetrics();
        renderCharts();
        renderRecentAgents();
        renderHeaderMeta();

        if (agentsFailed) {
            setError(agentsOutcome.reason?.message || 'Failed to load agents.');
            els.legendList.innerHTML = '<div class="empty-state">Unable to load overview data.</div>';
            els.osLegendList.innerHTML = '<div class="empty-state">Unable to load overview data.</div>';
            els.recentList.innerHTML = '<div class="empty-state">Unable to load recent agents.</div>';
            return;
        }

        // A task failure is not fatal - the task cards fall back to an em dash and the
        // rest of the dashboard is still accurate - so it is reported here rather than
        // blanking the page.
        if (tasksFailed) {
            setError(tasksOutcome.reason?.message || 'Failed to load tasks.');
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

    const cacheElements = () => {
        els.themeToggle = document.getElementById('themeToggle');
        els.logoutButton = document.getElementById('logoutButton');
        els.pageError = document.getElementById('pageError');
        els.serverChip = document.getElementById('serverChip');
        els.metricAgents = document.getElementById('metricAgents');
        els.metricActive = document.getElementById('metricActive');
        els.metricInactive = document.getElementById('metricInactive');
        els.metricTasks = document.getElementById('metricTasks');
        els.metricSent = document.getElementById('metricSent');
        els.metricCompleted = document.getElementById('metricCompleted');
        els.chartCanvas = document.getElementById('chartCanvas');
        els.chartTotal = document.getElementById('chartTotal');
        els.chartTotalCenter = document.getElementById('chartTotalCenter');
        els.chartSubtitle = document.getElementById('chartSubtitle');
        els.legendList = document.getElementById('legendList');
        els.osChartCanvas = document.getElementById('osChartCanvas');
        els.osChartTotal = document.getElementById('osChartTotal');
        els.osChartTotalCenter = document.getElementById('osChartTotalCenter');
        els.osChartSubtitle = document.getElementById('osChartSubtitle');
        els.osLegendList = document.getElementById('osLegendList');
        els.recentList = document.getElementById('recentList');
    };

    const main = async () => {
        if (!requireAuth()) {
            return;
        }

        cacheElements();
        initializeTheme();
        initializeLogout();
        await loadOverview();
        window.addEventListener('resize', renderCharts);
    };

    document.addEventListener('DOMContentLoaded', main);
})();
