import utils

const
    IMPLANT_ID*: string = obf("tombili")
    SERVER_BASE_URL*: string = obf("http://127.0.0.1:3000/")
    REG_FAIL_LIMIT*: int = 10 # 0 for no limit
    REG_FAIL_RETRY_SLEEP*: int = 10 # seconds
    CHECKIN_SLEEP*: int = 10 # seconds
    CHECKIN_SLEEP_JITTER*: int = 5 # seconds
    XOR_KEY*: string = obf("asdf") # must be a key in the server's key list
