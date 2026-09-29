import net
import strutils

import utils

type
    PortScanResult* = object
        ip*: string
        port*: int
        status*:string

proc expandIpRange*(ipOrCidr: string): seq[string] =
    ## Expand IP address or CIDR notation into list of IPs
    result = @[]

    if "/" in ipOrCidr:
        # CIDR notation - expand subnet
        let parts = ipOrCidr.split("/")
        if parts.len != 2:
            return @[ipOrCidr]  # Invalid, return as-is

        var baseIp = parts[0]
        var prefixLen = try: parseInt(parts[1]) except: 32

        # For simplicity, only support /24 and larger subnets
        if prefixLen >= 24:
            let ipParts = baseIp.split(".")
            if ipParts.len != 4:
                return @[ipOrCidr]
            
            var base = ipParts[0] & "." & ipParts[1] & "." & ipParts[2] & "."
            var hostBits = 32 - prefixLen
            var numHosts = 1 shl hostBits
            
            for i in 1..<numHosts-1:  # Skip network and broadcast
                result.add(base & $i)
        else:
            # For /16 or larger, just return the base IP to avoid huge scans
            result.add(baseIp)
    else:
        result.add(ipOrCidr)

proc expandPortRange*(portStr: string): seq[int] =
    ## Expand port range string into list of ports
    ## Supports: "80", "80,443", "80-85", "80,443,1000-1005"
    result = @[]
  
    var parts = portStr.split(",")
    for part in parts:
        let trimmed = part.strip()
        if "-" in trimmed:
            let rangeParts = trimmed.split("-")
            if rangeParts.len == 2:
                let start = try: parseInt(rangeParts[0].strip()) except: 0
                let stop = try: parseInt(rangeParts[1].strip()) except: 0
                if start > 0 and stop > 0 and start <= stop:
                    for p in start..stop:
                        if p > 0 and p <= 65535:
                            result.add(p)
        else:
            let port = try: parseInt(trimmed) except: 0
            if port > 0 and port <= 65535:
                result.add(port)

proc scanPorts*(ips: seq[string], ports: seq[int]): seq[PortScanResult] =
    var results: seq[PortScanResult]
    var portStatus: string

    for ip in ips:
        for port in ports:
            try:
                var socket: Socket = newSocket()
                socket.connect(ip, Port(port), timeout=100)
                socket.close()
                portStatus = obf("OPEN")
            except:
                portStatus = obf("CLOSED")

            results.add(PortScanResult(ip: ip, port: port, status: portStatus))

    return results
