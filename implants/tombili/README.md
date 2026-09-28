# Tombili Agent

## Configuring
Change the values in `src/config.nim` as needed. 

## Compiling
```bash
nim c -d:release -d:ssl -o:tombili.bin src/main.nim

# If compiling for Windows from MacOS or Linux:
nim c -d:release -d:ssl -o:tombili.exe src/main.nim
```

## Commands
|Command|Platform|Description|Arguments|
|---|---|---|---|
|`shell`|Windows, Linux|Run a command|command: str|
|`getsleep`|Windows, Linux|Get the current sleep time||
|`setsleep`|Windows, Linux|Set the sleep time between callbacks|sleep: float, jitter: float|
|`whoami`|Windows, Linux|Get the user that Tombili is running as||
|`cat`|Windows, Linux|Read the contents of a file|path: str|
|`fileinfo`|Windows, Linux|Get information about a file|path: str|
|`mv`|Windows, Linux|Move a file or directory|source: str, destination: str|
|`rm`|Windows, Linux|Delete a file|path: str|
|`getenv`|Windows, Linux|Get all environment variables||
|`pid`|Windows, Linux|Get the ID of the current process||
|`reverseshell`|Windows, Linux|Start a reverse shell|ip: str, port: int|
|`msgbox`|Windows|Display a message box|title: str, body: str|
|`getclipboard`|Windows|Get the contents of the clipboard||
|`kill`|Windows, Linux|Kill the agent||
