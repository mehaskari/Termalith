---
description: Serial consoles, Telnet and the built-in network tools of Termalith — ping, traceroute, TCP port check, port scan, DNS lookup, subnet calculator and Wake-on-LAN.
---

# Serial, Telnet and network tools

## Serial consoles

**Serial** at the top of the sidebar opens a console on a COM port (Windows)
or `/dev/tty…` device (macOS, Linux). Choose the port, speed, data bits,
parity, stop bits and flow control; 9600 8N1 is the usual setting for
network equipment. On Linux your user usually has to be in the `dialout`
group (`sudo usermod -aG dialout $USER`, then log in again).

## Telnet

For devices that offer nothing else, create a session of type **Telnet
(unencrypted)**. Everything you type, including passwords, can be read on
the network. Choose a jump host (**Connect via**) so that only the last leg,
from the jump host to the device, is unencrypted. Telnet can also go
through the proxy in [Settings](settings.md).

## Network tools

**Tools** in the sidebar footer:

![Network tools](../assets/screenshots/network-tools.png)

| Tool | What it does |
|---|---|
| Ping | Round-trip times to a host. |
| Traceroute | The route to a host, hop by hop. |
| TCP port check | Whether one port on a host accepts connections. |
| Port scan | Open TCP ports on a host (up to 1024 ports, 64 at a time). Only scan hosts you are allowed to test. |
| DNS lookup | The addresses a name resolves to. |
| Subnet calculator | Network, netmask, wildcard, broadcast and host range for an IPv4 address and prefix. |
| Wake-on-LAN | Sends a magic packet to a MAC address, optionally to a given broadcast address. |
