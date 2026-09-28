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
        filteredAgents: [],
        // One selection, owned by the table checkboxes. The bulk-task dialog is a
        // read-only view of it - a second selection set inside the dialog let the
        // dialog and the table disagree about who was about to be commanded.
        selectedIds: new Set(),
        filters: {
            search: '',
            implant: 'all',
            os: 'all',
        },
        bulkCommand: '',
        interactCommand: '',
    };

    const els = {};

    const normalizeServer = (value) => value.trim().replace(/\/+$/, '');

    const escapeHTML = (value) => String(value ?? '')
        .replace(/&/g, '&amp;')
        .replace(/</g, '&lt;')
        .replace(/>/g, '&gt;')
        .replace(/"/g, '&quot;')
        .replace(/'/g, '&#39;');

    const labelFor = (agent) => `${agent.implant_id}@${agent.ip}`;

    // The selected agents, in the order the table shows them, resolved against the
    // current agent list so a stale id cannot survive a refresh.
    const selectedAgents = () => state.agents.filter((agent) => state.selectedIds.has(agent.id));

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
        if (!els.bulkTaskMessage) return;
        els.bulkTaskMessage.textContent = message;
        els.bulkTaskMessage.dataset.tone = tone;
    };

    // Page-level failures, shown under the page title. This is deliberately not
    // setMessage: that writes into the bulk task modal's message area, which is
    // hidden unless the dialog is open, so a failed load used to report its
    // reason into a dialog nobody could see. Matches the #pageError used on
    // index.html.
    const setError = (message) => {
        if (!els.pageError) return;
        els.pageError.textContent = message;
    };

    // See the identical helper in app.js: the server labels naive local timestamps as
    // GMT, so a bare GMT/UTC label is re-read as local time instead of as UTC.
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

    const getCommands = (agent) => Array.isArray(agent.commands) ? agent.commands : [];

    // --- Agent interaction console -----------------------------------------
    // The management API creates tasks without returning their id, so a task we just
    // queued has to be identified by diffing the agent's task list around the POST.
    // Once it reports a result the task flips to completed, and the output is read back
    // from /api/task-results.
    const POLL_INTERVAL_MS = 2000;
    const TASK_TIMEOUT_MS = 90000;

    const console_ = {
        agentId: null,
        agent: null,
        entries: [],
        knownTaskIds: new Set(),
        seq: 0,
        open: false,
        sending: false,
    };

    // Command history is a read-only view of what has already been run against an
    // agent. Entries start collapsed and are remembered by task id, so re-rendering
    // after a filter change or a refresh does not snap everything shut again.
    const history_ = {
        agentId: null,
        agent: null,
        entries: [],
        expanded: new Set(),
        filter: '',
        open: false,
    };

    const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

    const interactMessage = (message, tone = '') => {
        if (!els.interactMessage) return;
        els.interactMessage.textContent = message;
        els.interactMessage.dataset.tone = tone;
    };

    // Show what was actually sent, preserving JSON types so the transcript reads like
    // the request body (setsleep 30 5, not "30" "5").
    const formatArgs = (args) => (args || [])
        .map((value) => (typeof value === 'string' ? value : JSON.stringify(value)))
        .join(' ');

    const renderInteractLog = () => {
        if (!els.interactLog) return;

        if (!console_.entries.length) {
            els.interactLog.innerHTML = '<div class="empty-state">No commands sent to this agent yet.</div>';
            return;
        }

        const wasPinned = els.interactLog.scrollHeight - els.interactLog.scrollTop - els.interactLog.clientHeight < 40;

        els.interactLog.innerHTML = console_.entries.map((entry) => {
            const output = (entry.outputs || []).map((result) =>
                `<pre class="console-output">${escapeHTML(result.result ?? '')}</pre>`
            ).join('');

            return `
                <article class="console-entry">
                    <div class="console-cmd">
                        <span class="console-prompt">${escapeHTML(console_.agent ? `${console_.agent.implant_id}@${console_.agent.ip}` : 'altin')} &gt;</span>
                        <span class="console-cmd-name">${escapeHTML(entry.command)}</span>
                        ${entry.argsText ? `<span class="console-cmd-args">${escapeHTML(entry.argsText)}</span>` : ''}
                        <span class="console-state" data-state="${entry.state}">${entry.state}</span>
                    </div>
                    ${output}
                    ${entry.note ? `<div class="console-note">${escapeHTML(entry.note)}</div>` : ''}
                </article>
            `;
        }).join('');

        if (wasPinned) {
            els.interactLog.scrollTop = els.interactLog.scrollHeight;
        }
    };

    const renderInteractCommand = () => {
        const commands = getCommands(console_.agent);
        const selected = commands.find((command) => command.command === state.interactCommand);

        els.interactCommandHelp.textContent = selected?.description || 'This agent declares no commands.';

        if (!commands.length) {
            els.interactArgsContainer.innerHTML = '<div class="empty-state">This agent exposes no commands.</div>';
            return;
        }

        const args = selected?.args || [];
        els.interactArgsContainer.innerHTML = args.length
            ? `<div class="arg-list">${args.map((arg, index) => `
                <div class="arg-item">
                    <label class="field-label" for="interactArg-${index}">${escapeHTML(arg.name)}${arg.required ? ' *' : ''}</label>
                    <input class="input" id="interactArg-${index}" type="text" data-interact-arg="${index}"
                        placeholder="${escapeHTML(arg.description || arg.name)}">
                    <div class="arg-help">${escapeHTML([arg.description, arg.arg_type].filter(Boolean).join(' · '))}</div>
                </div>`).join('')}</div>`
            : '<div class="arg-help">This command takes no arguments.</div>';
    };

    // The implant reads each argument with a typed accessor on the decoded JSON node
    // (args[i].getInt / getStr), and the server does NOT type-check - it only compares
    // the argument count. So a value sent as a JSON string reaches the agent as a
    // JString and the accessor yields garbage.
    //
    // Verified against Nim 2.2.4, which is what the implant builds with:
    //   getInt on JInt   -> the value        (correct)
    //   getInt on JFloat -> 0, silently      (no exception!)
    //   getInt on JStr   -> 0, silently      (no exception!)
    //   getFloat on JInt or JFloat -> the value (both fine)
    //   getStr on JStr   -> the value; on JInt/JFloat -> "" (silently)
    //
    // getInt is therefore the strict accessor, so whole numbers are always sent as JSON
    // integers even when the command declares "float" - a JSON 30.0 would decode to 0.
    const coerceArg = (raw, argType) => {
        const type = String(argType || 'str').toLowerCase();

        if (type === 'int' || type === 'integer' || type === 'long') {
            if (!/^[+-]?\d+$/.test(raw)) {
                return { error: `"${raw}" is not a valid integer.` };
            }
            return { value: Number.parseInt(raw, 10) };
        }

        if (type === 'float' || type === 'double' || type === 'number') {
            if (!/^[+-]?(\d+\.?\d*|\.\d+)([eE][+-]?\d+)?$/.test(raw)) {
                return { error: `"${raw}" is not a valid number.` };
            }
            const parsed = Number.parseFloat(raw);
            // Send a JSON integer when the value is whole: the implant's getInt accessor
            // silently yields 0 for a JSON float, so 30 must go over the wire as 30, not
            // 30.0. Genuinely fractional values (30.5) stay floats, which getFloat and
            // getInt-free code paths read correctly.
            return { value: Number.isInteger(parsed) ? Math.trunc(parsed) : parsed };
        }

        if (type === 'bool' || type === 'boolean') {
            const lowered = raw.toLowerCase();
            if (['true', '1', 'yes'].includes(lowered)) return { value: true };
            if (['false', '0', 'no'].includes(lowered)) return { value: false };
            return { error: `"${raw}" is not a valid boolean (use true or false).` };
        }

        return { value: raw };
    };

    // Reads the arg inputs for a command and coerces each to its declared type.
    // Shared by the interact console and the bulk-task modal so both send the same
    // JSON shapes. `fail` is called with a message and returns false on bad input.
    const collectTypedArgs = (container, schema, fail) => {
        const inputs = [...container.querySelectorAll('[data-interact-arg], [data-bulk-arg]')];
        const args = [];

        for (const input of inputs) {
            const raw = input.value.trim();
            // Blank optional args are skipped; a blank cannot be passed past this point
            // without shifting every later positional argument.
            if (raw === '') continue;

            const index = Number(input.dataset.interactArg ?? input.dataset.bulkArg);
            const arg = schema[index] || {};
            const { value, error } = coerceArg(raw, arg.arg_type);
            if (error) {
                fail(`${arg.name || 'Argument'}: ${error}`);
                input.focus();
                return null;
            }
            args.push(value);
        }

        return args;
    };

    const collectInteractArgs = () => {
        const command = getCommands(console_.agent).find((entry) => entry.command === state.interactCommand);
        if (!command) return null;

        const schema = command.args || [];
        const args = collectTypedArgs(els.interactArgsContainer, schema, interactMessage);
        if (!args) return null;

        // The API requires the argument count to equal the number of required arguments
        // exactly, so anything else is rejected with a 404.
        const required = schema.filter((arg) => arg.required).length;
        if (args.length !== required) {
            interactMessage(`This command needs exactly ${required} argument${required === 1 ? '' : 's'}.`, 'error');
            return null;
        }

        return { command, args };
    };

    const fetchAgentTasks = async () => {
        const payload = await loadJSON(`/api/tasks?agent=${encodeURIComponent(console_.agentId)}`);
        return payload.tasks || [];
    };

    // task_id filters on the *agent* parameter server-side, so the task id has to be
    // supplied as both. Omitting either one makes the endpoint return a 500.
    const fetchTaskOutput = async (taskId) => {
        const query = `task_id=${encodeURIComponent(taskId)}&agent=${encodeURIComponent(taskId)}`;
        const payload = await loadJSON(`/api/task-results?${query}`);
        return payload.results || [];
    };

    // Waits for one transcript entry to reach a terminal state. Entries fall into two
    // kinds: a task we just queued, whose id the API never told us and which therefore has
    // to be spotted by diffing the task list, and a task that was already on the server
    // when the console opened, whose id we already know.
    const pollForResult = async (entry, before) => {
        const deadline = Date.now() + TASK_TIMEOUT_MS;

        while (console_.open && Date.now() < deadline) {
            await sleep(POLL_INTERVAL_MS);
            if (!console_.open || entry.state === 'error' || entry.state === 'timeout') return;

            let tasks;
            try {
                tasks = await fetchAgentTasks();
            } catch (error) {
                // A 401 already triggered a redirect to the login page.
                if (error.message === 'Unauthorized') return;
                continue;
            }

            let task = null;

            if (entry.taskId) {
                task = tasks.find((candidate) => candidate.id === entry.taskId) || null;
                if (!task) {
                    entry.state = 'error';
                    entry.note = 'Task is no longer on the server.';
                    renderInteractLog();
                    return;
                }
            } else {
                const fresh = tasks.filter((candidate) => !before.has(candidate.id));
                const exact = fresh.find((candidate) =>
                    candidate.task === entry.command &&
                    JSON.stringify(candidate.args || []) === JSON.stringify(entry.args)
                );
                task = exact || fresh[0] || null;
                if (task) entry.taskId = task.id;
            }

            if (!task) continue;

            if (task.sent && entry.state === 'pending') {
                entry.state = 'sent';
                renderInteractLog();
            }

            if (!task.completed) continue;

            try {
                entry.outputs = await fetchTaskOutput(task.id);
                entry.state = 'done';
                if (!entry.outputs.length) {
                    entry.note = 'Task completed but the agent reported no result.';
                }
            } catch (error) {
                entry.state = 'error';
                entry.note = `Could not read output: ${error.message}`;
            }
            renderInteractLog();
            return;
        }

        if (console_.open && (entry.state === 'pending' || entry.state === 'sent')) {
            entry.state = 'timeout';
            entry.note = `No response after ${Math.round(TASK_TIMEOUT_MS / 1000)}s. The agent may be offline, ` +
                'asleep, or the command may not have completed.';
        }
        renderInteractLog();
    };

    const sendInteractTask = async () => {
        if (console_.sending) return;

        const collected = collectInteractArgs();
        if (!collected) return;

        const { command, args } = collected;
        console_.sending = true;
        els.sendInteractButton.disabled = true;
        interactMessage('Sending…');

        const entry = {
            key: ++console_.seq,
            command: command.command,
            args,
            argsText: formatArgs(args),
            state: 'pending',
            outputs: [],
            note: '',
        };

        let before;
        try {
            before = new Set((await fetchAgentTasks()).map((task) => task.id));
        } catch (error) {
            interactMessage(error.message || 'Could not read the agent task list.', 'error');
            console_.sending = false;
            els.sendInteractButton.disabled = false;
            return;
        }

        console_.entries.push(entry);
        renderInteractLog();

        try {
            const response = await apiFetch('/api/tasks', {
                method: 'POST',
                body: JSON.stringify({ agent_id: console_.agentId, command: command.command, args }),
            });

            if (!response.ok) {
                const payload = await response.json().catch(() => ({}));
                entry.state = 'error';
                entry.note = payload.message || `Server rejected the task (${response.status}).`;
                interactMessage(entry.note, 'error');
                renderInteractLog();
                return;
            }

            interactMessage('Task queued. Waiting for the agent to check in.', 'success');
            pollForResult(entry, before);
        } catch (error) {
            entry.state = 'error';
            entry.note = error.message || 'Failed to send the task.';
            interactMessage(entry.note, 'error');
            renderInteractLog();
        } finally {
            console_.sending = false;
            els.sendInteractButton.disabled = false;
        }
    };

    const openInteract = async (agentId) => {
        const summary = state.agents.find((agent) => agent.id === agentId);
        if (!summary) return;

        console_.agentId = agentId;
        console_.agent = summary;
        console_.entries = [];
        console_.seq = 0;
        console_.open = true;
        state.interactCommand = '';

        els.interactModal.classList.add('is-open');
        els.interactModal.classList.remove('hidden');
        els.interactModal.setAttribute('aria-hidden', 'false');
        els.interactTitle.textContent = 'Interact';
        els.interactSubtitle.textContent = `${summary.ip} · ${summary.os} · ${getCommands(summary).length} commands · last check-in ${formatTimeAgo(summary.last_checkin)}`;
        els.interactAgentChip.textContent = `${summary.implant_id}@${summary.ip}`;
        els.interactHint.textContent = 'Responses arrive when the agent next checks in, so this can take a few seconds.';
        interactMessage('');
        renderInteractLog();

        // Hold off sending until the command list and the existing task list are both
        // known, otherwise a command can be queued against a stale command list and then
        // show up twice once the seed below catches up.
        els.sendInteractButton.disabled = true;

        const commands = getCommands(summary);
        els.interactCommand.innerHTML = commands.length
            ? commands.map((command) => `<option value="${escapeHTML(command.command)}">${escapeHTML(command.command)}</option>`).join('')
            : '<option value="">No commands</option>';
        state.interactCommand = commands[0]?.command || '';
        els.interactCommand.value = state.interactCommand;
        renderInteractCommand();

        // Refresh from the API so the command list reflects the agent's latest
        // registration rather than whatever the table happened to be holding.
        try {
            const full = await loadJSON(`/api/agents/${encodeURIComponent(agentId)}`);
            console_.agent = full;

            const fresh = getCommands(full);
            els.interactCommand.innerHTML = fresh.length
                ? fresh.map((command) => `<option value="${escapeHTML(command.command)}">${escapeHTML(command.command)}</option>`).join('')
                : '<option value="">No commands</option>';
            state.interactCommand = fresh[0]?.command || '';
            els.interactCommand.value = state.interactCommand;
            renderInteractCommand();
        } catch (error) {
            interactMessage(error.message || 'Could not refresh agent details.', 'error');
        }

        // Surface tasks that were already queued before this session opened, since the
        // server re-delivers every incomplete task on the agent's next check-in. Each one
        // gets its own poller so it resolves instead of hanging at "pending" forever.
        try {
            const tasks = await fetchAgentTasks();
            console_.knownTaskIds = new Set(tasks.map((task) => task.id));

            tasks.filter((task) => !task.completed).forEach((task) => {
                console_.seq += 1;
                const entry = {
                    key: console_.seq,
                    command: task.task,
                    args: task.args || [],
                    argsText: formatArgs(task.args),
                    state: task.sent ? 'sent' : 'pending',
                    outputs: [],
                    taskId: task.id,
                    note: 'Queued before this console was opened.',
                };
                console_.entries.push(entry);
                pollForResult(entry, new Set());
            });

            renderInteractLog();
        } catch (error) {
            if (error.message !== 'Unauthorized') {
                interactMessage(error.message || 'Could not load existing tasks.', 'error');
            }
        } finally {
            if (console_.open) els.sendInteractButton.disabled = false;
        }
    };

    const closeInteract = () => {
        console_.open = false;
        console_.agent = null;
        console_.agentId = null;
        els.interactModal.classList.remove('is-open');
        els.interactModal.classList.add('hidden');
        els.interactModal.setAttribute('aria-hidden', 'true');
    };

    // ------------------------------------------------------------------ *
    // Command history
    // ------------------------------------------------------------------ */

    const historyMessage = (message, tone = '') => {
        if (!els.historyMessage) return;
        els.historyMessage.textContent = message;
        els.historyMessage.className = `modal-message${tone ? ` is-${tone}` : ''}`;
    };

    // The server exposes no way to scope results to one agent: get_taskresults
    // reads request.args['agent'] but compares it against TaskResult.task, so the
    // only filters available are "all results" (no params) or "one task" (task_id
    // passed as BOTH params, or the endpoint 500s). Fetching once and bucketing
    // locally keeps this at two requests instead of one per task.
    const fetchHistoryResults = async () => {
        const payload = await loadJSON('/api/task-results');
        return payload.results || [];
    };

    const historyState = (entry) => {
        if (entry.results.length) return 'done';
        return entry.sent ? 'sent' : 'pending';
    };

    // Newest first, with anything still awaiting a response pinned to the top
    // because that is what an operator opening this view is usually after.
    const sortHistory = (entries) => [...entries].sort((a, b) => {
        const aDone = a.state === 'done';
        const bDone = b.state === 'done';
        if (aDone !== bDone) return aDone ? 1 : -1;

        const at = parseServerTime(a.time);
        const bt = parseServerTime(b.time);
        const aValid = Number.isFinite(at);
        const bValid = Number.isFinite(bt);
        if (aValid !== bValid) return aValid ? -1 : 1;
        return (bValid ? bt : 0) - (aValid ? at : 0);
    });

    const buildHistoryEntries = (tasks, results) => {
        const byTask = new Map();

        for (const result of results) {
            const key = result.task;
            if (!byTask.has(key)) byTask.set(key, []);
            byTask.get(key).push(result);
        }

        return tasks.map((task) => {
            const matched = byTask.get(task.id) || [];
            // Oldest first inside an entry, so a command that reported more than
            // once reads in the order the agent produced it.
            const ordered = [...matched].sort((a, b) => parseServerTime(a.timestamp) - parseServerTime(b.timestamp));
            const latest = ordered.length ? ordered[ordered.length - 1].timestamp : null;

            return {
                taskId: task.id,
                command: task.task || 'unknown',
                args: task.args || [],
                argsText: formatArgs(task.args),
                sent: Boolean(task.sent),
                completed: Boolean(task.completed),
                results: ordered,
                time: latest,
                state: 'pending',
            };
        });
    };

    // Matches the command name or any argument text, so "shell" finds every shell
    // run and "passwd" finds the cat that read it.
    const historyMatches = (entry, needle) => {
        if (!needle) return true;
        return entry.command.toLowerCase().includes(needle)
            || entry.argsText.toLowerCase().includes(needle);
    };

    const renderHistory = () => {
        if (!els.historyList) return;

        const needle = history_.filter.trim().toLowerCase();
        const visible = history_.entries.filter((entry) => historyMatches(entry, needle));

        const total = history_.entries.length;
        els.historySummary.textContent = needle
            ? `${visible.length} of ${total} ${total === 1 ? 'command' : 'commands'}`
            : `${total} ${total === 1 ? 'command' : 'commands'}`;

        if (!total) {
            els.historyList.innerHTML = '<div class="empty-state">No commands have been sent to this agent yet.</div>';
            return;
        }

        if (!visible.length) {
            els.historyList.innerHTML = `<div class="empty-state">Nothing matches &ldquo;${escapeHTML(history_.filter.trim())}&rdquo;.</div>`;
            return;
        }

        els.historyList.innerHTML = sortHistory(visible).map((entry) => {
            const open = history_.expanded.has(entry.taskId);
            const label = `${entry.command}${entry.argsText ? ` ${entry.argsText}` : ''}`;

            const body = open
                ? (entry.results.length
                    ? entry.results.map((result) => `<pre class="console-output">${escapeHTML(result.result ?? '')}</pre>`).join('')
                    : `<p class="history-pending">${entry.state === 'pending'
                        ? 'Queued, waiting for the agent to check in.'
                        : 'Sent, but the agent has not reported a result yet.'}</p>`)
                : '';

            return `
                <article class="history-entry">
                    <button class="history-entry-head" type="button" data-history-toggle="${escapeHTML(entry.taskId)}"
                        aria-expanded="${open ? 'true' : 'false'}" aria-label="${escapeHTML(label)}">
                        <span class="history-caret" aria-hidden="true">&#9656;</span>
                        <span class="history-command">${escapeHTML(entry.command)}</span>
                        ${entry.argsText ? `<span class="history-args">${escapeHTML(entry.argsText)}</span>` : ''}
                        <span class="console-state" data-state="${entry.state}">${entry.state}</span>
                        <span class="history-time">${entry.time ? escapeHTML(formatTimeAgo(entry.time)) : ''}</span>
                    </button>
                    ${body ? `<div class="history-body">${body}</div>` : ''}
                </article>
            `;
        }).join('');
    };

    const loadHistory = async () => {
        if (!history_.agentId) return;

        els.refreshHistoryButton.disabled = true;
        historyMessage('Loading history…');

        try {
            const [tasks, results] = await Promise.all([
                loadJSON(`/api/tasks?agent=${encodeURIComponent(history_.agentId)}`),
                fetchHistoryResults(),
            ]);

            history_.entries = buildHistoryEntries(tasks.tasks || [], results);
            history_.entries.forEach((entry) => { entry.state = historyState(entry); });

            // Drop expansion state for tasks that no longer exist so the set cannot
            // grow unbounded across refreshes.
            const live = new Set(history_.entries.map((entry) => entry.taskId));
            history_.expanded.forEach((id) => { if (!live.has(id)) history_.expanded.delete(id); });

            renderHistory();
            historyMessage('');
        } catch (error) {
            if (error.message === 'Unauthorized') return;
            history_.entries = [];
            renderHistory();
            historyMessage(error.message || 'Could not load command history.', 'error');
        } finally {
            if (history_.open) els.refreshHistoryButton.disabled = false;
        }
    };

    const openHistory = async (agentId) => {
        const summary = state.agents.find((agent) => agent.id === agentId);
        if (!summary) return;

        history_.agentId = agentId;
        history_.agent = summary;
        history_.entries = [];
        history_.expanded = new Set();
        history_.filter = '';
        history_.open = true;

        els.historyModal.classList.add('is-open');
        els.historyModal.classList.remove('hidden');
        els.historyModal.setAttribute('aria-hidden', 'false');
        els.historyTitle.textContent = 'Command history';
        els.historySubtitle.textContent = `${summary.implant_id}@${summary.ip} · ${summary.os} · last check-in ${formatTimeAgo(summary.last_checkin)}`;
        els.historySearch.value = '';

        await loadHistory();

        if (history_.open) els.historySearch.focus();
    };

    const closeHistory = () => {
        history_.open = false;
        history_.agent = null;
        history_.agentId = null;
        history_.entries = [];
        history_.expanded = new Set();
        els.historyModal.classList.remove('is-open');
        els.historyModal.classList.add('hidden');
        els.historyModal.setAttribute('aria-hidden', 'true');
    };

    // A command is offered only when every selected agent advertises it, so the
    // send can never fan out to an agent that cannot run what was chosen.
    const commonCommands = () => {
        const agents = selectedAgents();
        if (!agents.length) return [];

        const commandMaps = agents.map((agent) => new Map(getCommands(agent).map((cmd) => [cmd.command, cmd])));
        const [firstMap, ...rest] = commandMaps;
        const commands = [];

        for (const [name, command] of firstMap.entries()) {
            if (rest.every((map) => map.has(name))) {
                commands.push(command);
            }
        }

        return commands.sort((a, b) => a.command.localeCompare(b.command));
    };

    const buildSelectableAgents = () => {
        const query = state.filters.search.trim().toLowerCase();
        return state.agents.filter((agent) => {
            const tags = Array.isArray(agent.tags) ? agent.tags : [];
            const matchesWildcardIp = query.includes('*') ? patternMatches(agent.ip || '', query) : false;
            const matchesSearch = !query || matchesWildcardIp || [
                agent.ip,
                agent.id,
                agent.implant_id,
                agent.os,
                ...tags,
            ].some((value) => String(value || '').toLowerCase().includes(query));
            const matchesImplant = state.filters.implant === 'all' || agent.implant_id === state.filters.implant;
            const matchesOs = state.filters.os === 'all' || agent.os === state.filters.os;
            return matchesSearch && matchesImplant && matchesOs;
        });
    };

    const patternMatches = (value, pattern) => {
        const escaped = pattern.trim().replace(/[.+?^${}()|[\]\\]/g, '\\$&').replace(/\*/g, '.*');
        const regex = new RegExp(`^${escaped}$`, 'i');
        return regex.test(value);
    };

    const updateFilterOptions = () => {
        const implants = [...new Set(state.agents.map((agent) => agent.implant_id).filter(Boolean))].sort((a, b) => a.localeCompare(b));
        const osValues = [...new Set(state.agents.map((agent) => agent.os).filter(Boolean))].sort((a, b) => a.localeCompare(b));

        els.implantFilter.innerHTML = ['<option value="all">All implant types</option>', ...implants.map((implant) => `<option value="${implant}">${implant}</option>`)].join('');
        els.osFilter.innerHTML = ['<option value="all">All operating systems</option>', ...osValues.map((os) => `<option value="${os}">${os}</option>`)].join('');
    };

    const renderSelection = () => {
        const agents = selectedAgents();

        els.selectionCount.textContent = `${agents.length} selected`;
        // No empty-state text under the bar: the count already reads "0 selected"
        // and the table directly below shows every agent unticked. .selection-chips
        // reserves min-height, so this leaves a stable gap rather than making the
        // table jump when the first box is ticked.
        els.selectionChips.innerHTML = agents
            .map((agent) => `<span class="selection-chip">${escapeHTML(labelFor(agent))}<button type="button" data-remove-selection="${escapeHTML(agent.id)}" aria-label="Remove ${escapeHTML(labelFor(agent))}">×</button></span>`)
            .join('');

        els.agentsTableBody.querySelectorAll('input[type="checkbox"]').forEach((checkbox) => {
            checkbox.checked = state.selectedIds.has(checkbox.dataset.agentId);
        });
    };

    // Read-only echo of the table selection, styled with the same chip pattern as
    // the selection bar above the table so the two are recognisably the same thing.
    const renderBulkTargets = () => {
        const agents = selectedAgents();
        const total = agents.length;

        els.bulkTargetCount.textContent = `${total} selected`;
        els.bulkTargetChips.innerHTML = total
            ? agents.map((agent) => `<span class="selection-chip">${escapeHTML(labelFor(agent))}</span>`).join('')
            : '<span class="empty-state">No agents selected.</span>';

        // Naming the fan-out on the button that performs it: this sends one command
        // to every selected agent, so the count is the part that matters. Zero is
        // unreachable (the dialog refuses to open unselected) so it just reads
        // "Send task" rather than claiming to target nobody.
        els.submitBulkTaskButton.textContent = total
            ? `Send task to ${total} ${total === 1 ? 'agent' : 'agents'}`
            : 'Send task';

        const commands = commonCommands();
        const currentCommandExists = commands.some((cmd) => cmd.command === state.bulkCommand);
        if (!currentCommandExists) {
            state.bulkCommand = commands[0]?.command || '';
        }

        els.bulkCommand.innerHTML = commands.length
            ? commands.map((cmd) => `<option value="${escapeHTML(cmd.command)}">${escapeHTML(cmd.command)}</option>`).join('')
            : '<option value="">No common commands</option>';

        if (state.bulkCommand) {
            els.bulkCommand.value = state.bulkCommand;
        }

        renderBulkArgs();
    };

    const renderBulkArgs = () => {
        const command = commonCommands().find((cmd) => cmd.command === state.bulkCommand);
        const args = command?.args || [];

        if (!command) {
            els.bulkArgsContainer.innerHTML = '<div class="empty-state">No command is shared by every selected agent.</div>';
            return;
        }

        els.bulkArgsContainer.innerHTML = `
            <div class="arg-list">
                ${args.map((arg, index) => `
                    <div class="arg-item">
                        <label class="field-label" for="bulkArg-${index}">${escapeHTML(arg.name)}${arg.required ? ' *' : ''}</label>
                        <input class="input" id="bulkArg-${index}" type="text" data-bulk-arg="${index}" placeholder="${escapeHTML(arg.description || arg.name)}">
                        <div class="arg-help">${escapeHTML([arg.description, arg.arg_type].filter(Boolean).join(' · '))}</div>
                    </div>
                `).join('') || '<div class="empty-state">This command does not require arguments.</div>'}
            </div>
        `;
    };

    const renderTable = () => {
        const agents = buildSelectableAgents();
        state.filteredAgents = agents;

        if (!agents.length) {
            // Distinguishes "you deleted the last one" from "your filters exclude
            // everything", which otherwise read as the same thing.
            const empty = state.agents.length
                ? 'No agents match the current filters.'
                : 'No agents are registered.';
            els.agentsTableBody.innerHTML = `<tr><td colspan="8"><div class="empty-state">${empty}</div></td></tr>`;
            renderSelection();
            return;
        }

        els.agentsTableBody.innerHTML = agents.map((agent) => {
            const commands = getCommands(agent);
            const label = `${agent.implant_id}@${agent.ip}`;
            return `
                <tr data-agent-id="${agent.id}" tabindex="0" aria-label="${escapeHTML(label)} — activate to open the interact console">
                    <td class="table-check">
                        <input class="row-checkbox" type="checkbox" data-agent-id="${agent.id}" ${state.selectedIds.has(agent.id) ? 'checked' : ''}>
                    </td>
                    <td>
                        <div class="agent-meta">
                            <div class="agent-ip">${escapeHTML(agent.ip)}</div>
                            <div class="agent-id">${escapeHTML(agent.id)}</div>
                        </div>
                    </td>
                    <td><span class="mini-chip">${escapeHTML(agent.implant_id)}</span></td>
                    <td><span class="mini-chip muted">${escapeHTML(agent.os)}</span></td>
                    <td>
                        <div class="agent-tags">
                            ${(agent.tags || []).length ? agent.tags.map((tag) => `<span class="mini-chip">${escapeHTML(tag)}</span>`).join('') : '<span class="mini-chip muted">No tags</span>'}
                        </div>
                    </td>
                    <td class="agent-checkin">${formatTimeAgo(agent.last_checkin)}</td>
                    <td>
                        <div class="agent-command-list">
                            <span class="mini-chip">${commands.length} ${commands.length === 1 ? 'command' : 'commands'}</span>
                        </div>
                    </td>
                    <td class="table-actions-cell">
                        <div class="table-actions">
                            <button class="button-secondary agent-interact" type="button" data-interact-id="${agent.id}">Interact</button>
                            <button class="button-secondary agent-history" type="button" data-history-id="${agent.id}">History</button>
                            <button class="button agent-delete" type="button" data-delete-id="${agent.id}">Delete</button>
                        </div>
                    </td>
                </tr>
            `;
        }).join('');

        renderSelection();
    };

    const refreshSelectionFromTable = () => {
        const checked = [...els.agentsTableBody.querySelectorAll('input.row-checkbox:checked')].map((input) => input.dataset.agentId);
        state.selectedIds = new Set(checked);
        renderSelection();
    };

    const openModal = () => {
        els.bulkTaskModal.classList.add('is-open');
        els.bulkTaskModal.classList.remove('hidden');
        els.bulkTaskModal.setAttribute('aria-hidden', 'false');
        renderBulkTargets();
        setMessage('');
    };

    const closeModal = () => {
        els.bulkTaskModal.classList.remove('is-open');
        els.bulkTaskModal.classList.add('hidden');
        els.bulkTaskModal.setAttribute('aria-hidden', 'true');
    };

    const renderServer = () => {
        els.serverChip.textContent = state.serverUrl.replace(/^https?:\/\//, '');
    };

    const loadAgents = async () => {
        const payload = await loadJSON('/api/agents');
        state.agents = (payload.agents || []).sort((a, b) => {
            const ipCompare = (a.ip || '').localeCompare(b.ip || '', undefined, { numeric: true, sensitivity: 'base' });
            if (ipCompare !== 0) return ipCompare;
            return (a.implant_id || '').localeCompare(b.implant_id || '', undefined, { sensitivity: 'base' });
        });
        // Forget selected ids the server no longer knows about. The chips would
        // disappear on their own, since selectedAgents intersects with
        // state.agents - but leaving the ids behind means any agent that comes
        // back under the same id reappears pre-ticked, silently swept into the
        // next bulk task.
        const live = new Set(state.agents.map((agent) => agent.id));
        state.selectedIds.forEach((id) => {
            if (!live.has(id)) state.selectedIds.delete(id);
        });
        updateFilterOptions();
        renderTable();
    };

    // Kept apart from loadAgents because the two failure modes differ. A failed
    // first load has nothing to show, but a failed refresh still has a table full
    // of agents the user can act on - blanking it to display an error would throw
    // away good data over a transient network blip. So a refresh reports through
    // the page error line and leaves the table alone.
    let refreshing = false;

    const refreshAgents = async () => {
        if (refreshing) return;
        refreshing = true;
        els.refreshAgentsButton.disabled = true;
        setError('');

        try {
            // Filters, search text and the open history/interact modals are all
            // held outside loadAgents, so a refresh leaves them exactly as they
            // were.
            await loadAgents();
        } catch (error) {
            setError(error.message || 'Failed to refresh agents.');
        } finally {
            refreshing = false;
            els.refreshAgentsButton.disabled = false;
        }
    };

    const sendBulkTasks = async () => {
        const agents = selectedAgents();
        const command = commonCommands().find((cmd) => cmd.command === state.bulkCommand);
        if (!agents.length) {
            setMessage('No agents selected.', 'error');
            return;
        }
        if (!command) {
            setMessage('No common command available for the selected agents.', 'error');
            return;
        }

        const schema = command.args || [];
        const args = collectTypedArgs(els.bulkArgsContainer, schema, setMessage);
        if (!args) return;

        const requiredCount = schema.filter((arg) => arg.required).length;
        if (args.length !== requiredCount) {
            setMessage('Required arguments are missing.', 'error');
            return;
        }

        setMessage('Sending tasks…');
        try {
            const results = await Promise.all(agents.map((agent) => apiFetch('/api/tasks', {
                method: 'POST',
                body: JSON.stringify({
                    agent_id: agent.id,
                    command: command.command,
                    args,
                }),
            })));

            const failed = results.filter((response) => !response.ok).length;
            if (failed) {
                setMessage(`Completed with ${failed} failed ${failed === 1 ? 'task' : 'tasks'}.`, 'error');
            } else {
                setMessage(`Task sent to ${agents.length} ${agents.length === 1 ? 'agent' : 'agents'}.`, 'success');
            }
        } catch (error) {
            setMessage(error.message || 'Failed to send tasks.', 'error');
        }
    };

    // --- Delete agent -------------------------------------------------------
    // DELETE /api/agents/<id> removes only the agent row: management_routes.py
    // delete_agent does not cascade to tasks. The survivors then make the unscoped
    // Task.serialize() dereference a missing agent, so GET /api/tasks returns 500
    // and the Overview page loses its task counts. The server is out of scope for
    // this work, so the dialog states the consequence rather than implying the
    // delete is clean.
    const delete_ = {
        agentId: null,
        agent: null,
        busy: false,
    };

    const deleteMessage = (message, tone = '') => {
        if (!els.deleteAgentMessage) return;
        els.deleteAgentMessage.textContent = message;
        els.deleteAgentMessage.dataset.tone = tone;
    };

    const openDelete = (agentId) => {
        const agent = state.agents.find((candidate) => candidate.id === agentId);
        if (!agent) return;

        delete_.agentId = agentId;
        delete_.agent = agent;
        delete_.busy = false;

        // Spelled out rather than shown as a bare UUID, so there is no way to
        // confirm the wrong row.
        els.deleteTargetDetails.innerHTML = [
            ['Agent', labelFor(agent)],
            ['ID', agent.id],
            ['OS', agent.os],
            ['Last check-in', formatTimeAgo(agent.last_checkin)],
            ['Tags', (agent.tags || []).length ? agent.tags.join(', ') : 'none'],
            ['Commands', String(getCommands(agent).length)],
        ].map(([term, detail]) => `<dt>${escapeHTML(term)}</dt><dd>${escapeHTML(detail)}</dd>`).join('');

        els.confirmDeleteAgentButton.disabled = false;
        els.confirmDeleteAgentButton.textContent = 'Delete agent';
        deleteMessage('');

        els.deleteAgentModal.classList.add('is-open');
        els.deleteAgentModal.classList.remove('hidden');
        els.deleteAgentModal.setAttribute('aria-hidden', 'false');
        // Focus the safe option, so a stray Enter cannot destroy the agent.
        els.cancelDeleteAgentButton.focus();
    };

    const closeDelete = () => {
        delete_.agentId = null;
        delete_.agent = null;
        delete_.busy = false;
        els.deleteAgentModal.classList.remove('is-open');
        els.deleteAgentModal.classList.add('hidden');
        els.deleteAgentModal.setAttribute('aria-hidden', 'true');
    };

    const deleteAgent = async () => {
        if (delete_.busy || !delete_.agentId) return;
        const agentId = delete_.agentId;

        delete_.busy = true;
        els.confirmDeleteAgentButton.disabled = true;
        els.confirmDeleteAgentButton.textContent = 'Deleting…';
        deleteMessage('');

        try {
            const response = await apiFetch(`/api/agents/${encodeURIComponent(agentId)}`, { method: 'DELETE' });

            if (!response.ok) {
                // Success is a 204 with no body, so the body is only parsed on
                // failure, where the server does send a message.
                const payload = await response.json().catch(() => ({}));
                throw new Error(payload.message || `Delete failed (HTTP ${response.status}).`);
            }

            // Dropped locally instead of reloading: the agent is gone from the
            // server, and a reload would also pull the unscoped /api/tasks that
            // this very delete has just broken.
            state.agents = state.agents.filter((agent) => agent.id !== agentId);
            state.selectedIds.delete(agentId);
            closeDelete();
            renderTable();
        } catch (error) {
            deleteMessage(error.message || 'Failed to delete the agent.', 'error');
        } finally {
            delete_.busy = false;
            els.confirmDeleteAgentButton.disabled = false;
            els.confirmDeleteAgentButton.textContent = 'Delete agent';
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
        els.agentSearch.addEventListener('input', (event) => {
            state.filters.search = event.target.value;
            renderTable();
        });

        els.implantFilter.addEventListener('change', (event) => {
            state.filters.implant = event.target.value;
            renderTable();
        });

        els.osFilter.addEventListener('change', (event) => {
            state.filters.os = event.target.value;
            renderTable();
        });

        els.agentsTableBody.addEventListener('change', (event) => {
            if (event.target.matches('.row-checkbox')) {
                refreshSelectionFromTable();
            }
        });

        // Clicking anywhere on a row opens the interaction console, except the checkbox
        // which is reserved for bulk selection. The History button is checked first so
        // that opening the history does not also fire the row's own action.
        els.agentsTableBody.addEventListener('click', (event) => {
            if (event.target.closest('.row-checkbox')) return;

            const historyButton = event.target.closest('[data-history-id]');
            if (historyButton) {
                event.stopPropagation();
                openHistory(historyButton.dataset.historyId);
                return;
            }

            // Checked before the row fallback, or deleting a row would also open
            // the interact console behind the confirmation dialog.
            const deleteButton = event.target.closest('[data-delete-id]');
            if (deleteButton) {
                event.stopPropagation();
                openDelete(deleteButton.dataset.deleteId);
                return;
            }

            const row = event.target.closest('tr[data-agent-id]');
            if (!row) return;
            openInteract(row.dataset.agentId);
        });

        els.agentsTableBody.addEventListener('keydown', (event) => {
            if (event.key !== 'Enter' && event.key !== ' ') return;
            // A focused button already turns Enter/Space into its own click event, so
            // handling it here too would open the console behind the history modal.
            if (event.target.closest('button')) return;
            const row = event.target.closest('tr[data-agent-id]');
            if (!row) return;
            event.preventDefault();
            openInteract(row.dataset.agentId);
        });

        els.selectionChips.addEventListener('click', (event) => {
            const button = event.target.closest('[data-remove-selection]');
            if (!button) return;
            state.selectedIds.delete(button.dataset.removeSelection);
            renderTable();
        });

        els.clearSelectionButton.addEventListener('click', () => {
            state.selectedIds.clear();
            renderTable();
        });

        els.selectFilteredButton.addEventListener('click', () => {
            buildSelectableAgents().forEach((agent) => state.selectedIds.add(agent.id));
            renderTable();
        });

        els.openBulkTaskButton.addEventListener('click', () => {
            if (!state.selectedIds.size) {
                setMessage('Select at least one agent first.', 'error');
                return;
            }
            openModal();
        });

        els.closeBulkTaskButton.addEventListener('click', closeModal);
        els.cancelBulkTaskButton.addEventListener('click', closeModal);
        els.bulkTaskModal.addEventListener('click', (event) => {
            if (event.target === els.bulkTaskModal) {
                closeModal();
            }
        });

        els.bulkCommand.addEventListener('change', (event) => {
            state.bulkCommand = event.target.value;
            renderBulkArgs();
        });

        els.refreshAgentsButton.addEventListener('click', refreshAgents);
        els.submitBulkTaskButton.addEventListener('click', sendBulkTasks);

        els.closeDeleteAgentButton.addEventListener('click', closeDelete);
        els.cancelDeleteAgentButton.addEventListener('click', closeDelete);
        els.confirmDeleteAgentButton.addEventListener('click', deleteAgent);
        els.deleteAgentModal.addEventListener('click', (event) => {
            if (event.target === els.deleteAgentModal) closeDelete();
        });
        document.addEventListener('keydown', (event) => {
            if (event.key === 'Escape' && delete_.agentId) closeDelete();
        });

        els.closeInteractButton.addEventListener('click', closeInteract);
        els.cancelInteractButton.addEventListener('click', closeInteract);
        els.interactModal.addEventListener('click', (event) => {
            if (event.target === els.interactModal) closeInteract();
        });

        els.closeHistoryButton.addEventListener('click', closeHistory);
        els.doneHistoryButton.addEventListener('click', closeHistory);
        els.historyModal.addEventListener('click', (event) => {
            if (event.target === els.historyModal) closeHistory();
        });
        els.refreshHistoryButton.addEventListener('click', loadHistory);

        els.historySearch.addEventListener('input', (event) => {
            history_.filter = event.target.value || '';
            renderHistory();
        });

        // One delegated listener on the list, so toggling never needs a rebind and
        // the collapsed/expanded state is re-read from history_ on every render.
        els.historyList.addEventListener('click', (event) => {
            const toggle = event.target.closest('[data-history-toggle]');
            if (!toggle) return;
            const id = toggle.dataset.historyToggle;
            if (history_.expanded.has(id)) history_.expanded.delete(id);
            else history_.expanded.add(id);
            renderHistory();
        });
        els.clearInteractButton.addEventListener('click', () => {
            console_.entries = [];
            interactMessage('');
            renderInteractLog();
        });
        els.interactCommand.addEventListener('change', (event) => {
            state.interactCommand = event.target.value;
            renderInteractCommand();
        });
        els.sendInteractButton.addEventListener('click', sendInteractTask);
    };

    const cacheElements = () => {
        els.themeToggle = document.getElementById('themeToggle');
        els.logoutButton = document.getElementById('logoutButton');
        els.serverChip = document.getElementById('serverChip');
        els.agentSearch = document.getElementById('agentSearch');
        els.implantFilter = document.getElementById('implantFilter');
        els.osFilter = document.getElementById('osFilter');
        els.selectionCount = document.getElementById('selectionCount');
        els.selectionChips = document.getElementById('selectionChips');
        els.clearSelectionButton = document.getElementById('clearSelectionButton');
        els.selectFilteredButton = document.getElementById('selectFilteredButton');
        els.openBulkTaskButton = document.getElementById('openBulkTaskButton');
        els.agentsTableBody = document.getElementById('agentsTableBody');
        els.bulkTaskModal = document.getElementById('bulkTaskModal');
        els.closeBulkTaskButton = document.getElementById('closeBulkTaskButton');
        els.cancelBulkTaskButton = document.getElementById('cancelBulkTaskButton');
        els.bulkTargetCount = document.getElementById('bulkTargetCount');
        els.bulkTargetChips = document.getElementById('bulkTargetChips');
        els.bulkCommand = document.getElementById('bulkCommand');
        els.pageError = document.getElementById('pageError');
        els.refreshAgentsButton = document.getElementById('refreshAgentsButton');
        els.bulkArgsContainer = document.getElementById('bulkArgsContainer');
        els.bulkTaskMessage = document.getElementById('bulkTaskMessage');
        els.submitBulkTaskButton = document.getElementById('submitBulkTaskButton');
        els.deleteAgentModal = document.getElementById('deleteAgentModal');
        els.closeDeleteAgentButton = document.getElementById('closeDeleteAgentButton');
        els.cancelDeleteAgentButton = document.getElementById('cancelDeleteAgentButton');
        els.confirmDeleteAgentButton = document.getElementById('confirmDeleteAgentButton');
        els.deleteTargetDetails = document.getElementById('deleteTargetDetails');
        els.deleteAgentMessage = document.getElementById('deleteAgentMessage');
        els.interactModal = document.getElementById('interactModal');
        els.interactTitle = document.getElementById('interactTitle');
        els.interactSubtitle = document.getElementById('interactSubtitle');
        els.interactAgentChip = document.getElementById('interactAgentChip');
        els.interactHint = document.getElementById('interactHint');
        els.interactLog = document.getElementById('interactLog');
        els.interactCommand = document.getElementById('interactCommand');
        els.interactCommandHelp = document.getElementById('interactCommandHelp');
        els.interactArgsContainer = document.getElementById('interactArgsContainer');
        els.interactMessage = document.getElementById('interactMessage');
        els.sendInteractButton = document.getElementById('sendInteractButton');
        els.clearInteractButton = document.getElementById('clearInteractButton');
        els.closeInteractButton = document.getElementById('closeInteractButton');
        els.cancelInteractButton = document.getElementById('cancelInteractButton');
        els.historyModal = document.getElementById('historyModal');
        els.historyTitle = document.getElementById('historyTitle');
        els.historySubtitle = document.getElementById('historySubtitle');
        els.historySearch = document.getElementById('historySearch');
        els.historySummary = document.getElementById('historySummary');
        els.historyList = document.getElementById('historyList');
        els.historyMessage = document.getElementById('historyMessage');
        els.refreshHistoryButton = document.getElementById('refreshHistoryButton');
        els.closeHistoryButton = document.getElementById('closeHistoryButton');
        els.doneHistoryButton = document.getElementById('doneHistoryButton');
    };

    const main = async () => {
        if (!requireAuth()) return;
        cacheElements();
        initializeTheme();
        initializeLogout();
        initializeHandlers();
        renderServer();

        try {
            await loadAgents();
        } catch (error) {
            // setError, not setMessage: on a first load there is genuinely nothing
            // to show, so the table is replaced, but the reason has to land
            // somewhere the user can actually read it.
            setError(error.message || 'Failed to load agents.');
            els.agentsTableBody.innerHTML = '<tr><td colspan="8"><div class="empty-state">Unable to load agents.</div></td></tr>';
        }
    };

    document.addEventListener('DOMContentLoaded', main);
})();
